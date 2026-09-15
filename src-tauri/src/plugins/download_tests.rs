use super::*;
use crate::plugin_config::DesktopPluginArtifact;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn declaration() -> DesktopPluginConfig {
    DesktopPluginConfig {
        plugin_id: "example.monitor".into(),
        version: "1.2.3".into(),
        enabled: true,
        artifacts: BTreeMap::from([(
            DESKTOP_TARGETS[0].to_string(),
            DesktopPluginArtifact {
                url: "https://packages.example.invalid/monitor-1.2.3.idplugin".into(),
                sha256: "a".repeat(64),
                extra: BTreeMap::new(),
            },
        )]),
        extra: BTreeMap::new(),
    }
}

fn artifact(declaration: &mut DesktopPluginConfig) -> &mut DesktopPluginArtifact {
    declaration.artifacts.values_mut().next().unwrap()
}

#[test]
fn declarations_accept_empty_disabled_and_complete_desktop_target_sets() {
    assert_eq!(validate_declarations(&[]), Ok(()));
    let mut config = declaration();
    config.enabled = false;
    config.version = "1.2.3-rc.1+build.42".into();
    let package = artifact(&mut config).clone();
    config.artifacts = DESKTOP_TARGETS
        .iter()
        .map(|target| (target.to_string(), package.clone()))
        .collect();
    assert_eq!(validate_declarations(&[config]), Ok(()));
}

#[test]
fn declaration_limit_and_identity_include_disabled_entries() {
    let mut declarations: Vec<_> = (0..MAX_DECLARATIONS)
        .map(|index| {
            let mut config = declaration();
            config.plugin_id = format!("example.plugin{index}");
            config.enabled = false;
            config
        })
        .collect();
    assert!(validate_declarations(&declarations).is_ok());
    declarations.push(declaration());
    assert!(validate_declarations(&declarations).is_err());
    declarations.pop();
    declarations[1].plugin_id = declarations[0].plugin_id.clone();
    assert!(validate_declarations(&declarations).is_err());
    for plugin_id in ["monitor", "../monitor", "Example.monitor", "example.-bad"] {
        let mut config = declaration();
        config.plugin_id = plugin_id.into();
        assert!(validate_declarations(&[config]).is_err(), "{plugin_id}");
    }
}

#[test]
fn versions_are_bounded_exact_pins_and_artifacts_are_known_nonempty_targets() {
    for version in ["latest", "^1.2.3", "1.2", "v1.2.3", "01.2.3", " 1.2.3"] {
        let mut config = declaration();
        config.version = version.into();
        assert!(validate_declarations(&[config]).is_err(), "{version}");
    }
    let mut config = declaration();
    config.version = format!("1.2.3+{}", "a".repeat(128));
    assert!(validate_declarations(&[config]).is_err());
    let mut config = declaration();
    config.artifacts.clear();
    assert!(validate_declarations(&[config]).is_err());
    for target in ["aarch64-linux-android", "aarch64-apple-ios", "latest", ""] {
        let mut config = declaration();
        let package = artifact(&mut config).clone();
        config.artifacts = BTreeMap::from([(target.to_string(), package)]);
        assert!(validate_declarations(&[config]).is_err(), "{target}");
    }
}

#[test]
fn unknown_declaration_and_artifact_fields_fail_closed_on_desktop() {
    let mut config = declaration();
    config
        .extra
        .insert("install_command".into(), "ignored".into());
    assert!(validate_declarations(&[config]).is_err());
    let mut config = declaration();
    artifact(&mut config)
        .extra
        .insert("skip_verification".into(), true.into());
    assert!(validate_declarations(&[config]).is_err());
}

#[test]
fn config_sources_reject_secrets_ambiguous_urls_and_unpinned_digests() {
    for value in [
        "http://packages.example.invalid/plugin.idplugin",
        "file:///plugin.idplugin",
        "https:packages.example.invalid/plugin.idplugin",
        "//packages.example.invalid/plugin.idplugin",
        "https://user:secret@packages.example.invalid/plugin.idplugin",
        "https://@packages.example.invalid/plugin.idplugin",
        "https://packages.example.invalid/plugin.idplugin?token=secret",
        "https://packages.example.invalid/plugin.idplugin?",
        "https://packages.example.invalid/plugin.idplugin#secret",
        "https://packages.example.invalid/plugin.idplugin#",
        " https://packages.example.invalid/plugin.idplugin",
        "https://packages.example.invalid/\nplugin.idplugin",
        "https://packages.example.invalid\\plugin.idplugin",
    ] {
        let mut config = declaration();
        artifact(&mut config).url = value.into();
        let error = validate_declarations(&[config]).unwrap_err();
        assert!(!error.contains("secret"));
        assert!(!error.contains("packages.example.invalid"));
    }
    for digest in [
        "a".repeat(63),
        "a".repeat(65),
        "A".repeat(64),
        "g".repeat(64),
    ] {
        let mut config = declaration();
        artifact(&mut config).sha256 = digest;
        assert!(validate_declarations(&[config]).is_err());
    }
    let mut config = declaration();
    let prefix = "https://packages.example.invalid/";
    artifact(&mut config).url = format!("{prefix}{}", "a".repeat(2048 - prefix.len()));
    artifact(&mut config).sha256 = "0123456789abcdef".repeat(4);
    assert!(validate_declarations(&[config.clone()]).is_ok());
    artifact(&mut config).url.push('a');
    assert!(validate_declarations(&[config]).is_err());
}

#[test]
fn redirect_policy_allows_signed_https_and_rejects_every_downgrade() {
    let policy = DownloadPolicy::default();
    let base =
        Url::parse("https://github.com/example/releases/download/v1/plugin.idplugin").unwrap();
    let signed =
        "https://release-assets.githubusercontent.com/asset?token=private&signature=secret";
    assert_eq!(
        redirect_url(&base, signed, &policy).unwrap().as_str(),
        signed
    );
    assert_eq!(
        redirect_url(&base, "../v2/plugin.idplugin?token=private", &policy)
            .unwrap()
            .as_str(),
        "https://github.com/example/releases/download/v2/plugin.idplugin?token=private"
    );
    for location in [
        "http://github.com/secret",
        "http://127.0.0.1/secret",
        "ftp://github.com/secret",
        "https://user:secret@github.com/asset",
        "//user:secret@github.com/asset",
        "https://@github.com/asset",
        "#secret",
        "/asset#secret",
        "/asset\nsecret",
        "https:\\github.com\\secret",
        "",
    ] {
        let error = redirect_url(&base, location, &policy).unwrap_err();
        assert!(!error.contains("secret"));
        assert!(!error.contains("github.com"));
    }
    assert!(redirect_url(&base, &format!("/{}", "a".repeat(8192)), &policy).is_err());
}

struct Reply {
    header: String,
    body: Vec<u8>,
    before_headers: Duration,
    before_body: Duration,
}

impl Reply {
    fn raw(header: &str, body: &[u8]) -> Self {
        Self {
            header: header.into(),
            body: body.into(),
            before_headers: Duration::ZERO,
            before_body: Duration::ZERO,
        }
    }

    fn body(body: &[u8]) -> Self {
        Self::raw(
            &format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n", body.len()),
            body,
        )
    }

    fn redirect(location: &str) -> Self {
        Self::raw(
            &format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\n"),
            &[],
        )
    }
}

struct Fixture {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Fixture {
    async fn new(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/plugin.idplugin", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let task = tokio::spawn(async move {
            for reply in replies {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let byte = stream.read_u8().await.unwrap();
                    request.push(byte);
                    assert!(request.len() <= 16 * 1024);
                }
                recorded
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(request).unwrap());
                tokio::time::sleep(reply.before_headers).await;
                let header = format!("{}Connection: close\r\n\r\n", reply.header);
                if stream.write_all(header.as_bytes()).await.is_err() {
                    return;
                }
                tokio::time::sleep(reply.before_body).await;
                if stream.write_all(&reply.body).await.is_err() {
                    return;
                }
                let _ = stream.shutdown().await;
            }
        });
        Self {
            url,
            requests,
            task,
        }
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn fixture_policy() -> DownloadPolicy {
    DownloadPolicy {
        max_bytes: 8,
        max_redirects: 2,
        total_timeout: Duration::from_secs(2),
        allow_loopback_http: true,
        ..DownloadPolicy::default()
    }
}

#[tokio::test]
async fn production_entry_rejects_http_before_connecting_and_fixture_policy_stays_local() {
    let server = Fixture::new(vec![Reply::body(b"archive")]).await;
    assert!(download_archive(&server.url).await.is_err());
    assert!(server.requests().is_empty());
    assert!(source_url("http://example.invalid/plugin.idplugin", &fixture_policy()).is_err());
}

#[tokio::test]
async fn download_preserves_archive_bytes_at_limit_without_decompression_or_credentials() {
    let server = Fixture::new(vec![Reply::raw(
        "HTTP/1.1 200 OK\r\nContent-Length: 8\r\nContent-Encoding: gzip\r\n",
        b"archive!",
    )])
    .await;
    assert_eq!(
        download_with_policy(&server.url, &fixture_policy())
            .await
            .unwrap(),
        b"archive!"
    );
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    let request = requests[0].to_ascii_lowercase();
    for header in [
        "authorization:",
        "proxy-authorization:",
        "cookie:",
        "referer:",
    ] {
        assert!(!request.contains(header), "{header}");
    }
}

#[tokio::test]
async fn download_bounds_both_announced_and_streamed_bodies_and_rejects_empty_archives() {
    let cases = [
        (
            Reply::raw("HTTP/1.1 200 OK\r\nContent-Length: 9\r\n", b"123456789"),
            "desktop plugin archive exceeds size limit",
        ),
        (
            Reply::raw(
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n",
                b"4\r\n1234\r\n5\r\n56789\r\n0\r\n\r\n",
            ),
            "desktop plugin archive exceeds size limit",
        ),
        (
            Reply::raw("HTTP/1.1 200 OK\r\n", b"123456789"),
            "desktop plugin archive exceeds size limit",
        ),
        (Reply::body(b""), "desktop plugin archive is empty"),
    ];
    for (reply, expected) in cases {
        let server = Fixture::new(vec![reply]).await;
        assert_eq!(
            download_with_policy(&server.url, &fixture_policy())
                .await
                .unwrap_err(),
            expected
        );
    }
    let server = Fixture::new(vec![Reply::raw(
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n",
        b"4\r\n1234\r\n4\r\n5678\r\n0\r\n\r\n",
    )])
    .await;
    assert_eq!(
        download_with_policy(&server.url, &fixture_policy())
            .await
            .unwrap(),
        b"12345678"
    );
}

#[tokio::test]
async fn redirects_follow_relative_signed_urls_without_forwarding_response_credentials() {
    let mut redirect = Reply::redirect("/asset.idplugin?token=secret");
    redirect.header.push_str("Set-Cookie: token=private\r\n");
    let server = Fixture::new(vec![redirect, Reply::body(b"archive")]).await;
    assert_eq!(
        download_with_policy(&server.url, &fixture_policy())
            .await
            .unwrap(),
        b"archive"
    );
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].starts_with("GET /asset.idplugin?token=secret HTTP/1.1\r\n"));
    for request in requests {
        let request = request.to_ascii_lowercase();
        for header in ["authorization:", "cookie:", "referer:"] {
            assert!(!request.contains(header), "{header}");
        }
    }
}

#[tokio::test]
async fn redirect_loops_are_bounded_and_invalid_targets_never_receive_a_request() {
    let server = Fixture::new(vec![
        Reply::redirect("/loop?token=secret"),
        Reply::redirect("/loop?token=secret"),
        Reply::redirect("/loop?token=secret"),
    ])
    .await;
    assert_eq!(
        download_with_policy(&server.url, &fixture_policy())
            .await
            .unwrap_err(),
        "desktop plugin download exceeded redirect limit"
    );
    assert_eq!(server.requests().len(), 3);
    for location in [
        "http://example.invalid/secret",
        "http://user:secret@127.0.0.1/asset",
        "/asset#secret",
    ] {
        let server = Fixture::new(vec![Reply::redirect(location)]).await;
        assert_eq!(
            download_with_policy(&server.url, &fixture_policy())
                .await
                .unwrap_err(),
            "invalid desktop plugin download redirect"
        );
        assert_eq!(server.requests().len(), 1);
    }
}

#[tokio::test]
async fn failed_http_missing_redirect_and_truncated_body_errors_are_redacted() {
    let cases = [
        (
            Reply::raw("HTTP/1.1 403 Forbidden\r\nContent-Length: 6\r\n", b"secret"),
            "desktop plugin download returned an unsuccessful response",
        ),
        (
            Reply::raw(
                "HTTP/1.1 300 Multiple Choices\r\nContent-Length: 0\r\n",
                b"",
            ),
            "desktop plugin download returned an unsuccessful response",
        ),
        (
            Reply::raw("HTTP/1.1 302 Found\r\nContent-Length: 0\r\n", b""),
            "invalid desktop plugin download redirect",
        ),
        (
            Reply::raw("HTTP/1.1 200 OK\r\nContent-Length: 8\r\n", b"secret"),
            "desktop plugin archive transfer failed",
        ),
    ];
    for (reply, expected) in cases {
        let server = Fixture::new(vec![Reply::redirect("/asset?token=private"), reply]).await;
        let error = download_with_policy(&server.url, &fixture_policy())
            .await
            .unwrap_err();
        assert_eq!(error, expected);
        assert!(!error.contains("secret"));
        assert!(!error.contains("private"));
        assert!(!error.contains("127.0.0.1"));
    }
}

#[tokio::test]
async fn one_total_deadline_covers_redirects_and_body_transfer() {
    let mut first = Reply::redirect("/second?token=secret");
    first.before_headers = Duration::from_millis(150);
    let mut second = Reply::body(b"archive");
    second.before_headers = Duration::from_millis(150);
    let server = Fixture::new(vec![first, second]).await;
    let policy = DownloadPolicy {
        total_timeout: Duration::from_millis(250),
        ..fixture_policy()
    };
    assert_eq!(
        download_with_policy(&server.url, &policy)
            .await
            .unwrap_err(),
        "desktop plugin download exceeded time limit"
    );

    let mut reply = Reply::body(b"archive");
    reply.before_body = Duration::from_secs(1);
    let server = Fixture::new(vec![reply]).await;
    assert_eq!(
        download_with_policy(&server.url, &policy)
            .await
            .unwrap_err(),
        "desktop plugin download exceeded time limit"
    );
}
