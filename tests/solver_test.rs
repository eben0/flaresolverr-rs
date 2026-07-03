use std::time::Duration;

use chaser_cf::Cookie;
use flaresolverr_rs::models::ProxyConfig;
use flaresolverr_rs::solver::{
    body_is_challenge, chaser_cookie_to_response, env_proxy_from, is_challenge_title,
    is_proxy_retryable_error, is_transient_fetch_error, page_settled, parse_proxy_url, pick_proxy,
};

#[test]
fn test_parse_proxy_no_auth() {
    let p = parse_proxy_url("http://proxy.example.com:8080").unwrap();
    assert_eq!(p.host, "proxy.example.com");
    assert_eq!(p.port, 8080);
    assert!(p.username.is_none());
}

#[test]
fn test_parse_proxy_with_auth() {
    let p = parse_proxy_url("http://user:pass@proxy.example.com:1081").unwrap();
    assert_eq!(p.host, "proxy.example.com");
    assert_eq!(p.port, 1081);
    assert_eq!(p.username.as_deref(), Some("user"));
    assert_eq!(p.password.as_deref(), Some("pass"));
}

#[test]
fn test_parse_proxy_socks5() {
    let p = parse_proxy_url("socks5://host:1080").unwrap();
    assert_eq!(p.scheme.as_deref(), Some("socks5"));
}

#[test]
fn test_parse_proxy_invalid_no_scheme() {
    assert!(parse_proxy_url("proxy.example.com:8080").is_err());
}

#[test]
fn test_chaser_cookie_to_response_session_cookie() {
    let cookie = Cookie {
        name: "cf_clearance".into(),
        value: "abc123".into(),
        domain: Some(".example.com".into()),
        path: Some("/".into()),
        expires: Some(9999999.0),
        http_only: Some(false),
        secure: Some(true),
        same_site: None,
    };
    let rc = chaser_cookie_to_response(cookie);
    assert_eq!(rc.name, "cf_clearance");
    assert_eq!(rc.value, "abc123");
    assert_eq!(rc.domain, ".example.com");
    assert!(rc.secure);
    assert!(!rc.http_only);
    assert!(!rc.session); // has expiry → not a session cookie
}

#[test]
fn test_chaser_cookie_no_expiry_is_session() {
    let cookie = Cookie {
        name: "tmp".into(),
        value: "x".into(),
        domain: None,
        path: None,
        expires: None,
        http_only: None,
        secure: None,
        same_site: None,
    };
    let rc = chaser_cookie_to_response(cookie);
    assert!(rc.session);
}

#[test]
fn test_is_challenge_title_detects_walls() {
    assert!(is_challenge_title("Just a moment...")); // Cloudflare
    assert!(is_challenge_title("Bloomberg - Are you a robot?")); // PerimeterX
    assert!(is_challenge_title("Attention Required! | Cloudflare"));
    assert!(is_challenge_title("Access denied"));
    assert!(is_challenge_title("CHECKING YOUR BROWSER")); // case-insensitive
}

#[test]
fn test_is_challenge_title_allows_real_titles() {
    assert!(!is_challenge_title(
        "Gold Steadies Near $4,000 as Inflation Data Eases Rate-Hike Bets - Bloomberg"
    ));
    assert!(!is_challenge_title("Example Domain"));
    assert!(!is_challenge_title(""));
}

#[test]
fn test_page_settled_blocks_until_ready_and_not_challenge() {
    // not ready → never settled, however idle/quiet
    assert!(!page_settled(false, false, true, Duration::from_secs(10)));
    // a challenge page is never "settled" even when idle and quiet
    assert!(!page_settled(true, true, true, Duration::from_secs(10)));
}

#[test]
fn test_page_settled_on_network_idle_plus_dom_quiet() {
    // ready, not a challenge, network idle, DOM quiet ≥ 600ms → settled
    assert!(page_settled(true, false, true, Duration::from_millis(600)));
    // idle but DOM not yet quiet long enough → keep waiting
    assert!(!page_settled(true, false, true, Duration::from_millis(599)));
}

#[test]
fn test_body_is_challenge_detects_interstitial_by_title() {
    // The interstitial title trips it → retry on a fresh context.
    assert!(body_is_challenge(
        "<html><head><TITLE>Just a moment...</TITLE></head><body>x</body></html>"
    ));
    // The real page title does not.
    assert!(!body_is_challenge(
        "<html><head><title>Download Movies Torrents | 1337x</title></head><body>x</body></html>"
    ));
    // No title, or empty body → not a challenge (don't retry a genuine empty result).
    assert!(!body_is_challenge("<html><body>no title element here</body></html>"));
    assert!(!body_is_challenge(""));
}

#[test]
fn test_is_transient_fetch_error_matches_flaky_proxy_failures() {
    // Flaky-proxy signatures observed in practice → retry on a fresh context.
    assert!(is_transient_fetch_error("browser error: net::ERR_EMPTY_RESPONSE"));
    assert!(is_transient_fetch_error("browser error: Request timed out."));
    assert!(is_transient_fetch_error("net::ERR_CONNECTION_RESET"));
    assert!(is_transient_fetch_error("net::ERR_TUNNEL_CONNECTION_FAILED"));
    // A stable/permanent failure must NOT be retried (don't burn the timeout budget).
    assert!(!is_transient_fetch_error("net::ERR_NAME_NOT_RESOLVED"));
    assert!(!is_transient_fetch_error("missing required field: url"));
}

#[test]
fn test_is_proxy_retryable_error_matches_proxy_auth_failures() {
    assert!(is_proxy_retryable_error(
        "browser error: net::ERR_TUNNEL_CONNECTION_FAILED"
    ));
    assert!(is_proxy_retryable_error("net::ERR_INVALID_AUTH_CREDENTIALS"));
    assert!(is_proxy_retryable_error("net::ERR_PROXY_CONNECTION_FAILED"));
    // unrelated failures must NOT trigger a retry
    assert!(!is_proxy_retryable_error("net::ERR_NAME_NOT_RESOLVED"));
    assert!(!is_proxy_retryable_error("browser error: Request timed out."));
}

fn proxy(url: &str, user: Option<&str>, pass: Option<&str>) -> ProxyConfig {
    ProxyConfig {
        url: Some(url.into()),
        username: user.map(str::to_string),
        password: pass.map(str::to_string),
    }
}

#[test]
fn test_effective_url_folds_separate_credentials() {
    // Prowlarr/FlareSolverr send creds as separate fields → must be embedded.
    let p = proxy("http://host:1081", Some("user"), Some("pass"));
    assert_eq!(p.effective_url().as_deref(), Some("http://user:pass@host:1081"));
}

#[test]
fn test_effective_url_keeps_already_embedded_credentials() {
    let p = proxy("http://a:b@host:1081", Some("user"), Some("pass"));
    assert_eq!(p.effective_url().as_deref(), Some("http://a:b@host:1081"));
}

#[test]
fn test_effective_url_without_auth_is_unchanged() {
    let p = proxy("socks5://host:1080", None, None);
    assert_eq!(p.effective_url().as_deref(), Some("socks5://host:1080"));
}

#[test]
fn test_effective_url_defaults_scheme_when_missing() {
    let p = proxy("host:1081", Some("user"), Some("pass"));
    assert_eq!(p.effective_url().as_deref(), Some("http://user:pass@host:1081"));
}

#[test]
fn test_page_settled_safety_valve_when_network_never_idle() {
    // network stays busy: brief DOM-quiet is not enough …
    assert!(!page_settled(true, false, false, Duration::from_millis(600)));
    assert!(!page_settled(true, false, false, Duration::from_millis(2_499)));
    // … but a long DOM-quiet bounds the wait regardless of the network
    assert!(page_settled(true, false, false, Duration::from_millis(2_500)));
}

#[test]
fn test_pick_proxy_returns_first_nonempty_trimmed() {
    // Empty and whitespace-only candidates are skipped; the winner is trimmed.
    assert_eq!(pick_proxy(["", "  ", "http://a:1"]).as_deref(), Some("http://a:1"));
    assert_eq!(pick_proxy(["  http://b:2  ", "http://c:3"]).as_deref(), Some("http://b:2"));
}

#[test]
fn test_pick_proxy_none_when_all_blank() {
    assert_eq!(pick_proxy(Vec::<String>::new()), None);
    assert_eq!(pick_proxy(["", "   ", "\t"]), None);
}

#[test]
fn test_env_proxy_prefers_https_over_http_and_all() {
    // HTTPS_PROXY wins the priority order (we fetch https URLs).
    let lookup = |k: &str| match k {
        "HTTPS_PROXY" => Some("http://secure:1".to_string()),
        "HTTP_PROXY" => Some("http://plain:2".to_string()),
        "ALL_PROXY" => Some("http://all:3".to_string()),
        _ => None,
    };
    assert_eq!(env_proxy_from(lookup).as_deref(), Some("http://secure:1"));
}

#[test]
fn test_env_proxy_falls_through_empty_var_to_next() {
    // A set-but-empty HTTPS_PROXY (common: `HTTP_PROXY=""`) is skipped for the
    // next non-empty variable rather than treated as "no proxy but present".
    let lookup = |k: &str| match k {
        "HTTPS_PROXY" => Some("   ".to_string()),
        "HTTP_PROXY" => Some("http://plain:2".to_string()),
        _ => None,
    };
    assert_eq!(env_proxy_from(lookup).as_deref(), Some("http://plain:2"));
}

#[test]
fn test_env_proxy_none_when_unset() {
    assert_eq!(env_proxy_from(|_| None), None);
}
