use super::*;

#[tokio::test(start_paused = true)]
async fn idle_deadline_of_a_limited_body_starts_at_the_first_poll() {
    let stalled =
        http_body_util::StreamBody::new(tokio_stream::pending::<Result<Frame<Bytes>, BoxError>>())
            .boxed_unsync();
    let mut body = LimitedBody::new(stalled, 1024, Duration::from_secs(10));
    let mut cx = Context::from_waker(std::task::Waker::noop());

    tokio::time::advance(Duration::from_secs(60)).await;
    let first = hyper::body::Body::poll_frame(Pin::new(&mut body), &mut cx);
    tokio::time::advance(Duration::from_secs(11)).await;
    let second = hyper::body::Body::poll_frame(Pin::new(&mut body), &mut cx);

    assert!(first.is_pending());
    assert!(matches!(second, Poll::Ready(Some(Err(_)))));
    assert_eq!(body.limit().get(), Some(&BodyLimit::TimedOut));
}

#[test]
fn upstream_url_accepts_plain_http_only_when_allowed() {
    let url = UpstreamUrl::parse("http://forge.example/acme/app.git", true).unwrap();
    assert!(!url.is_https());
    assert_eq!(url.port(), 80);
    assert!(UpstreamUrl::parse("http://forge.example/acme/app.git", false).is_none());
    let url = UpstreamUrl::parse("https://forge.example/acme/app.git", true).unwrap();
    assert!(url.is_https());
    assert_eq!(url.port(), 443);
    assert!(UpstreamUrl::parse("ssh://forge.example/acme/app.git", true).is_none());
}

#[test]
fn upstream_url_accepts_only_https_without_userinfo_query_or_fragment() {
    assert!(UpstreamUrl::parse("https://forge.example/acme/app.git", false).is_some());
    for url in [
        "http://forge.example/acme/app.git",
        "https://bot:pw@forge.example/acme/app.git",
        "https://forge.example/acme/app.git?x=1",
        "https://forge.example/acme/app.git#frag",
        "/acme/app.git",
    ] {
        assert!(UpstreamUrl::parse(url, false).is_none(), "{url}");
    }
}
