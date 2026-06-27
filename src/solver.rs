use std::collections::HashMap;
use std::time::{Duration, Instant};

use chaser_cf::core::BrowserManager;
use chaser_cf::{Cookie, ProxyConfig};
use chaser_oxide::auth::Credentials;
use chaser_oxide::cdp::browser_protocol::dom::{
    GetBoxModelParams, GetDocumentParams, Node, NodeId,
};
use chaser_oxide::cdp::browser_protocol::network;
use chaser_oxide::layout::Point;
use chaser_oxide::{ChaserPage, Page};

use crate::error::{FlareSolverError, Result};
use crate::models::{FetchResponse, RequestCookie, ResponseCookie};

#[cfg(feature = "server")]
use crate::session::SessionStore;
#[cfg(feature = "server")]
use std::sync::Arc;

/// Parse "scheme://[user:pass@]host:port" into a chaser_cf ProxyConfig.
pub fn parse_proxy_url(url: &str) -> Result<ProxyConfig> {
    let (scheme, rest) = url.split_once("://").ok_or_else(|| {
        FlareSolverError::Browser("proxy URL must include a scheme (e.g. http://host:port)".into())
    })?;

    let (creds, hostport) = if let Some(at) = rest.rfind('@') {
        (Some(&rest[..at]), &rest[at + 1..])
    } else {
        (None, rest)
    };

    let (host, port_str) = hostport
        .rsplit_once(':')
        .ok_or_else(|| FlareSolverError::Browser("proxy URL missing port".into()))?;
    let port = port_str
        .parse::<u16>()
        .map_err(|_| FlareSolverError::Browser("proxy URL has invalid port".into()))?;

    let mut cfg = ProxyConfig::new(host, port).with_scheme(scheme);
    if let Some(creds) = creds {
        if let Some((user, pass)) = creds.split_once(':') {
            cfg = cfg.with_auth(user, pass);
        }
    }
    Ok(cfg)
}

/// Convert a chaser_cf Cookie into a ResponseCookie.
pub fn chaser_cookie_to_response(c: Cookie) -> ResponseCookie {
    let is_session = c.expires.is_none();
    ResponseCookie {
        name: c.name,
        value: c.value,
        domain: c.domain.unwrap_or_default(),
        path: c.path.unwrap_or_else(|| "/".into()),
        expiry: c.expires,
        http_only: c.http_only.unwrap_or(false),
        secure: c.secure.unwrap_or(false),
        session: is_session,
        same_party: false,
        store_id: "0".into(),
    }
}

/// Map a CDP network cookie (from the live browser) onto a chaser_cf Cookie.
fn oxide_to_chaser_cookie(c: network::Cookie) -> Cookie {
    Cookie {
        name: c.name,
        value: c.value,
        domain: Some(c.domain),
        path: Some(c.path),
        // CDP reports -1 for "no expiry" (a session cookie); chaser_cf uses None.
        expires: if c.expires < 0.0 {
            None
        } else {
            Some(c.expires)
        },
        http_only: Some(c.http_only),
        secure: Some(c.secure),
        same_site: c.same_site.map(|s| format!("{s:?}")),
    }
}

pub const DEFAULT_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

/// Init script that makes `navigator.languages` consistent between the main
/// thread and Web Workers.
///
/// chaser-oxide's native profile applies a CDP `setUserAgentOverride` with
/// `accept_language` (e.g. `"en-US"`), which collapses the **main** thread's
/// `navigator.languages` to a single entry (`["en-US"]`). That override does
/// **not** reach worker contexts, where `navigator.languages` keeps Chrome's
/// natural expansion (`["en-US","en"]`). Fingerprint WAFs spawn a worker and
/// compare its `navigator` against the page's; the mismatch
/// (`hasInconsistentWorkerValues`) was the lone signal labelling the stealth
/// browser "a bot" on deviceandbrowserinfo.com.
///
/// We fix it on the **main thread only**: redefine `navigator.languages` to the
/// natural `[lang, base]` expansion the workers already report, so both sides
/// agree without touching any worker.
///
/// An earlier version wrapped `Worker` to inject the values into each worker via
/// a `blob:` script + `importScripts`. That broke WAF bypass: Cloudflare and
/// Turnstile challenge pages forbid `blob:` workers through a `worker-src` CSP,
/// so the wrapped worker was blocked and their proof-of-work never ran — the
/// challenges silently failed to solve. The main-thread override avoids all
/// worker tampering and so cannot break challenge pages.
const LANGUAGES_CONSISTENCY_JS: &str = r#"(function () {
  try {
    var lang = navigator.language || 'en-US';
    var base = lang.indexOf('-') > 0 ? lang.split('-')[0] : lang;
    var list = base === lang ? [lang] : [lang, base];
    Object.defineProperty(Navigator.prototype, 'languages', {
      get: function () { return list.slice(); },
      configurable: true
    });
  } catch (e) {}
})();"#;

/// Return true if a page title looks like a bot/WAF interstitial (Cloudflare,
/// PerimeterX, generic "access denied", etc.) rather than real content.
pub fn is_challenge_title(title: &str) -> bool {
    let t = title.to_ascii_lowercase();
    const MARKERS: [&str; 10] = [
        "just a moment",
        "attention required",
        "checking your browser",
        "are you a robot",
        "verifying you are human",
        "please wait",
        "access denied",
        "ddos",
        "one moment",
        "security check",
    ];
    MARKERS.iter().any(|m| t.contains(m))
}

/// True when a navigation error matches the proxy/auth failures a *cold* first
/// proxied request can produce: the initial proxied CONNECT can race ahead of
/// the CDP Fetch auth-interception that `authenticate()` enables, so Chrome
/// answers the proxy 407 with no credentials and the tunnel fails. These clear
/// on a second attempt (the handler is live by then), so [`fetch`] retries once.
pub fn is_proxy_retryable_error(msg: &str) -> bool {
    let m = msg.to_ascii_uppercase();
    [
        "ERR_TUNNEL_CONNECTION_FAILED",
        "ERR_PROXY_CONNECTION_FAILED",
        "ERR_INVALID_AUTH_CREDENTIALS",
        "ERR_PROXY_AUTH_REQUESTED",
        "ERR_NO_SUPPORTED_PROXIES",
    ]
    .iter()
    .any(|needle| m.contains(needle))
}

/// Snapshot of the live page used to decide when a fetch has settled.
struct PageState {
    /// `document.readyState === 'complete'`.
    ready: bool,
    /// Page currently looks like a WAF challenge (by title or visible widget).
    challenge: bool,
    /// An interactive Cloudflare Turnstile widget is present.
    turnstile: bool,
    /// Milliseconds since the most recent network resource finished loading
    /// (large when nothing is in flight) — our network-idle signal.
    idle_ms: u64,
    /// Length of the serialized DOM; used to detect when rendering has stopped.
    dom_len: u64,
}

/// JS reporting raw page signals. Detection logic lives in Rust
/// ([`is_challenge_title`], [`page_settled`]) so it stays in one place and is
/// unit-testable. `idleMs` comes from the Resource Timing buffer (time since the
/// last resource's `responseEnd`); `domLen` is the serialized DOM length.
const PAGE_STATE_JS: &str = r#"(() => {
  const res = performance.getEntriesByType('resource');
  let last = 0;
  for (let i = 0; i < res.length; i++) { const e = res[i].responseEnd; if (e > last) last = e; }
  const de = document.documentElement;
  return {
    ready: document.readyState === 'complete',
    title: document.title || '',
    turnstile: !!document.querySelector('[name="cf-turnstile-response"], iframe[src*="challenges.cloudflare.com"], div.cf-turnstile'),
    px: !!document.querySelector('#px-captcha, [id^="px-captcha"], .px-captcha, #px-block'),
    idleMs: Math.round(performance.now() - last),
    domLen: de ? de.innerHTML.length : 0
  };
})()"#;

/// Read a non-negative integer field from a JS-returned object (numbers may come
/// back as floats).
fn json_u64(o: &serde_json::Value, key: &str) -> u64 {
    o.get(key)
        .and_then(|x| x.as_f64())
        .map(|f| f.max(0.0) as u64)
        .unwrap_or(0)
}

async fn read_page_state(chaser: &ChaserPage) -> PageState {
    let v = chaser.evaluate(PAGE_STATE_JS).await.ok().flatten();
    match v {
        Some(o) => {
            let title = o.get("title").and_then(|x| x.as_str()).unwrap_or("");
            let turnstile = o
                .get("turnstile")
                .and_then(|x| x.as_bool())
                .unwrap_or(false);
            let px = o.get("px").and_then(|x| x.as_bool()).unwrap_or(false);
            PageState {
                ready: o.get("ready").and_then(|x| x.as_bool()).unwrap_or(false),
                challenge: is_challenge_title(title) || turnstile || px,
                turnstile,
                idle_ms: json_u64(&o, "idleMs"),
                dom_len: json_u64(&o, "domLen"),
            }
        }
        None => PageState {
            ready: false,
            challenge: false,
            turnstile: false,
            idle_ms: 0,
            dom_len: 0,
        },
    }
}

/// Return true if the browser holds a `cf_clearance` cookie (CF challenge solved).
async fn has_clearance_cookie(page: &Page) -> bool {
    page.get_cookies()
        .await
        .map(|cs| cs.iter().any(|c| c.name == "cf_clearance"))
        .unwrap_or(false)
}

// ── settle thresholds ────────────────────────────────────────────────────────
/// No network resource has finished loading for this long ⇒ the network is idle.
const NET_IDLE_MS: u64 = 500;
/// Once the network is idle, the DOM must also be unchanged for this long.
const SETTLE_QUIET: Duration = Duration::from_millis(600);
/// Safety valve: settle when the DOM has been quiet this long even if the network
/// never goes idle (pages with persistent analytics / polling connections).
const MAX_QUIET: Duration = Duration::from_millis(2_500);

/// Decide whether a non-challenge page has rendered enough to capture.
///
/// `network_idle` — no network resource finished within [`NET_IDLE_MS`].
/// `quiet` — how long the serialized DOM has been unchanged.
///
/// Settles when the page is `ready`, not a challenge, and either the network is
/// idle with a brief DOM-quiet confirmation, or the DOM has been quiet long enough
/// on its own — which bounds the wait on pages whose network never goes idle.
pub fn page_settled(ready: bool, challenge: bool, network_idle: bool, quiet: Duration) -> bool {
    if !ready || challenge {
        return false;
    }
    (network_idle && quiet >= SETTLE_QUIET) || quiet >= MAX_QUIET
}

/// Wait until the page has settled on real content or a challenge has been solved,
/// up to `max`. Returns as soon as either:
///   - a `cf_clearance` cookie appears (Cloudflare cleared), or
///   - the page is `readyState=complete`, not a challenge, and has gone
///     network-idle with a stable DOM ([`page_settled`]).
///
/// Waiting for network-idle + DOM stability (rather than snapshotting at
/// `readyState=complete`) lets post-load JS finish — XHR-rendered content, SPA
/// hydration, web-worker fingerprint tests — before we capture the DOM.
///
/// Interactive Cloudflare Turnstile widgets are clicked after a passive window —
/// CF's managed-challenge JS runs its invisible proof-of-work first, and touching
/// the DOM too early raises the bot score.
async fn solve_and_wait(page: &Page, chaser: &ChaserPage, max: Duration) {
    const PASSIVE_WAIT: Duration = Duration::from_millis(6_000);
    const CLICK_INTERVAL: Duration = Duration::from_millis(1_200);
    const POLL: Duration = Duration::from_millis(400);

    let start = Instant::now();
    let mut last_click: Option<Instant> = None;
    let mut prev_dom: Option<u64> = None;
    let mut last_dom_change = start;

    loop {
        if start.elapsed() >= max {
            return;
        }

        if has_clearance_cookie(page).await {
            tokio::time::sleep(Duration::from_millis(500)).await;
            return;
        }

        let st = read_page_state(chaser).await;

        if prev_dom != Some(st.dom_len) {
            prev_dom = Some(st.dom_len);
            last_dom_change = Instant::now();
        }

        if page_settled(
            st.ready,
            st.challenge,
            st.idle_ms >= NET_IDLE_MS,
            last_dom_change.elapsed(),
        ) {
            return;
        }

        if st.turnstile
            && start.elapsed() >= PASSIVE_WAIT
            && last_click.is_none_or(|t| t.elapsed() >= CLICK_INTERVAL)
        {
            try_click_challenge(page).await;
            last_click = Some(Instant::now());
        }

        tokio::time::sleep(POLL).await;
    }
}

/// Drive a fetch entirely through the browser: navigate, solve any challenge, and
/// return the rendered DOM (GET) or an in-page POST response — all from the one
/// stealth session that passed the WAF.
pub async fn fetch(
    browser: &BrowserManager,
    url: &str,
    is_post: bool,
    post_data: Option<&str>,
    proxy_url: Option<&str>,
    extra_cookies: &[RequestCookie],
    max_timeout_ms: u64,
) -> Result<FetchResponse> {
    let proxy = proxy_url
        .filter(|p| !p.is_empty())
        .map(parse_proxy_url)
        .transpose()?;

    // Bound concurrent contexts; incognito context per proxied request, shared
    // default context otherwise (lets cf_clearance carry across requests).
    let _permit = browser
        .acquire_permit()
        .await
        .map_err(FlareSolverError::from)?;
    let ctx_id = browser
        .create_context(proxy.as_ref())
        .await
        .map_err(FlareSolverError::from)?;

    // Blank page first (applies the stealth profile), set proxy auth, then navigate.
    let (page, chaser) = browser
        .new_page(ctx_id, "about:blank")
        .await
        .map_err(FlareSolverError::from)?;

    // Align navigator.languages between the main thread and workers (best-effort,
    // main-thread only). Registered now, before navigation, so it runs on the
    // target document.
    let _ = page.add_init_script(LANGUAGES_CONSISTENCY_JS).await;

    if let Some(p) = &proxy {
        if let (Some(user), Some(pass)) = (&p.username, &p.password) {
            if let Err(e) = page
                .authenticate(Credentials {
                    username: user.clone(),
                    password: pass.clone(),
                })
                .await
            {
                let _ = page.close().await;
                return Err(FlareSolverError::Browser(format!("proxy auth: {e}")));
            }
        }
    }

    // Navigate. The first proxied request can lose a race between the proxy
    // CONNECT and the CDP Fetch auth-interception set up above, surfacing as
    // ERR_TUNNEL_CONNECTION_FAILED / ERR_INVALID_AUTH_CREDENTIALS even when the
    // credentials are correct. The handler is active by the time that error
    // returns, so retry the navigation once for authenticated proxies.
    let proxy_auth = proxy
        .as_ref()
        .is_some_and(|p| p.username.is_some() && p.password.is_some());
    let mut goto_res = chaser.goto(url).await;
    if proxy_auth
        && goto_res
            .as_ref()
            .err()
            .is_some_and(|e| is_proxy_retryable_error(&e.to_string()))
    {
        goto_res = chaser.goto(url).await;
    }
    if let Err(e) = goto_res {
        let _ = page.close().await;
        return Err(FlareSolverError::Browser(e.to_string()));
    }

    // Seed caller-supplied cookies for this origin before settling.
    for c in extra_cookies {
        let pair = format!("{}={}", c.name, c.value);
        let js = format!("document.cookie = {}", serde_json::Value::from(pair));
        let _ = chaser.evaluate(&js).await;
    }

    solve_and_wait(&page, &chaser, Duration::from_millis(max_timeout_ms)).await;

    let user_agent = chaser
        .evaluate("navigator.userAgent")
        .await
        .ok()
        .flatten()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| DEFAULT_UA.to_string());

    let body = if is_post {
        match in_page_post(&chaser, url, post_data.unwrap_or("")).await {
            Ok(b) => b,
            Err(e) => {
                let _ = page.close().await;
                return Err(e);
            }
        }
    } else {
        match page.content().await {
            Ok(b) => b,
            Err(e) => {
                let _ = page.close().await;
                return Err(FlareSolverError::Browser(e.to_string()));
            }
        }
    };

    let final_url = chaser
        .url()
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| url.to_string());

    let cookies = page
        .get_cookies()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|c| chaser_cookie_to_response(oxide_to_chaser_cookie(c)))
        .collect();

    let _ = page.close().await;

    // Selenium/CDP doesn't surface the raw HTTP status or headers for a navigation;
    // like FlareSolverr-py we report 200 + empty headers and put the truth in `body`.
    Ok(FetchResponse {
        url: final_url,
        status: 200,
        headers: HashMap::new(),
        body,
        cookies,
        user_agent,
    })
}

/// POST from the page's own JS context — same cookies, origin, and TLS/fingerprint
/// as the navigation that cleared the WAF.
async fn in_page_post(chaser: &ChaserPage, url: &str, body: &str) -> Result<String> {
    let script = format!(
        "(async () => {{ const r = await fetch({}, {{ method: 'POST', \
         headers: {{ 'Content-Type': 'application/x-www-form-urlencoded' }}, \
         body: {}, credentials: 'include' }}); return await r.text(); }})()",
        serde_json::Value::from(url),
        serde_json::Value::from(body),
    );
    chaser
        .evaluate(&script)
        .await
        .map_err(|e| FlareSolverError::Browser(e.to_string()))?
        .and_then(|v| v.as_str().map(str::to_owned))
        .ok_or_else(|| FlareSolverError::Browser("POST returned no body".into()))
}

/// Click the Turnstile challenge element by traversing its closed shadow root via CDP.
///
/// Cloudflare's Turnstile widget lives inside a CLOSED shadow root invisible to JS;
/// CDP's `DOM.getDocument` with `pierce: true` exposes it. We move the mouse along a
/// human-like Bezier curve before clicking to avoid trivial bot detection. Adapted
/// from chaser-cf's internal solver (not exposed in its public API).
async fn try_click_challenge(page: &Page) {
    let doc = match page
        .execute(GetDocumentParams {
            depth: Some(-1),
            pierce: Some(true),
        })
        .await
    {
        Ok(r) => r,
        Err(_) => return,
    };

    let Some(target_id) = find_shadow_challenge_node(&doc.result.root) else {
        return;
    };

    let box_model = match page
        .execute(GetBoxModelParams {
            node_id: Some(target_id),
            backend_node_id: None,
            object_id: None,
        })
        .await
    {
        Ok(r) => r,
        Err(_) => return,
    };

    let content = box_model.result.model.content.inner();
    if content.len() < 8 {
        return;
    }

    let cx = (content[0] + content[2]) / 2.0;
    let cy = (content[1] + content[5]) / 2.0;

    // Compute all random values in a synchronous block so ThreadRng (which is !Send)
    // is dropped before any await point.
    let (tx, ty, curve_points, post_pause_ms) = {
        use rand::Rng as _;
        let mut rng = rand::rng();

        let tx = cx + rng.random_range(-5.0..=5.0_f64);
        let ty = cy + rng.random_range(-4.0..=4.0_f64);

        let p0x = tx + rng.random_range(-200.0..=-60.0_f64);
        let p0y = ty + rng.random_range(-120.0..=120.0_f64);
        let p1x = p0x + (tx - p0x) * rng.random_range(0.2..0.5_f64) + rng.random_range(-30.0..30.0);
        let p1y = p0y + (ty - p0y) * rng.random_range(0.1..0.4_f64) + rng.random_range(-40.0..40.0);
        let p2x = p0x + (tx - p0x) * rng.random_range(0.5..0.8_f64) + rng.random_range(-20.0..20.0);
        let p2y = p0y + (ty - p0y) * rng.random_range(0.5..0.9_f64) + rng.random_range(-20.0..20.0);

        let steps: u8 = rng.random_range(12..22);
        let mut points: Vec<(f64, f64, u64)> = Vec::with_capacity(steps as usize);
        for i in 1..=steps {
            let t = i as f64 / steps as f64;
            let u = 1.0 - t;
            let bx =
                u * u * u * p0x + 3.0 * u * u * t * p1x + 3.0 * u * t * t * p2x + t * t * t * tx;
            let by =
                u * u * u * p0y + 3.0 * u * u * t * p1y + 3.0 * u * t * t * p2y + t * t * t * ty;
            let speed = (4.0 * t * (1.0 - t)).max(0.1);
            let step_ms = (rng.random_range(8.0..22.0_f64) / speed) as u64;
            points.push((bx, by, step_ms.min(80)));
        }

        let post_pause_ms = rng.random_range(40..120_u64);
        (tx, ty, points, post_pause_ms)
    };

    for (bx, by, step_ms) in curve_points {
        let _ = page.move_mouse(Point::new(bx, by)).await;
        tokio::time::sleep(Duration::from_millis(step_ms)).await;
    }

    tokio::time::sleep(Duration::from_millis(post_pause_ms)).await;
    let _ = page.click(Point::new(tx, ty)).await;
}

/// Walk the CDP DOM tree and return the NodeId to click for a Turnstile challenge.
fn find_shadow_challenge_node(node: &Node) -> Option<NodeId> {
    let children = node.children.as_deref().unwrap_or(&[]);

    let is_shadow_host = children.iter().any(|child| {
        child
            .attributes
            .as_deref()
            .unwrap_or(&[])
            .chunks(2)
            .any(|p| p.len() == 2 && p[0] == "name" && p[1] == "cf-turnstile-response")
    });

    if is_shadow_host {
        if let Some(sr) = node.shadow_roots.as_ref().and_then(|srs| srs.first()) {
            for sr_child in sr.children.as_deref().unwrap_or(&[]) {
                let hidden = sr_child
                    .attributes
                    .as_deref()
                    .unwrap_or(&[])
                    .chunks(2)
                    .any(|p| p.len() == 2 && p[0] == "style" && p[1].contains("display: none"));
                if !hidden {
                    return Some(sr_child.node_id);
                }
            }
        }
    }

    for child in children {
        if let Some(id) = find_shadow_challenge_node(child) {
            return Some(id);
        }
    }
    for sr in node.shadow_roots.as_deref().unwrap_or(&[]) {
        for sr_child in sr.children.as_deref().unwrap_or(&[]) {
            if let Some(id) = find_shadow_challenge_node(sr_child) {
                return Some(id);
            }
        }
    }
    None
}

/// Dispatch a FlareSolverr protocol command. Returns (status, message, Option<FetchResponse>).
#[cfg(feature = "server")]
#[allow(clippy::too_many_arguments)]
pub async fn dispatch(
    store: &Arc<SessionStore>,
    cmd: &str,
    url: Option<&str>,
    is_post: bool,
    post_data: Option<&str>,
    proxy_url: Option<&str>,
    session_id: Option<&str>,
    extra_cookies: &[RequestCookie],
    max_timeout_ms: u64,
) -> Result<(String, String, Option<FetchResponse>)> {
    match cmd {
        "request.get" | "request.post" => {
            let url = url.ok_or_else(|| FlareSolverError::MissingField("url".into()))?;
            let session_proxy = session_id.and_then(|id| store.registry.get_proxy(id));
            let effective_proxy = session_proxy.as_deref().or(proxy_url);
            let browser = store.browser().await?;
            let resp = fetch(
                browser,
                url,
                is_post,
                post_data,
                effective_proxy,
                extra_cookies,
                max_timeout_ms,
            )
            .await?;
            Ok(("ok".into(), String::new(), Some(resp)))
        }
        "sessions.create" => {
            let id = store
                .registry
                .create(session_id.map(|s| s.to_string()), proxy_url)?;
            Ok(("ok".into(), format!("Session created: {id}"), None))
        }
        "sessions.list" => {
            let ids = store.registry.list();
            Ok((
                "ok".into(),
                serde_json::to_string(&ids).unwrap_or_default(),
                None,
            ))
        }
        "sessions.destroy" => {
            let id = session_id.ok_or_else(|| FlareSolverError::MissingField("session".into()))?;
            store.registry.destroy(id)?;
            Ok(("ok".into(), format!("Session destroyed: {id}"), None))
        }
        other => Err(FlareSolverError::UnsupportedCommand(other.to_string())),
    }
}
