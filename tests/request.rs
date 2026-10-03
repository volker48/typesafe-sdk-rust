//! Request construction, validation diagnostics, and the effective wire body.
mod common;

use common::{client, response, server};
use serde_json::json;
use std::collections::BTreeMap;
use typesafe_sdk::*;

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
        json!({"type":"noul"})
    );
    assert_eq!(
        serde_json::to_value(null).unwrap(),
        json!({"type":"noul","instructions":null})
    );
    assert!(serde_json::from_str::<Question>(r#"{"type":"noul","unexpected":1}"#).is_err());
    assert!(serde_json::from_str::<Question>(r#"{"type":"future"}"#).is_err());
}

#[tokio::test]
async fn json_requests_preserve_typed_questions_on_the_wire() {
    let state = json!({"message": "Payment failed", "attempts": 3});
    let questions = json!({
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
        let (url, task) = server(response(
            200,
            "",
            r#"{"model":"m","usage":{},"answers":{}}"#,
        ))
        .await;
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
            json!({"state": state, "questions": questions, "model": "m"})
        );
    }
}

#[tokio::test]
async fn json_presence_and_content_match_existing_typed_deserialization() {
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

#[test]
fn json_input_errors_identify_the_field_without_disclosing_it_in_logs() {
    let name = "secret/~.\"雪";
    let error = SystemOneRequest::from_json(
        json!("private evidence"),
        json!({name: {"type": "choice", "criteria": {"billing/~": 42}}}),
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
        json!("x"),
        json!({
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
        SystemOneRequest::from_json(json!("x"), json!({})).unwrap_err(),
    ] {
        assert_eq!(
            error.input_details().unwrap().kind,
            InputErrorKind::EmptyQuestions
        );
        assert_eq!(error.input_details().unwrap().path, "/questions");
    }
    // The first named question wins even when a later question cannot be decoded.
    let error = SystemOneRequest::from_json(
        json!("x"),
        json!({
            "z": {"type": "future"}, "a": {"type": "score", "criteria": []}
        }),
    )
    .unwrap_err();
    assert_eq!(error.input_details().unwrap().path, "/questions/a/criteria");
}

#[test]
fn json_shapes_have_stable_repair_diagnostics() {
    use InputErrorKind::*;
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
async fn raw_questions_preserve_extensions_and_share_preview_and_execution() {
    let questions = json!({
        "future": {"type": "future", "instructions": null, "nested": [null, {"weight": 3}]},
        "extended": {"type": "noul", "weight": 2},
        "typed": Question::noul("Keep typed construction"),
        "server_validates": {"type": "choice", "criteria": false}
    });
    assert!(SystemOneRequest::from_json(json!("x"), questions.clone()).is_err());
    let mut request = SystemOneRequest::from_raw_json(json!("x"), questions.clone()).unwrap();
    request.model = Some("call-model".into());
    request.extra_body.insert("extension".into(), json!(null));
    let (url, task) = server(response(200, "", r#"{"model":"m","usage":{}}"#)).await;
    let client = client(url);
    let expected =
        json!({"state": "x", "questions": questions, "model": "call-model", "extension": null});
    assert_eq!(client.system_one_body(&request).unwrap(), expected);
    client.system_one(&request).await.unwrap();
    let wire = task.await.unwrap();
    let sent: serde_json::Value =
        serde_json::from_str(wire.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(sent, expected);
}

#[test]
fn raw_questions_validate_envelope_and_minimal_question_requirements() {
    for (state, questions, kind, path) in [
        (
            json!(null),
            json!({}),
            InputErrorKind::InvalidContent,
            "/state",
        ),
        (
            json!("x"),
            json!([]),
            InputErrorKind::InvalidType,
            "/questions",
        ),
        (
            json!("x"),
            json!({}),
            InputErrorKind::EmptyQuestions,
            "/questions",
        ),
        (
            json!("x"),
            json!({"q/~": null}),
            InputErrorKind::InvalidType,
            "/questions/q~1~0",
        ),
        (
            json!("x"),
            json!({"z": 1, "a": []}),
            InputErrorKind::InvalidType,
            "/questions/a",
        ),
    ] {
        let error = SystemOneRequest::from_raw_json(state, questions).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Input);
        assert!(error.api.is_none());
        let input = error.input_details().unwrap();
        assert_eq!((input.kind, input.path.as_str()), (kind, path));
    }
    for raw in [
        json!({"type": "future"}),
        json!({"type": " "}),
        json!({"type": "noul", "instructions": 2}),
        json!({"type": "choice", "criteria": false}),
    ] {
        assert!(SystemOneRequest::from_raw_json(json!([]), json!({"": raw})).is_ok());
    }
    for raw in [
        json!({}),
        json!({"type": null}),
        json!({"type": 2}),
        json!({"type": ""}),
        json!({"type": "choice"}),
        json!({"type": "score"}),
        json!({"type": "score", "criteria": []}),
        json!({"type": "score", "criteria": null}),
        json!({"type": "score", "criteria": false}),
        json!({"type": "score", "criteria": 0}),
        json!({"type": "score", "criteria": ""}),
        json!({"type": "score", "criteria": {}}),
    ] {
        assert!(SystemOneRequest::from_raw_json(json!("x"), json!({"q": raw})).is_err());
    }
    for (raw, kind, suffix) in [
        (json!({}), InputErrorKind::MissingField, "type"),
        (json!({"type": null}), InputErrorKind::InvalidType, "type"),
        (
            json!({"type": ""}),
            InputErrorKind::EmptyQuestionKind,
            "type",
        ),
        (
            json!({"type": "choice"}),
            InputErrorKind::MissingField,
            "criteria",
        ),
        (
            json!({"type": "score", "criteria": []}),
            InputErrorKind::EmptyScoreCriteria,
            "criteria",
        ),
    ] {
        let error = SystemOneRequest::from_raw_json(json!("x"), json!({"q/~": raw})).unwrap_err();
        let input = error.input_details().unwrap();
        assert_eq!(input.kind, kind);
        assert_eq!(input.path, format!("/questions/q~1~0/{suffix}"));
    }
    let error =
        SystemOneRequest::from_raw_json(json!("x"), json!({"private-key": "private-value"}))
            .unwrap_err();
    assert!(!format!("{error} {error:?}").contains("private-"));
    assert!(std::error::Error::source(&error).is_some());
}

#[tokio::test]
async fn preview_matches_execution_after_model_and_raw_overrides() {
    let (url, task) = server(response(200, "", r#"{"model":"m","usage":{}}"#)).await;
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
