use flaresolverr_rs::models::{FetchRequest, RequestCookie};

#[test]
fn test_fetch_request_get_defaults() {
    let req = FetchRequest::get("https://example.com");
    assert_eq!(req.url, "https://example.com");
    assert!(!req.is_post);
    assert!(req.post_data.is_none());
    assert!(req.proxy.is_none());
    assert!(req.cookies.is_empty());
}

#[test]
fn test_fetch_request_post_with_body() {
    let req = FetchRequest::post("https://example.com").body("a=1&b=2");
    assert!(req.is_post);
    assert_eq!(req.post_data.as_deref(), Some("a=1&b=2"));
}

#[test]
fn test_fetch_request_builder_chain() {
    let req = FetchRequest::get("https://example.com")
        .proxy("http://proxy:8080")
        .cookie("name", "value")
        .cookie("foo", "bar");
    assert_eq!(req.proxy.as_deref(), Some("http://proxy:8080"));
    assert_eq!(req.cookies.len(), 2);
    assert_eq!(req.cookies[0].name, "name");
    assert_eq!(req.cookies[1].name, "foo");
}

#[test]
fn test_request_cookie_is_clone() {
    let c = RequestCookie { name: "x".into(), value: "y".into() };
    let c2 = c.clone();
    assert_eq!(c2.name, "x");
}
