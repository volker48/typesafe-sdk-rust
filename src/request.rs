//! System One request data: validated state and questions.
mod json;

use crate::{Error, InputErrorKind};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Text or structured JSON. Scalar numbers, booleans and null are not content.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Object(Map<String, Value>),
    Array(Vec<Value>),
}

impl From<&str> for Content {
    fn from(value: &str) -> Self {
        Self::Text(value.into())
    }
}

/// Wire presence, including the explicit null supported by Python raw questions.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Field<T> {
    #[default]
    Omitted,
    Null,
    Value(T),
}

impl<T> Field<T> {
    fn is_omitted(&self) -> bool {
        matches!(self, Self::Omitted)
    }
}

impl<T: Serialize> Serialize for Field<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Value(value) => value.serialize(serializer),
            _ => serializer.serialize_none(),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Field<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Option::<T>::deserialize(deserializer)?.map_or(Self::Null, Self::Value))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NoulCriteria {
    #[serde(default, skip_serializing_if = "Field::is_omitted")]
    pub r#true: Field<Content>,
    #[serde(default, skip_serializing_if = "Field::is_omitted")]
    pub r#false: Field<Content>,
}

/// Known question types. For extensions use [`SystemOneRequest::from_raw_json`].
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Question {
    Noul {
        #[serde(default, skip_serializing_if = "Field::is_omitted")]
        instructions: Field<Content>,
        #[serde(default, skip_serializing_if = "Field::is_omitted")]
        criteria: Field<NoulCriteria>,
    },
    Choice {
        #[serde(default, skip_serializing_if = "Field::is_omitted")]
        instructions: Field<Content>,
        criteria: BTreeMap<String, Option<Content>>,
    },
    Score {
        #[serde(default, skip_serializing_if = "Field::is_omitted")]
        instructions: Field<Content>,
        criteria: Vec<Content>,
    },
}

impl Question {
    pub fn noul(instructions: impl Into<Content>) -> Self {
        Self::Noul {
            instructions: Field::Value(instructions.into()),
            criteria: Field::Omitted,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum Questions {
    Typed(BTreeMap<String, Question>),
    Raw(BTreeMap<String, Map<String, Value>>),
}

/// A request with validated state and either typed or explicitly raw questions.
/// Use options for per-call transport overrides.
#[derive(Clone, Debug)]
pub struct SystemOneRequest {
    pub(crate) state: Content,
    pub(crate) questions: Questions,
    pub model: Option<String>,
    /// Shallow last-write-wins overrides, including null and reserved body fields.
    pub extra_body: Map<String, Value>,
}

impl SystemOneRequest {
    /// Decode JSON into typed state and questions, validating before HTTP.
    ///
    /// State accepts text, objects, and arrays. Known question kinds and fields
    /// are checked strictly. Missing fields and explicit null remain distinct;
    /// score criteria retain their order and must contain at least one element.
    ///
    /// Failures have [`crate::ErrorKind::Input`] and [`Error::input_details`].
    /// Selection order is state, then questions sorted by name. Within a question:
    /// object shape, type, unknown fields (sorted), instructions, then criteria.
    /// Noul criteria check unknown fields, true, then false; choice labels sort
    /// by name and score levels follow array order. Each question's semantic
    /// checks run before decoding the next question.
    ///
    /// `json!` has already collapsed duplicate keys to the last value and encodes
    /// interpolated `None` as null. Omit a key to omit a field. For custom fallible
    /// serialization use `serde_json::to_value(data)?` rather than interpolating
    /// it in `json!`, which can panic on serialization failure. Neither path
    /// preserves nonfinite floats: check them before they become JSON null.
    ///
    /// ```
    /// use serde_json::json;
    /// use typesafe_sdk::SystemOneRequest;
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let state = serde_json::to_value(["Payment failed", "Deadline today"])?;
    /// let request = SystemOneRequest::from_json(state, json!({
    ///     "urgent": {"type": "noul", "instructions": "Is this urgent?"},
    ///     "team": {"type": "choice", "instructions": null,
    ///              "criteria": {"billing": "Payments", "other": null}}
    /// }))?;
    /// # Ok(()) }
    /// # example().unwrap();
    /// ```
    pub fn from_json(state: Value, questions: Value) -> Result<Self, Error> {
        json::from_json(state, questions)
    }

    /// Construct a request with question objects passed through to the API.
    ///
    /// State must be text, an object or an array. Questions must be a nonempty
    /// object whose values are objects with nonempty string `type` fields.
    /// Choice and score require `criteria`; score rejects null, false, zero and
    /// empty strings/arrays/objects, matching Python's raw-question checks.
    /// Other field contents are unchecked: future kinds, extra fields and nulls
    /// are preserved. The API owns their remaining schema validation.
    /// [`Self::from_json`] remains strict.
    /// Local errors use the same input reasons and JSON Pointers as `from_json`,
    /// checking state first and then question objects in sorted name order.
    /// Model inheritance and final `extra_body` overrides work as usual.
    ///
    /// ```
    /// use serde_json::json;
    /// use typesafe_sdk::{Question, SystemOneRequest};
    /// let request = SystemOneRequest::from_raw_json(json!("Evidence"), json!({
    ///     "known": Question::noul("Is it urgent?"),
    ///     "extended": {"type": "noul", "weight": 2, "instructions": null},
    ///     "future": {"type": "future", "criteria": {"nested": [null, 1]}}
    /// }))?;
    /// # Ok::<(), typesafe_sdk::Error>(())
    /// ```
    pub fn from_raw_json(state: Value, questions: Value) -> Result<Self, Error> {
        json::from_raw_json(state, questions)
    }

    /// Validate typed questions. Rejects an empty map or empty score criteria.
    /// Request input diagnostics use the same paths/reasons as [`Self::from_json`].
    pub fn new(state: Content, questions: BTreeMap<String, Question>) -> Result<Self, Error> {
        if questions.is_empty() {
            return Err(Error::request_input(
                InputErrorKind::EmptyQuestions,
                QUESTIONS,
            ));
        }
        for (name, question) in &questions {
            validate_question(name, question)?;
        }
        Ok(Self::from_parts(state, Questions::Typed(questions)))
    }

    fn from_parts(state: Content, questions: Questions) -> Self {
        Self {
            state,
            questions,
            model: None,
            extra_body: Map::new(),
        }
    }
}

/// JSON Pointer to the questions object in the conceptual `{state, questions}` input.
const QUESTIONS: &str = "/questions";

/// Append `key` to a JSON Pointer, escaping it per RFC 6901.
fn pointer(parent: &str, key: &str) -> String {
    format!("{parent}/{}", key.replace('~', "~0").replace('/', "~1"))
}

/// Semantic checks shared by typed and JSON construction.
fn validate_question(name: &str, question: &Question) -> Result<(), Error> {
    if matches!(question, Question::Score { criteria, .. } if criteria.is_empty()) {
        return Err(Error::request_input(
            InputErrorKind::EmptyScoreCriteria,
            pointer(&pointer(QUESTIONS, name), "criteria"),
        ));
    }
    Ok(())
}
