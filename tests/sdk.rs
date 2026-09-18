use std::{collections::BTreeMap, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use typesafe_sdk::*;

fn request() -> SystemOneRequest {
    SystemOneRequest::new(
        "text".into(),
        BTreeMap::from([("q".into(), Question::noul("Question?"))]),
    )
    .unwrap()
}

#[test]
fn typed_boundaries_and_presence() {
    assert!(SystemOneRequest::new("x".into(), BTreeMap::new()).is_err());
    assert!(
        SystemOneRequest::new(
            "x".into(),
            BTreeMap::from([(
                "q".into(),
                Question::Score {
                    instructions: Field::Omitted,
                    criteria: vec![]
                }
            )])
        )
        .is_err()
    );
    for value in ["null", "true", "3"] {
        assert!(serde_json::from_str::<Content>(value).is_err());
    }
    let omitted = Question::Noul {
        instructions: Field::Omitted,
        criteria: Field::Omitted,
    };
    let null = Question::Noul {
        instructions: Field::Null,
        criteria: Field::Omitted,
    };
    assert_eq!(
        serde_json::to_value(omitted).unwrap(),
        serde_json::json!({"type":"noul"})
    );
    assert_eq!(
        serde_json::to_value(null).unwrap(),
        serde_json::json!({"type":"noul","instructions":null})
    );
    assert!(serde_json::from_str::<Question>(r#"{"type":"noul","unexpected":1}"#).is_err());
    assert!(serde_json::from_str::<Question>(r#"{"type":"future"}"#).is_err());
}

#[tokio::test]
async fn configuration_fails_as_errors() {
    assert!(
        Client::builder()
            .api_key("key")
            .timeout(Duration::ZERO)
            .build()
            .is_err()
    );
    assert!(Client::builder().api_key("key\nsecret").build().is_err());
    for url in [
        "file:///tmp/api",
        "http://user:secret@localhost",
        "https://example.com?token=secret",
        "https://example.com#fragment",
    ] {
        assert!(
            Client::builder()
                .api_key("key")
                .base_url(url)
                .build()
                .is_err()
        );
    }
    let retry = RetryPolicy {
        backoff_jitter: f64::NAN,
        ..Default::default()
    };
    assert!(
        Client::builder()
            .api_key("key")
            .retry(retry)
            .build()
            .is_err()
    );
}

async fn server(response: &'static str) -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        let mut buf = [0; 4096];
        loop {
            let n = stream.read(&mut buf).await.unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buf[..n]);
            if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                let length: usize = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length: "))
                    .unwrap()
                    .parse()
                    .unwrap();
                if bytes.len() >= end + 4 + length {
                    break;
                }
            }
        }
        stream.write_all(response.as_bytes()).await.unwrap();
        String::from_utf8(bytes).unwrap()
    });
    (url, task)
}

#[tokio::test]
async fn round_trip_and_metadata() {
    let body = r#"{"model":"m","usage":{"input_tokens":12},"answers":{"q":{"type":"noul","noul":0.98},"future":{"type":"future"}}}"#;
    // Fixed HTTP fixture, independent of the compatibility adapter.
    let response: &'static str = "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nx-typesafe-request-id: native-id\r\nconnection: close\r\n\r\n{\"model\":\"m\",\"usage\":{\"input_tokens\":12},\"answers\":{\"q\":{\"type\":\"noul\",\"noul\":0.98},\"future\":{\"type\":\"future\"}}}";
    let (url, task) = server(response).await;
    let client = Client::builder()
        .api_key("test-key")
        .base_url(url)
        .build()
        .unwrap();
    let result = client.system_one(&request()).await.unwrap();
    assert_eq!(result.data.nouls().collect::<Vec<_>>(), vec![("q", 0.98)]);
    assert_eq!(result.data.usage.output_tokens, None);
    assert_eq!(result.metadata.body, body.as_bytes());
    assert_eq!(result.metadata.request_id(), Some("native-id"));
    assert_eq!(result.data.answers.len(), 1);
    let wire = task.await.unwrap();
    assert!(wire.starts_with("POST /v1/systemone HTTP/1.1\r\n"));
    assert!(wire.contains("authorization: Bearer test-key\r\n"));
    let serialized = serde_json::to_value(&result.data).unwrap();
    assert!(serialized.get("metadata").is_none());
}

#[tokio::test]
async fn validation_path_and_error_redaction() {
    let (url, task) =
        server("HTTP/1.1 200 OK\r\nconnection: close\r\n\r\n{\"usage\":{},\"answers\":{}}").await;
    let client = Client::builder()
        .api_key("secret-key")
        .base_url(url)
        .build()
        .unwrap();
    let error = client.system_one(&request()).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Validation);
    assert_eq!(
        error.api.as_ref().unwrap().field_path.as_deref(),
        Some("model")
    );
    assert!(!format!("{error:?}").contains("secret-key"));
    task.await.unwrap();
}

#[tokio::test]
async fn timeout_is_structured_and_dropping_future_cancels_retry() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::builder()
        .api_key("key")
        .base_url(format!("http://{}", listener.local_addr().unwrap()))
        .retry(RetryPolicy::disabled())
        .timeout(Duration::from_millis(20))
        .build()
        .unwrap();
    let error = client.system_one(&request()).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Timeout);
    // No server response is needed; the listener's backlog holds the socket.
    let (url, task) = server(
        "HTTP/1.1 429 Too Many Requests\r\nretry-after: 3600\r\nconnection: close\r\n\r\n{}",
    )
    .await;
    let client = Client::builder()
        .api_key("key")
        .base_url(url)
        .retry(RetryPolicy {
            timeout: None,
            ..Default::default()
        })
        .build()
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), client.system_one(&request()))
            .await
            .is_err()
    );
    task.await.unwrap();
}

#[tokio::test]
async fn validation_error_formatting_does_not_disclose_server_values() {
    let (url, task) = server("HTTP/1.1 200 OK\r\nconnection: close\r\n\r\n{\"model\":\"m\",\"usage\":{\"input_tokens\":\"server-secret\"}}").await;
    let client = Client::builder()
        .api_key("key")
        .base_url(url)
        .build()
        .unwrap();
    let error = client.system_one(&request()).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Validation);
    assert_eq!(
        error.api.as_ref().unwrap().field_path.as_deref(),
        Some("usage.input_tokens")
    );
    assert!(!format!("{error:?} {error}").contains("server-secret"));
    assert!(std::error::Error::source(&error).is_some());
    assert_eq!(
        error.api.as_ref().unwrap().body["usage"]["input_tokens"],
        "server-secret"
    );
    task.await.unwrap();
}
