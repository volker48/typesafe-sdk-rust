//! THROWAWAY: compare construction interfaces without credentials or HTTP.
//! Run: cargo run --locked --example prototype_request
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use typesafe_sdk::{Content, Field, NoulCriteria, Question, SystemOneRequest};

type Failure = Box<dyn std::error::Error>;

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, Failure> {
    Ok(serde_path_to_error::deserialize(value)?)
}

// Proposed facade, using only today's public SDK. Production errors need a
// structured, redacted local diagnostic; this experiment prints fixture errors.
fn from_json(state: Value, questions: Value) -> Result<SystemOneRequest, Failure> {
    Ok(SystemOneRequest::new(decode(state)?, decode(questions)?)?)
}

fn from_pairs<K: Into<String>>(
    state: impl Into<Content>,
    questions: impl IntoIterator<Item = (K, Question)>,
) -> Result<SystemOneRequest, Failure> {
    let mut map = BTreeMap::new();
    for (key, question) in questions {
        if map.insert(key.into(), question).is_some() {
            return Err("Duplicate question name".into());
        }
    }
    Ok(SystemOneRequest::new(state.into(), map)?)
}

// Intentionally small syntax sugar: values are ordinary Rust expressions.
macro_rules! questions {
    ($($key:literal : $value:expr),* $(,)?) => {
        [$(($key, $value)),*]
    };
}

fn choice<const N: usize>(instructions: &str, criteria: [(&str, &str); N]) -> Question {
    Question::Choice {
        instructions: Field::Value(instructions.into()),
        criteria: criteria
            .into_iter()
            .map(|(k, v)| (k.into(), Some(v.into())))
            .collect(),
    }
}

fn score<const N: usize>(instructions: &str, criteria: [&str; N]) -> Question {
    Question::Score {
        instructions: Field::Value(instructions.into()),
        criteria: criteria.into_iter().map(Content::from).collect(),
    }
}

#[derive(Serialize)]
struct Observation {
    name: &'static str,
    state: Value,
    questions: Value,
    accepted: bool,
    diagnostic: Option<String>,
    serialized_questions: Option<Value>,
}

fn observe(name: &'static str, state: Value, questions: Value) -> Observation {
    let result = from_json(state.clone(), questions.clone());
    let serialized_questions = if result.is_ok() {
        decode::<BTreeMap<String, Question>>(questions.clone())
            .and_then(|v| Ok(serde_json::to_value(v)?))
            .ok()
    } else {
        None
    };
    Observation {
        name,
        state,
        questions,
        accepted: result.is_ok(),
        diagnostic: result.err().map(|e| e.to_string()),
        serialized_questions,
    }
}

fn main() -> Result<(), Failure> {
    let state = "Help! My payment has failed three times today and I need it fixed before payroll.";
    let questions = json!({
        "is_urgent": {
            "type": "noul",
            "instructions": "Does this support request require a fast response?",
            "criteria": {
                "true": "The customer reports a blocked payment or a time-sensitive deadline.",
                "false": "The request is informational or has no stated deadline."
            }
        },
        "team": {
            "type": "choice",
            "instructions": "Which team should handle this request?",
            "criteria": {
                "billing": "Payments, invoices, refunds, or charges.",
                "technical": "Bugs, outages, or integration problems.",
                "account": "Login, profile, or account-access problems."
            }
        },
        "customer_sentiment": {
            "type": "score",
            "instructions": "How frustrated is the customer?",
            "criteria": ["Calm or neutral", "Frustrated", "Very angry or threatening to leave"]
        }
    });
    let typed = BTreeMap::from([
        (
            "is_urgent".into(),
            Question::Noul {
                instructions: Field::Value(
                    "Does this support request require a fast response?".into(),
                ),
                criteria: Field::Value(NoulCriteria {
                    r#true: Field::Value(
                        "The customer reports a blocked payment or a time-sensitive deadline."
                            .into(),
                    ),
                    r#false: Field::Value(
                        "The request is informational or has no stated deadline.".into(),
                    ),
                }),
            },
        ),
        (
            "team".into(),
            choice(
                "Which team should handle this request?",
                [
                    ("billing", "Payments, invoices, refunds, or charges."),
                    ("technical", "Bugs, outages, or integration problems."),
                    ("account", "Login, profile, or account-access problems."),
                ],
            ),
        ),
        (
            "customer_sentiment".into(),
            score(
                "How frustrated is the customer?",
                [
                    "Calm or neutral",
                    "Frustrated",
                    "Very angry or threatening to leave",
                ],
            ),
        ),
    ]);
    let typed_serialized = serde_json::to_value(&typed)?;
    let typed_request = SystemOneRequest::new(state.into(), typed.clone())?;
    let json_request = from_json(json!(state), questions.clone())?;
    let pairs_request = from_pairs(state, typed)?;
    let macro_request = from_pairs(
        state,
        questions! {
            "urgent": Question::noul("Is this urgent?"),
        },
    )?;
    let duplicate_pairs = from_pairs(
        "x",
        [
            ("q", Question::noul("First")),
            ("q", Question::noul("Second")),
        ],
    );
    let dynamic_name = "dynamic_question";
    let mut observations = vec![observe("smoke", json!(state), questions)];
    for (name, state, questions) in [
        (
            "structured",
            json!({"message": state, "attempts": 3, "account": null}),
            json!({dynamic_name: {"type": "noul"}}),
        ),
        ("omitted", json!("x"), json!({"q": {"type": "noul"}})),
        (
            "null",
            json!("x"),
            json!({"q": {"type": "noul", "instructions": null, "criteria": null}}),
        ),
        (
            "choice_null",
            json!("x"),
            json!({"q": {"type": "choice", "criteria": {"billing": null}}}),
        ),
        ("empty_questions", json!("x"), json!({})),
        (
            "empty_score",
            json!("x"),
            json!({"q": {"type": "score", "criteria": []}}),
        ),
        (
            "one_score",
            json!("x"),
            json!({"q": {"type": "score", "criteria": ["Only level"]}}),
        ),
        (
            "empty_choice",
            json!("x"),
            json!({"q": {"type": "choice", "criteria": {}}}),
        ),
        ("scalar_state", json!(42), json!({"q": {"type": "noul"}})),
        (
            "typo",
            json!("x"),
            json!({"q": {"type": "noul", "instruction": "Typo"}}),
        ),
        (
            "bad_content",
            json!("x"),
            json!({"q": {"type": "choice", "criteria": {"billing": 42}}}),
        ),
        ("future", json!("x"), json!({"q": {"type": "future"}})),
        (
            "duplicate_json",
            json!("x"),
            json!({"q": {"type": "noul", "instructions": "First"}, "q": {"type": "noul", "instructions": "Second"}}),
        ),
    ] {
        observations.push(observe(name, state, questions));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "baseline": "fd8d1a0",
            "scope": "Offline fixture experiment, not a shipped API or wire replay",
            "smoke_questions_equal": typed_serialized == observations[0].serialized_questions.clone().unwrap(),
            "typed_and_json_request_debug_equal": format!("{typed_request:?}") == format!("{json_request:?}"),
            "typed_and_pairs_request_debug_equal": format!("{typed_request:?}") == format!("{pairs_request:?}"),
            "macro_request_constructed": format!("{macro_request:?}"),
            "duplicate_pairs_diagnostic": duplicate_pairs.err().map(|e| e.to_string()),
            "limitations": ["Macro only explores literal keys; hygiene and dynamic expressions unverified", "Typed choice helper does not reject duplicate labels", "Debug equality is exploratory evidence, not a production test", "Serde enum buffering loses nested error location"],
            "observations": observations,
        }))?
    );
    Ok(())
}
