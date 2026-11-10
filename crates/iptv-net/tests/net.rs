//! Transport behaviour against a local server.

mod support;

use std::io::Write as _;
use std::time::Duration;

use iptv_import::{BodyStream, FetchErrorKind};
use iptv_net::{NetClient, NetConfig};
use iptv_providers::HttpRequest;
use support::{Resp, TestServer, self_signed};

fn client() -> NetClient {
    NetClient::with_defaults().unwrap()
}

#[tokio::test]
async fn buffered_fetch_sends_headers_as_given() {
    let server = TestServer::http(|_| Resp::ok("hello")).await;
    let net = client();
    let resp = net
        .fetch(
            HttpRequest::get(server.url("/a?x=1"))
                .with_header("User-Agent", "SyntheticPlayer/1.0")
                .with_header("Cookie", "mac=00:1A:79:00:00:01; stb_lang=en")
                .with_header("Referer", "http://ref.example.com/"),
        )
        .await
        .unwrap();
    assert_eq!((resp.status, resp.body.as_slice()), (200, &b"hello"[..]));
    let seen = &server.requests()[0];
    assert_eq!(seen.path, "/a");
    assert_eq!(seen.param("x"), Some("1"));
    assert_eq!(seen.header("user-agent"), Some("SyntheticPlayer/1.0"));
    assert_eq!(
        seen.header("cookie"),
        Some("mac=00:1A:79:00:00:01; stb_lang=en")
    );
    assert_eq!(seen.header("referer"), Some("http://ref.example.com/"));

    // Without a User-Agent the configured default is sent.
    net.fetch(HttpRequest::get(server.url("/b"))).await.unwrap();
    assert_eq!(
        server.requests()[1].header("user-agent"),
        Some(iptv_providers::xtream::DEFAULT_USER_AGENT)
    );
    // Non-success statuses are responses, not errors.
    let server = TestServer::http(|_| Resp::status(404)).await;
    assert_eq!(
        net.fetch(HttpRequest::get(server.url("/")))
            .await
            .unwrap()
            .status,
        404
    );
}

#[tokio::test]
async fn content_encoding_is_decoded_but_gz_files_pass_through() {
    let gz = support::synth::gzip(b"#EXTM3U\n");
    let mut deflate = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    deflate.write_all(b"deflated body").unwrap();
    let deflate = deflate.finish().unwrap();
    let (g, d) = (gz.clone(), deflate.clone());
    let server = TestServer::http(move |req| match req.path.as_str() {
        "/gzip" => Resp::ok(g.clone()).header("Content-Encoding", "gzip"),
        "/deflate" => Resp::ok(d.clone()).header("Content-Encoding", "deflate"),
        _ => Resp::ok(g.clone()).header("Content-Type", "application/gzip"),
    })
    .await;
    let net = client();
    let body = |p: &'static str| {
        let net = net.clone();
        let url = server.url(p);
        async move { net.fetch(HttpRequest::get(url)).await.unwrap().body }
    };
    assert_eq!(body("/gzip").await, b"#EXTM3U\n");
    assert_eq!(body("/deflate").await, b"deflated body");
    assert_eq!(body("/guide.xml.gz").await, gz, "file bytes untouched");
    let accept = server.requests()[0]
        .header("accept-encoding")
        .unwrap()
        .to_owned();
    assert!(
        accept.contains("gzip") && accept.contains("deflate"),
        "{accept}"
    );
}

#[tokio::test]
async fn redirects_relative_cross_origin_and_cross_protocol() {
    let target = TestServer::http(|req| Resp::ok(format!("landed {}", req.path))).await;
    let to = target.url("/final");
    let origin = TestServer::http(move |req| match req.path.as_str() {
        "/rel" => Resp::redirect(302, "/abs"),
        "/abs" => Resp::redirect(301, &to),
        "/loop" => Resp::redirect(302, "/loop"),
        _ => Resp::status(404),
    })
    .await;
    let net = client();
    let resp = net
        .fetch(HttpRequest::get(origin.url("/rel")).with_header("Cookie", "mac=00:1A:79:00:00:02"))
        .await
        .unwrap();
    assert_eq!(resp.body, b"landed /final");
    // Identity cookies stay with the original host.
    assert_eq!(target.requests()[0].header("cookie"), None);
    assert!(origin.requests()[1].header("cookie").is_some());

    let err = net
        .fetch(HttpRequest::get(origin.url("/loop")))
        .await
        .unwrap_err();
    assert_eq!(err.kind, FetchErrorKind::Redirect, "{err}");

    // https → http.
    let (acceptor, der) = self_signed();
    let to = target.url("/plain");
    let secure = TestServer::https(move |_| Resp::redirect(302, &to), acceptor).await;
    let trusting = NetClient::new(NetConfig {
        extra_roots_der: vec![der],
        ..NetConfig::default()
    })
    .unwrap();
    let resp = trusting
        .fetch(HttpRequest::get(secure.url("/start")))
        .await
        .unwrap();
    assert_eq!(resp.body, b"landed /plain");
}

#[tokio::test]
async fn tls_uses_webpki_roots_plus_extra_roots() {
    let (acceptor, der) = self_signed();
    let server = TestServer::https(|_| Resp::ok("secure"), acceptor).await;
    let err = client()
        .fetch(HttpRequest::get(server.url("/")))
        .await
        .unwrap_err();
    assert_eq!(err.kind, FetchErrorKind::Tls, "{err}");
    let trusting = NetClient::new(NetConfig {
        extra_roots_der: vec![der],
        ..NetConfig::default()
    })
    .unwrap();
    let resp = trusting
        .fetch(HttpRequest::get(server.url("/")))
        .await
        .unwrap();
    assert_eq!(resp.body, b"secure");
    assert!(
        NetClient::new(NetConfig {
            extra_roots_der: vec![b"not der".to_vec()],
            ..NetConfig::default()
        })
        .is_err()
    );
}

#[tokio::test]
async fn streaming_delivers_chunks_as_they_arrive() {
    let pieces: Vec<Vec<u8>> = (0..5).map(|i| vec![b'a' + i; 1000]).collect();
    let p2 = pieces.clone();
    let server =
        TestServer::http(move |_| Resp::ok("").chunked(p2.clone(), Duration::from_millis(30)))
            .await;
    let resp = client()
        .fetch_stream(HttpRequest::get(server.url("/big")))
        .await
        .unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.content_length, None);
    let mut body = resp.body;
    let mut got = Vec::new();
    let mut chunks = 0;
    while let Some(c) = body.next_chunk().await.unwrap() {
        assert!(!c.is_empty());
        chunks += 1;
        got.extend(c);
    }
    assert_eq!(got, pieces.concat());
    assert!(
        chunks >= 5,
        "chunks are not coalesced into one buffer: {chunks}"
    );
    assert_eq!(body.next_chunk().await.unwrap(), None, "stays finished");
}

#[tokio::test]
async fn timeouts_and_connection_failures() {
    let net = NetClient::new(NetConfig {
        read_timeout: Duration::from_millis(200),
        ..NetConfig::default()
    })
    .unwrap();
    let server = TestServer::http(|req| match req.path.as_str() {
        "/slow-head" => Resp::ok("late").delayed(Duration::from_secs(3)),
        _ => Resp::ok("").chunked(vec![b"x".to_vec(), b"y".to_vec()], Duration::from_secs(3)),
    })
    .await;
    let err = net
        .fetch(HttpRequest::get(server.url("/slow-head")))
        .await
        .unwrap_err();
    assert_eq!(err.kind, FetchErrorKind::Timeout, "{err}");
    let resp = net
        .fetch_stream(HttpRequest::get(server.url("/stall")))
        .await
        .unwrap();
    let mut body = resp.body;
    assert_eq!(body.next_chunk().await.unwrap().as_deref(), Some(&b"x"[..]));
    let err = body.next_chunk().await.unwrap_err();
    assert_eq!(err.kind, FetchErrorKind::Timeout, "{err}");

    // A closed port; credentials in the URL never reach the error.
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let url = format!(
        "http://127.0.0.1:{port}/player_api.php?username=demo-user&password=synthetic-pass"
    );
    let err = net.fetch(HttpRequest::get(url)).await.unwrap_err();
    assert_eq!(err.kind, FetchErrorKind::Connect, "{err}");
    let shown = format!("{err} {err:?}");
    assert!(
        !shown.contains("synthetic-pass") && !shown.contains("demo-user"),
        "{shown}"
    );
    assert!(!shown.contains("127.0.0.1"), "no URL in errors: {shown}");
}

#[tokio::test]
async fn buffered_bodies_are_bounded() {
    let server = TestServer::http(|req| match req.path.as_str() {
        "/sized" => Resp::ok(vec![b'z'; 4096]),
        _ => Resp::ok("").chunked(vec![vec![b'z'; 3000]; 2], Duration::from_millis(1)),
    })
    .await;
    let net = NetClient::new(NetConfig {
        max_buffered_bytes: 4000,
        ..NetConfig::default()
    })
    .unwrap();
    for p in ["/sized", "/chunked"] {
        let err = net
            .fetch(HttpRequest::get(server.url(p)))
            .await
            .unwrap_err();
        assert_eq!(err.kind, FetchErrorKind::TooLarge, "{p}");
    }
}

#[tokio::test]
async fn local_files_and_unsupported_requests() {
    let dir = std::env::temp_dir().join(format!("iptv-net-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("list.m3u");
    std::fs::write(&path, b"#EXTM3U\n#EXTINF:-1,Local\nhttp://127.0.0.1/1.ts\n").unwrap();
    let net = client();
    let by_url = net
        .fetch(HttpRequest::get(format!("file://{}", path.display())))
        .await
        .unwrap();
    let by_path = net
        .fetch(HttpRequest::get(path.display().to_string()))
        .await
        .unwrap();
    assert_eq!(by_url.body, by_path.body);
    assert!(by_url.body.starts_with(b"#EXTM3U"));
    let err = net
        .fetch(HttpRequest::get(
            dir.join("missing.m3u").display().to_string(),
        ))
        .await
        .unwrap_err();
    assert_eq!(err.kind, FetchErrorKind::Io);
    assert!(
        !err.to_string().contains("missing.m3u"),
        "no paths in errors"
    );
    std::fs::remove_dir_all(&dir).unwrap();

    let no_files = NetClient::new(NetConfig {
        allow_files: false,
        ..NetConfig::default()
    })
    .unwrap();
    let err = no_files
        .fetch(HttpRequest::get("file:///etc/hostname"))
        .await
        .unwrap_err();
    assert_eq!(err.kind, FetchErrorKind::Unsupported);
    let err = net
        .fetch(HttpRequest::get("ftp://files.example.com/x.m3u"))
        .await
        .unwrap_err();
    assert_eq!(err.kind, FetchErrorKind::Unsupported);
    let err = net.fetch(HttpRequest::get("not a url")).await.unwrap_err();
    assert_eq!(err.kind, FetchErrorKind::InvalidRequest);
    let err = net
        .fetch(HttpRequest::get("http://127.0.0.1/").with_header("X-Bad", "line\nbreak"))
        .await
        .unwrap_err();
    assert_eq!(err.kind, FetchErrorKind::InvalidRequest);
    assert!(
        !err.to_string().contains("break"),
        "header values are not quoted"
    );
}

#[test]
fn debug_output_is_redacted() {
    let net = NetClient::new(NetConfig {
        proxy: Some("http://proxyuser:proxy-secret@127.0.0.1:3128".into()),
        ..NetConfig::default()
    })
    .unwrap();
    let shown = format!("{net:?}");
    assert!(!shown.contains("proxy-secret"), "{shown}");
}
