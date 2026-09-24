//! Standard and custom response decoding, validation paths, and redaction.
mod common;

use common::{client, mock, request, response, server};
use serde_json::json;
use std::collections::BTreeMap;
use typesafe_sdk::*;

#[tokio::test]
async fn round_trip_and_metadata() {
    let body = r#"{"model":"m","usage":{"input_tokens":12},"answers":{"q":{"type":"noul","noul":0.98},"future":{"type":"future"}}}"#;
    // Fixed HTTP fixture, independent of the compatibility adapter.
    let headers = "content-type: application/json\r\nx-typesafe-request-id: native-id\r\n";
    let (url, task) = server(response(200, headers, body)).await;
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
    let (url, task) = server(response(200, "", r#"{"usage":{},"answers":{}}"#)).await;
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
async fn validation_error_formatting_does_not_disclose_server_values() {
    let body = r#"{"model":"m","usage":{"input_tokens":"server-secret"}}"#;
    let (url, task) = server(response(200, "", body)).await;
    let client = client(url);
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
async fn custom_response_decodes_the_wire_schema_and_keeps_metadata() {
    #[derive(Debug, serde::Deserialize, PartialEq)]
    struct CustomAnswer {
        value: Vec<String>,
    }
    #[derive(Debug, serde::Deserialize)]
    struct CustomResponse {
        answers: BTreeMap<String, CustomAnswer>,
    }
    let body = r#"{"answers":{"q":{"type":"future","value":["kept"]}},"extra":true}"#;
    let (url, task) = server(response(200, "x-typesafe-request-id: custom-id\r\n", body)).await;
    let client = client(url);
    let response = client
        .system_one_as::<CustomResponse>(&request())
        .await
        .unwrap();
    assert_eq!(response.data.answers["q"].value, ["kept"]);
    assert_eq!(response.metadata.request_id(), Some("custom-id"));
    assert_eq!(response.metadata.status, 200);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&response.metadata.body).unwrap()["extra"],
        true
    );
    assert!(
        task.await
            .unwrap()
            .starts_with("POST /v1/systemone HTTP/1.1\r\n")
    );
}

#[tokio::test]
async fn custom_validation_preserves_paths_causes_and_http_context() {
    #[derive(Debug, serde::Deserialize)]
    struct CustomResponse {
        #[serde(rename = "answers")]
        _answers: BTreeMap<String, Vec<u32>>,
    }
    for (body, path) in [
        (r#"{"answers":{"q":[1,"server-secret"]}}"#, "answers.q[1]"),
        (r#"{"answers":null}"#, "answers"),
        (r#"{}"#, ""),
        (r#"[]"#, ""),
        (r#"{"answers":{}} trailing-secret"#, ""),
        (r#"{"answers":{}} {}"#, ""),
        ("{", ""),
        (r#"{"answers":{"q":[1,]}}"#, ""),
        ("", ""),
    ] {
        let (client, task) = mock(1, move |_| {
            response(
                200,
                "x-typesafe-request-id: custom-error\r\nx-private: header-secret\r\n",
                body,
            )
        })
        .await;
        let error = client
            .system_one_as_with::<CustomResponse>(
                &request(),
                &RequestOptions {
                    retry: Some(RetryPolicy::default()),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Validation);
        assert!(std::error::Error::source(&error).is_some());
        let api = error.api.as_ref().unwrap();
        assert_eq!(api.field_path.as_deref(), Some(path));
        assert_eq!(api.metadata.body, body.as_bytes());
        assert_eq!(api.metadata.request_id(), Some("custom-error"));
        assert!(api.endpoint.starts_with("POST http://127.0.0.1:"));
        assert!(api.endpoint.ends_with("/v1/systemone"));
        assert!(!format!("{error} {error:?} {api:?}").contains("secret"));
        task.await.unwrap();
    }
    // A literal dot is a map key, not Serde's root-path display marker.
    let (client, task) = mock(1, |_| response(200, "", r#"{".":false}"#)).await;
    let error = client
        .system_one_as::<BTreeMap<String, u32>>(&request())
        .await
        .unwrap_err();
    assert_eq!(error.api.unwrap().field_path.as_deref(), Some("."));
    task.await.unwrap();
}

#[tokio::test]
async fn list_models_is_bodyless_and_preserves_metadata() {
    let body = r#"{"models":[{"name":"jev","description":"Model","release_date":"not a date","extra":1}]}"#;
    let (url, task) = server(response(200, "x-typesafe-request-id: models-id\r\n", body)).await;
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
        json!({"models":[{"name":"jev","description":"Model","release_date":"not a date"}]})
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
    let body = r#"{"models":[{"release_date":null,"description":{"secret":"server-secret"}}]}"#;
    let (url, task) = server(response(200, "x-typesafe-request-id: invalid-id\r\n", body)).await;
    let client = client(&url);
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
