use std::{process::Command, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{ServerConfig, pki_types::PrivatePkcs8KeyDer},
};
use typesafe_sdk::{Client, ErrorKind, RetryPolicy};

// Trust and proxy environment is process-global. Re-execute only this test so
// the platform verifier sees controlled settings without mutating shared state.
fn isolated(name: &str, trust_ca: bool) -> bool {
    if std::env::var("TYPESAFE_TLS_TEST_CHILD").as_deref() == Ok(name) {
        return false;
    }
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", name, "--nocapture"]);
    command.env("TYPESAFE_TLS_TEST_CHILD", name);
    for (key, _) in std::env::vars_os() {
        let normalized = key.to_string_lossy().to_ascii_uppercase();
        if normalized.ends_with("_PROXY") || normalized.starts_with("SSL_CERT_") {
            command.env_remove(key);
        }
    }
    command.env("NO_PROXY", "*");
    if trust_ca {
        command.env(
            "SSL_CERT_FILE",
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tls/ca.pem"),
        );
    }
    assert!(command.status().unwrap().success(), "TLS child test failed");
    true
}

async fn exchange(host: &str, succeeds: bool) {
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![include_bytes!("fixtures/tls/server.der").to_vec().into()],
            PrivatePkcs8KeyDer::from(include_bytes!("fixtures/tls/server-key.der").to_vec()).into(),
        )
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let handshake = acceptor.accept(stream).await;
        if !succeeds {
            assert!(handshake.is_err(), "client accepted an invalid certificate");
            return;
        }
        let mut stream = handshake.unwrap();
        let mut request = Vec::new();
        loop {
            let byte = stream.read_u8().await.unwrap();
            request.push(byte);
            assert!(
                request.len() < 16_384,
                "request headers exceeded fixture limit"
            );
            if request.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        let request = String::from_utf8(request).unwrap();
        assert!(request.starts_with("GET /v1/models HTTP/1.1\r\n"));
        assert!(
            request
                .to_lowercase()
                .contains("authorization: bearer tls-fixture\r\n")
        );
        let body = r#"{"models":[{"name":"tls-model","description":"local","release_date":"2026-01-01"}]}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\nx-typesafe-request-id: tls-fixture\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        stream.shutdown().await.unwrap();
    });
    let client = Client::builder()
        .api_key("tls-fixture")
        .base_url(format!("https://{host}:{port}"))
        .retry(RetryPolicy::disabled())
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let response = client.list_models().await;
    if succeeds {
        let response = response.unwrap();
        assert_eq!(response.data.models[0].name, "tls-model");
        assert_eq!(response.metadata.status, 200);
        assert_eq!(response.metadata.request_id(), Some("tls-fixture"));
    } else {
        let error = response.unwrap_err();
        assert_eq!(error.kind, ErrorKind::Connection);
        assert!(std::error::Error::source(&error).is_some());
        assert!(error.api.is_none());
    }
    server.await.unwrap();
}

fn run(host: &str, succeeds: bool) {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        tokio::time::timeout(Duration::from_secs(10), exchange(host, succeeds))
            .await
            .expect("local TLS exchange exceeded deadline");
    });
}

#[test]
fn rejects_untrusted_certificate() {
    if !isolated("rejects_untrusted_certificate", false) {
        run("localhost", false);
    }
}

// Only Linux's platform verifier supports a process-local SSL_CERT_FILE.
// macOS and Windows use system trust stores, which these tests never modify.
#[cfg(target_os = "linux")]
#[test]
fn accepts_trusted_local_https() {
    if !isolated("accepts_trusted_local_https", true) {
        run("localhost", true);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn rejects_trusted_certificate_for_wrong_host() {
    if !isolated("rejects_trusted_certificate_for_wrong_host", true) {
        run("127.0.0.1", false);
    }
}
