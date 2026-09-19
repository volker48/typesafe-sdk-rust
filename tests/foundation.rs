use std::{collections::BTreeMap, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use typesafe_sdk::*;

fn request() -> SystemOneRequest {
    SystemOneRequest::new(
        "text".into(),
        BTreeMap::from([("q".into(), Question::noul("q"))]),
    )
    .unwrap()
}

async fn read_request(stream: &mut TcpStream) -> String {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut bytes = Vec::new();
        let mut buf = [0; 4096];
        loop {
            let n = stream.read(&mut buf).await.unwrap();
            assert!(n > 0, "client closed before completing its request");
            bytes.extend_from_slice(&buf[..n]);
            if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                let length: usize = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length: "))
                    .unwrap_or("0")
                    .parse()
                    .unwrap();
                if bytes.len() == end + 4 + length {
                    return String::from_utf8(bytes).unwrap();
                }
            }
        }
    })
    .await
    .unwrap()
}

fn response(status: u16, headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status} Test\r\nconnection: close\r\ncontent-length: {}\r\n{headers}\r\n{body}",
        body.len()
    )
}

async fn mock(
    count: usize,
    mut handler: impl FnMut(&str) -> String + Send + 'static,
) -> (Client, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::builder()
        .api_key("test-key")
        .base_url(format!("http://{}", listener.local_addr().unwrap()))
        .retry(RetryPolicy::disabled())
        .build()
        .unwrap();
    let task = tokio::spawn(async move {
        let mut requests = Vec::new();
        for _ in 0..count {
            let (mut stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let request = read_request(&mut stream).await;
            stream
                .write_all(handler(&request).as_bytes())
                .await
                .unwrap();
            requests.push(request);
        }
        requests
    });
    (client, task)
}

#[tokio::test]
async fn numeric_limits_fail_explicitly_and_preserve_raw_response() {
    for (body, path) in [
        (
            r#"{"model":"m","usage":{"input_tokens":9223372036854775808}}"#,
            "usage.input_tokens",
        ),
        (
            r#"{"model":"m","usage":{},"answers":{"q":{"type":"score","score":0,"confidence":1,"legend":{"9223372036854775808.0":"large"},"probabilities":{}}}}"#,
            "answers.q.legend.9223372036854775808.0",
        ),
        (
            r#"{"model":"m","usage":{},"answers":{"q":{"type":"noul","noul":NaN}}}"#,
            "",
        ),
        (
            r#"{"model":"m","usage":{},"answers":{"q":{"type":"noul","noul":Infinity}}}"#,
            "",
        ),
    ] {
        let (client, task) = mock(1, move |_| response(200, "", body)).await;
        let error = client.system_one(&request()).await.unwrap_err();
        assert_eq!(error.kind, ErrorKind::Validation);
        assert!(std::error::Error::source(&error).is_some());
        let api = error.api.unwrap();
        assert_eq!(api.field_path.as_deref(), Some(path));
        assert_eq!(api.metadata.body, body.as_bytes());
        task.await.unwrap();
    }
}

#[test]
fn serde_score_keys_are_exact_and_collisions_follow_input_order() {
    let answer: ScoreAnswer = serde_json::from_str(r#"{"score":0,"confidence":1,"legend":{"0.0":"first","0":"last","9007199254740993.0":"exact"},"probabilities":{"1_0.00":1}}"#).unwrap();
    assert_eq!(answer.legend[&0], Content::from("last"));
    assert_eq!(
        answer.legend[&9_007_199_254_740_993],
        Content::from("exact")
    );
    assert_eq!(answer.probabilities[&10], 1.0);
}

#[tokio::test]
async fn server_retry_delay_is_exposed_without_disclosing_response_secrets() {
    let (client, task) = mock(1, |_| {
        response(
            429,
            "retry-after-ms: 125\r\nx-private: header-secret\r\n",
            r#"{"error":"body-secret"}"#,
        )
    })
    .await;
    let error = client.system_one(&request()).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::RateLimit);
    let api = error.api.as_ref().unwrap();
    assert_eq!(api.retry_after, Some(Duration::from_millis(125)));
    let display = format!("{error} {error:?} {api:?} {:?}", api.metadata);
    assert!(!display.contains("header-secret"));
    assert!(!display.contains("body-secret"));
    task.await.unwrap();
}

#[tokio::test]
async fn unrepresentable_ms_delay_does_not_fall_back_to_seconds() {
    let (client, task) = mock(1, |_| {
        response(429, "retry-after-ms: 1e30\r\nretry-after: 2\r\n", "{}")
    })
    .await;
    let error = client.system_one(&request()).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::RateLimit);
    assert_eq!(error.api.unwrap().retry_after, None);
    task.await.unwrap();
}

#[tokio::test]
async fn concurrent_calls_keep_retry_options_and_headers_independent() {
    for models in [false, true] {
        let (client, task) = mock(3, |wire| {
            if wire.contains("x-typesafe-retry-count: 1\r\n") {
                assert!(wire.contains("x-call: retry\r\n"));
                response(
                    200,
                    "",
                    if wire.starts_with("GET ") {
                        r#"{"models":[]}"#
                    } else {
                        r#"{"model":"m","usage":{}}"#
                    },
                )
            } else {
                response(429, "retry-after-ms: 0\r\n", "{}")
            }
        })
        .await;
        let retry = RequestOptions {
            headers: HeaderMap::from_iter([(
                HeaderName::from_static("x-call"),
                HeaderValue::from_static("retry"),
            )]),
            retry: Some(RetryPolicy {
                max_retries: 1,
                ..Default::default()
            }),
            ..Default::default()
        };
        let no_retry = RequestOptions {
            headers: HeaderMap::from_iter([(
                HeaderName::from_static("x-call"),
                HeaderValue::from_static("once"),
            )]),
            ..Default::default()
        };
        let request = request();
        let clone = client.clone();
        let (retried, once) = tokio::join!(
            async {
                if models {
                    client.list_models_with(&retry).await.map(|_| ())
                } else {
                    client.system_one_with(&request, &retry).await.map(|_| ())
                }
            },
            clone.system_one_with(&request, &no_retry)
        );
        assert!(retried.is_ok());
        assert_eq!(once.unwrap_err().kind, ErrorKind::RateLimit);
        let requests = task.await.unwrap();
        let retried: Vec<_> = requests
            .iter()
            .filter(|wire| wire.contains("x-call: retry\r\n"))
            .collect();
        assert_eq!(retried.len(), 2);
        assert!(!retried[0].contains("x-typesafe-retry-count"));
        assert!(retried[1].contains("x-typesafe-retry-count: 1\r\n"));
        let once = requests
            .iter()
            .find(|wire| wire.contains("x-call: once\r\n"))
            .unwrap();
        assert!(!once.contains("x-typesafe-retry-count"));
        assert!(once.starts_with("POST /v1/systemone HTTP/1.1\r\n"));
        if models {
            for wire in retried {
                assert!(wire.starts_with("GET /v1/models HTTP/1.1\r\n"));
                assert!(wire.ends_with("\r\n\r\n"));
            }
        }
    }
}

#[tokio::test]
async fn timeout_retry_recovers_and_can_be_disabled() {
    for models in [false, true] {
        for enabled in [true, false] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let client = Client::builder()
                .api_key("key")
                .base_url(format!("http://{}", listener.local_addr().unwrap()))
                .timeout(if models {
                    Duration::from_secs(5)
                } else {
                    Duration::from_millis(100)
                })
                .retry(RetryPolicy {
                    max_retries: 1,
                    backoff_initial: Duration::ZERO,
                    api_timeout_error: enabled,
                    ..Default::default()
                })
                .build()
                .unwrap();
            let task = tokio::spawn(async move {
                let (mut stalled, _) = listener.accept().await.unwrap();
                let first = read_request(&mut stalled).await;
                assert!(!first.contains("x-typesafe-retry-count"));
                let next =
                    tokio::time::timeout(Duration::from_millis(500), listener.accept()).await;
                if enabled {
                    let (mut second, _) = next.unwrap().unwrap();
                    assert!(
                        read_request(&mut second)
                            .await
                            .contains("x-typesafe-retry-count: 1\r\n")
                    );
                    second
                        .write_all(
                            response(
                                200,
                                "",
                                if models {
                                    r#"{"models":[]}"#
                                } else {
                                    r#"{"model":"m","usage":{}}"#
                                },
                            )
                            .as_bytes(),
                        )
                        .await
                        .unwrap();
                } else {
                    assert!(next.is_err(), "timeout retry was disabled");
                }
                drop(stalled);
            });
            let result = if models {
                client
                    .list_models_with(&RequestOptions {
                        timeout: Some(Duration::from_millis(100)),
                        ..Default::default()
                    })
                    .await
                    .map(|_| ())
            } else {
                client.system_one(&request()).await.map(|_| ())
            };
            if enabled {
                assert!(result.is_ok());
            } else {
                assert_eq!(result.unwrap_err().kind, ErrorKind::Timeout);
            }
            task.await.unwrap();
        }
    }
}

#[tokio::test]
async fn cancelling_a_pending_retry_leaves_no_background_request() {
    for models in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = Client::builder()
            .api_key("key")
            .base_url(format!("http://{}", listener.local_addr().unwrap()))
            .retry(RetryPolicy {
                max_retries: 1,
                timeout: None,
                ..Default::default()
            })
            .build()
            .unwrap();
        let (sent, received) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_request(&mut stream).await;
            stream
                .write_all(response(429, "retry-after-ms: 150\r\n", "{}").as_bytes())
                .await
                .unwrap();
            stream.shutdown().await.unwrap();
            sent.send(()).unwrap();
            // Keep accepting past the scheduled retry time so a detached retry is observable.
            assert!(
                tokio::time::timeout(Duration::from_millis(400), listener.accept())
                    .await
                    .is_err()
            );
        });
        let call = tokio::spawn(async move {
            if models {
                client.list_models().await.map(|_| ())
            } else {
                client.system_one(&request()).await.map(|_| ())
            }
        });
        tokio::time::timeout(Duration::from_secs(5), received)
            .await
            .unwrap()
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        call.abort();
        assert!(call.await.unwrap_err().is_cancelled());
        server.await.unwrap();
    }
}
