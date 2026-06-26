use chaser_cf::Cookie;
use flaresolverr_rs::solver::{chaser_cookie_to_response, is_challenge_title, parse_proxy_url};

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
