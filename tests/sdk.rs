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
                    .unwrap_or("0")
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

#[tokio::test]
async fn json_requests_preserve_typed_questions_on_the_wire() {
    let state = serde_json::json!({"message": "Payment failed", "attempts": 3});
    let questions = serde_json::json!({
        "urgent": {"type": "noul", "instructions": "Is this urgent?",
                   "criteria": {"true": "Deadline", "false": null}},
        "team": {"type": "choice", "criteria": {"billing": "Payments", "other": null}},
        "sentiment": {"type": "score", "instructions": null,
                      "criteria": ["Calm", {"label": "Angry", "signals": [true, null]}]}
    });
    let json_request = SystemOneRequest::from_json(state.clone(), questions.clone()).unwrap();
    let typed_request = SystemOneRequest::new(
        serde_json::from_value(state.clone()).unwrap(),
        serde_json::from_value(questions.clone()).unwrap(),
    )
    .unwrap();
    for request in [json_request, typed_request] {
        let (url, task) = server("HTTP/1.1 200 OK\r\nconnection: close\r\n\r\n{\"model\":\"m\",\"usage\":{},\"answers\":{}}").await;
        let client = Client::builder()
            .api_key("fixture")
            .base_url(url)
            .model("m")
            .build()
            .unwrap();
        client.system_one(&request).await.unwrap();
        let wire = task.await.unwrap();
        let body: serde_json::Value =
            serde_json::from_str(wire.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(
            body,
            serde_json::json!({"state": state, "questions": questions, "model": "m"})
        );
    }
}

#[test]
fn json_input_errors_identify_the_field_without_disclosing_it_in_logs() {
    let name = "secret/~.\"雪";
    let error = SystemOneRequest::from_json(
        serde_json::json!("private evidence"),
        serde_json::json!({name: {"type": "choice", "criteria": {"billing/~": 42}}}),
    )
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Input);
    assert!(error.api.is_none());
    let details = error.input_details().unwrap();
    assert_eq!(details.kind, InputErrorKind::InvalidContent);
    assert_eq!(
        details.path,
        "/questions/secret~1~0.\"雪/criteria/billing~1~0"
    );
    assert!(format!("{error:?}").contains("InvalidContent"));
    let logged = format!("{error} {error:?} {details:?}");
    for secret in ["secret", "雪", "billing", "42", "private evidence"] {
        assert!(!logged.contains(secret), "disclosed {secret}");
    }
}

#[test]
fn typed_and_json_requests_share_semantic_errors_and_question_order() {
    let typed = SystemOneRequest::new(
        "x".into(),
        BTreeMap::from([(
            "secret/q".into(),
            Question::Score {
                instructions: Field::Omitted,
                criteria: vec![],
            },
        )]),
    )
    .unwrap_err();
    let json = SystemOneRequest::from_json(
        serde_json::json!("x"),
        serde_json::json!({
            "secret/q": {"type": "score", "criteria": []}
        }),
    )
    .unwrap_err();
    for error in [typed, json] {
        let details = error.input_details().unwrap();
        assert_eq!(details.kind, InputErrorKind::EmptyScoreCriteria);
        assert_eq!(details.path, "/questions/secret~1q/criteria");
        assert!(!format!("{error} {error:?}").contains("secret"));
    }
    for error in [
        SystemOneRequest::new("x".into(), BTreeMap::new()).unwrap_err(),
        SystemOneRequest::from_json(serde_json::json!("x"), serde_json::json!({})).unwrap_err(),
    ] {
        assert_eq!(
            error.input_details().unwrap().kind,
            InputErrorKind::EmptyQuestions
        );
        assert_eq!(error.input_details().unwrap().path, "/questions");
    }
    // The first named question wins even when a later question cannot be decoded.
    let error = SystemOneRequest::from_json(
        serde_json::json!("x"),
        serde_json::json!({
            "z": {"type": "future"}, "a": {"type": "score", "criteria": []}
        }),
    )
    .unwrap_err();
    assert_eq!(error.input_details().unwrap().path, "/questions/a/criteria");
}

#[test]
fn json_shapes_have_stable_repair_diagnostics() {
    use InputErrorKind::*;
    use serde_json::json;
    for (state, questions, kind, path) in [
        (json!(null), json!({}), InvalidContent, "/state"),
        (json!(false), json!({}), InvalidContent, "/state"),
        (json!(3), json!({}), InvalidContent, "/state"),
        (json!("x"), json!([]), InvalidType, "/questions"),
        (json!("x"), json!({"q": false}), InvalidType, "/questions/q"),
        (
            json!("x"),
            json!({"q": {}}),
            MissingField,
            "/questions/q/type",
        ),
        (
            json!("x"),
            json!({"q": {"type": null}}),
            InvalidType,
            "/questions/q/type",
        ),
        (
            json!("x"),
            json!({"q": {"type": "future"}}),
            UnknownQuestionKind,
            "/questions/q/type",
        ),
        (
            json!("x"),
            json!({"q": {"type": "noul", "instruction": "typo"}}),
            UnknownField,
            "/questions/q/instruction",
        ),
        (
            json!("x"),
            json!({"q": {"type": "noul", "instructions": 1}}),
            InvalidContent,
            "/questions/q/instructions",
        ),
        (
            json!("x"),
            json!({"q": {"type": "choice"}}),
            MissingField,
            "/questions/q/criteria",
        ),
        (
            json!("x"),
            json!({"q": {"type": "score"}}),
            MissingField,
            "/questions/q/criteria",
        ),
        (
            json!("x"),
            json!({"q": {"type": "noul", "criteria": []}}),
            InvalidType,
            "/questions/q/criteria",
        ),
        (
            json!("x"),
            json!({"q": {"type": "noul", "criteria": {"maybe/~": "x"}}}),
            UnknownField,
            "/questions/q/criteria/maybe~1~0",
        ),
        (
            json!("x"),
            json!({"q": {"type": "noul", "criteria": {"true": 1}}}),
            InvalidContent,
            "/questions/q/criteria/true",
        ),
        (
            json!("x"),
            json!({"q": {"type": "noul", "criteria": {"false": false}}}),
            InvalidContent,
            "/questions/q/criteria/false",
        ),
        (
            json!("x"),
            json!({"q": {"type": "choice", "criteria": null}}),
            InvalidType,
            "/questions/q/criteria",
        ),
        (
            json!("x"),
            json!({"q": {"type": "choice", "criteria": []}}),
            InvalidType,
            "/questions/q/criteria",
        ),
        (
            json!("x"),
            json!({"q": {"type": "score", "criteria": {}}}),
            InvalidType,
            "/questions/q/criteria",
        ),
        (
            json!("x"),
            json!({"q": {"type": "score", "criteria": null}}),
            InvalidType,
            "/questions/q/criteria",
        ),
        (
            json!("x"),
            json!({"q": {"type": "score", "criteria": ["valid", null]}}),
            InvalidContent,
            "/questions/q/criteria/1",
        ),
    ] {
        let error = SystemOneRequest::from_json(state, questions).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Input);
        assert!(error.api.is_none());
        let details = error.input_details().unwrap();
        assert_eq!((details.kind, details.path.as_str()), (kind, path));
        if matches!(kind, InvalidContent | InvalidType) {
            assert!(std::error::Error::source(&error).is_some());
        }
    }
    assert_eq!(
        serde_json::to_value(InvalidContent).unwrap(),
        json!("invalid_content")
    );
}

#[test]
fn local_error_order_is_independent_of_object_insertion_order() {
    use serde_json::json;
    for (question, expected) in [
        (
            json!({"criteria": [], "type": "future", "bad": true}),
            "/type",
        ),
        (
            json!({"type": "noul", "z": true, "a": true, "instructions": 3}),
            "/a",
        ),
        (
            json!({"type": "score", "criteria": null, "instructions": false}),
            "/instructions",
        ),
        (
            json!({"type": "noul", "criteria": {"false": 1, "true": 1}}),
            "/criteria/true",
        ),
        (
            json!({"type": "choice", "criteria": {"z": 1, "a": 1}}),
            "/criteria/a",
        ),
        (json!({"type": "score", "criteria": [1, 2]}), "/criteria/0"),
    ] {
        let error = SystemOneRequest::from_json(json!("x"), json!({"q": question})).unwrap_err();
        assert_eq!(
            error.input_details().unwrap().path,
            format!("/questions/q{expected}")
        );
    }
}

#[tokio::test]
async fn preview_matches_execution_after_model_and_raw_overrides() {
    use serde_json::json;
    let (url, task) =
        server("HTTP/1.1 200 OK\r\nconnection: close\r\n\r\n{\"model\":\"m\",\"usage\":{}}").await;
    let client = Client::builder()
        .api_key("private-api-key")
        .base_url(url)
        .model("inherited")
        .build()
        .unwrap();
    let mut request =
        SystemOneRequest::from_json(json!("x"), json!({"q": {"type": "noul"}})).unwrap();
    let original = client.system_one_body(&request).unwrap();
    assert_eq!(
        original,
        json!({"state": "x", "questions": {"q": {"type": "noul"}}, "model": "inherited"})
    );
    request.model = Some("request-model".into());
    assert_eq!(
        client.system_one_body(&request).unwrap()["model"],
        "request-model"
    );
    request.extra_body = serde_json::from_value(json!({
        "model": null, "state": {"replacement": true}, "questions": null, "extension": [1, null]
    }))
    .unwrap();
    let preview = client.system_one_body(&request).unwrap();
    assert_eq!(
        preview,
        json!({"model": null, "state": {"replacement": true}, "questions": null, "extension": [1, null]})
    );
    // Preview does not mutate prior snapshots or send HTTP; the server accepts one request.
    assert_eq!(original["model"], "inherited");
    client.system_one(&request).await.unwrap();
    let wire = task.await.unwrap();
    let body: serde_json::Value =
        serde_json::from_str(wire.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(body, preview);
    assert!(!preview.to_string().contains("private-api-key"));
    assert!(wire.contains("authorization: Bearer private-api-key"));
}

#[tokio::test]
async fn json_presence_and_content_match_existing_typed_deserialization() {
    use serde_json::json;
    let client = Client::builder()
        .api_key("fixture")
        .model("m")
        .build()
        .unwrap();
    for question in [
        json!({"type": "noul"}),
        json!({"type": "noul", "instructions": null, "criteria": null}),
        json!({"type": "noul", "instructions": [1, null, false], "criteria": {}}),
        json!({"type": "noul", "criteria": {"true": null}}),
        json!({"type": "noul", "criteria": {"false": {"nested": [false, 2, null]}}}),
        json!({"type": "choice", "criteria": {}}),
        json!({"type": "choice", "instructions": {}, "criteria": {"": null, "label": [1, false]}}),
        json!({"type": "score", "criteria": ["only level"]}),
        json!({"type": "score", "criteria": [[], {}, "text"]}),
    ] {
        for state in [json!("text"), json!({}), json!([]), json!([true, 3, null])] {
            let questions = json!({"": question, "dynamic/~.雪": {"type": "noul"}});
            let typed = SystemOneRequest::new(
                serde_json::from_value(state.clone()).unwrap(),
                serde_json::from_value(questions.clone()).unwrap(),
            )
            .unwrap();
            let decoded = SystemOneRequest::from_json(state.clone(), questions.clone()).unwrap();
            let body = client.system_one_body(&decoded).unwrap();
            assert_eq!(
                body,
                json!({"state": state, "questions": questions, "model": "m"})
            );
            assert_eq!(body, client.system_one_body(&typed).unwrap());
        }
    }
}

#[tokio::test]
async fn json_literals_keep_serde_duplicate_and_optional_value_semantics() {
    use serde_json::json;
    let name = "dynamic";
    let optional_text: Option<&str> = None;
    let request = SystemOneRequest::from_json(
        json!({"value": null}),
        json!({
            name: {"type": "noul", "instructions": "First"},
            name: {"type": "noul", "instructions": optional_text},
        }),
    )
    .unwrap();
    let client = Client::builder()
        .api_key("fixture")
        .model("m")
        .build()
        .unwrap();
    assert_eq!(
        client.system_one_body(&request).unwrap()["questions"],
        json!({"dynamic": {"type": "noul", "instructions": null}})
    );
}

#[tokio::test]
async fn list_models_is_bodyless_and_preserves_metadata() {
    let body = r#"{"models":[{"name":"jev","description":"Model","release_date":"not a date","extra":1}]}"#;
    let (url, task) = server("HTTP/1.1 200 OK\r\nx-typesafe-request-id: models-id\r\nconnection: close\r\n\r\n{\"models\":[{\"name\":\"jev\",\"description\":\"Model\",\"release_date\":\"not a date\",\"extra\":1}]}").await;
    let client = Client::builder()
        .api_key("test-key")
        .base_url(format!("{url}/gateway/"))
        .build()
        .unwrap();
    let result: Response<ListModelsResponse> = client.list_models().await.unwrap();
    assert_eq!(
        result.data.models,
        vec![ModelMetadata {
            name: "jev".into(),
            description: "Model".into(),
            release_date: "not a date".into(),
        }]
    );
    assert_eq!(result.metadata.body, body.as_bytes());
    assert_eq!(result.metadata.request_id(), Some("models-id"));
    let data = serde_json::to_value(&result.data).unwrap();
    assert_eq!(
        data,
        serde_json::json!({"models":[{"name":"jev","description":"Model","release_date":"not a date"}]})
    );
    assert_eq!(
        serde_json::from_value::<ListModelsResponse>(data).unwrap(),
        result.data
    );
    let wire = task.await.unwrap();
    assert!(wire.starts_with("GET /gateway/v1/models HTTP/1.1\r\n"));
    assert!(wire.contains("authorization: Bearer test-key\r\n"));
    assert!(!wire.contains("content-type:"));
    assert!(wire.ends_with("\r\n\r\n"));
}

#[tokio::test]
async fn list_models_validation_keeps_index_context_and_redacts_values() {
    let (url, task) = server("HTTP/1.1 200 OK\r\nx-typesafe-request-id: invalid-id\r\nconnection: close\r\n\r\n{\"models\":[{\"release_date\":null,\"description\":{\"secret\":\"server-secret\"}}]}").await;
    let client = Client::builder()
        .api_key("key")
        .base_url(&url)
        .build()
        .unwrap();
    let error = client.list_models().await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Validation);
    assert!(std::error::Error::source(&error).is_some());
    let api = error.api.as_ref().unwrap();
    assert_eq!(api.field_path.as_deref(), Some("models[0].name"));
    assert_eq!(api.endpoint, format!("GET {url}/v1/models"));
    assert_eq!(api.metadata.request_id(), Some("invalid-id"));
    assert!(!format!("{error:?} {error}").contains("server-secret"));
    assert_eq!(
        api.body["models"][0]["description"]["secret"],
        "server-secret"
    );
    task.await.unwrap();
}

#[tokio::test]
async fn list_models_rejects_invalid_options_before_network_io() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::builder()
        .api_key("key")
        .base_url(format!("http://{}", listener.local_addr().unwrap()))
        .build()
        .unwrap();
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
