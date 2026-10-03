//! Client configuration, per-call options, retries, timeouts, and cancellation.
mod common;

use common::{base_url, client, mock, read_request, request, response, server};
use serde_json::json;
use std::time::Duration;
use tokio::{io::AsyncWriteExt, net::TcpListener};
use typesafe_sdk::*;

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

#[tokio::test]
async fn list_models_rejects_invalid_options_before_network_io() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = client(base_url(&listener));
    for options in [
        RequestOptions {
            timeout: Some(Duration::ZERO),
            ..Default::default()
        },
        RequestOptions {
            retry: Some(RetryPolicy {
                backoff_jitter: f64::NAN,
                ..Default::default()
            }),
            ..Default::default()
        },
    ] {
        assert_eq!(
            client.list_models_with(&options).await.unwrap_err().kind,
            ErrorKind::Input
        );
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn custom_response_rejects_invalid_options_before_io() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = client(base_url(&listener));
    for options in [
        RequestOptions {
            timeout: Some(Duration::ZERO),
            ..Default::default()
        },
        RequestOptions {
            retry: Some(RetryPolicy {
                backoff_jitter: f64::NAN,
                ..Default::default()
            }),
            ..Default::default()
        },
    ] {
        let error = client
            .system_one_as_with::<serde_json::Value>(&request(), &options)
            .await
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Input);
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn timeout_is_structured_and_dropping_future_cancels_retry() {
    // No server response is needed; the listener's backlog holds the socket.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::builder()
        .api_key("key")
        .base_url(base_url(&listener))
        .retry(RetryPolicy::disabled())
        .timeout(Duration::from_millis(20))
        .build()
        .unwrap();
    let error = client.system_one(&request()).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Timeout);
    let (url, task) = server(response(429, "retry-after: 3600\r\n", "{}")).await;
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
async fn timeout_retry_recovers_and_can_be_disabled() {
    for models in [false, true] {
        for enabled in [true, false] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let client = Client::builder()
                .api_key("key")
                .base_url(base_url(&listener))
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
async fn custom_response_respects_per_call_retries_and_preserves_http_errors() {
    let (client, task) = mock(3, |wire| {
        if wire.contains("x-typesafe-retry-count: 1\r\n") {
            response(200, "", r#"{"future":{"value":3}}"#)
        } else {
            response(429, "retry-after-ms: 0\r\n", r#"{"detail":"try again"}"#)
        }
    })
    .await;
    let options = RequestOptions {
        headers: HeaderMap::from_iter([(
            HeaderName::from_static("x-call"),
            HeaderValue::from_static("custom"),
        )]),
        retry: Some(RetryPolicy {
            max_retries: 1,
            ..Default::default()
        }),
        ..Default::default()
    };
    let result = client
        .system_one_as_with::<serde_json::Value>(&request(), &options)
        .await
        .unwrap();
    assert_eq!(result.data, json!({"future":{"value":3}}));
    let error = client
        .system_one_as::<serde_json::Value>(&request())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::RateLimit);
    let api = error.api.unwrap();
    assert_eq!(api.body, json!({"detail":"try again"}));
    assert_eq!(api.retry_after, Some(Duration::ZERO));
    let requests = task.await.unwrap();
    assert!(requests[0].contains("x-call: custom\r\n"));
    assert!(requests[1].contains("x-call: custom\r\n"));
    assert!(!requests[2].contains("x-call:"));
    assert!(!requests[2].contains("x-typesafe-retry-count:"));
    assert_eq!(
        requests[0].split_once("\r\n\r\n").unwrap().1,
        requests[1].split_once("\r\n\r\n").unwrap().1
    );
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
async fn cancelling_a_pending_retry_leaves_no_background_request() {
    for models in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = Client::builder()
            .api_key("key")
            .base_url(base_url(&listener))
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
