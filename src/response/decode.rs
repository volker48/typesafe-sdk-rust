//! Response validation follows schema field order and visits maps in wire order,
//! so the first reported failure matches the Python SDK.
use super::{
    Answer, ChoiceAnswer, ListModelsResponse, ModelMetadata, NoulAnswer, ScoreAnswer,
    SystemOneResponse, Usage, score_key,
};
use crate::{Content, error::BoxError};
use serde::de::{DeserializeOwned, Error as _};
use serde_json::Value;
use std::collections::BTreeMap;

/// Decodes a successful response body.
pub(crate) type Decoder<T> = fn(&[u8]) -> Result<T, Failure>;

/// Why a response body failed validation.
pub(crate) struct Failure {
    /// Dotted field path; empty for the root and for malformed JSON.
    pub(crate) path: String,
    pub(crate) source: BoxError,
}

impl Failure {
    fn new(path: impl Into<String>, source: impl Into<BoxError>) -> Self {
        Self {
            path: path.into(),
            source: source.into(),
        }
    }

    fn root(source: impl Into<BoxError>) -> Self {
        Self::new(String::new(), source)
    }

    fn invalid(path: &str, message: &str) -> Self {
        Self::new(path, serde_json::Error::custom(message))
    }
}

/// Decode the whole body with the caller's Serde contract.
pub(crate) fn custom<T: DeserializeOwned>(body: &[u8]) -> Result<T, Failure> {
    let mut deserializer = serde_json::Deserializer::from_slice(body);
    let data = serde_path_to_error::deserialize(&mut deserializer).map_err(|error| {
        let path = if error.inner().is_syntax()
            || error.inner().is_eof()
            || error.path().iter().next().is_none()
        {
            String::new()
        } else {
            error.path().to_string()
        };
        Failure::new(path, error)
    })?;
    // Deserialize alone accepts a valid prefix; the HTTP body must be one JSON value.
    deserializer.end().map_err(Failure::root)?;
    Ok(data)
}

pub(crate) fn system_one(body: &[u8]) -> Result<SystemOneResponse, Failure> {
    let value = object(body)?;
    // Python checks every discriminator before validating any envelope/answer field.
    if let Some(Value::Object(answers)) = value.get("answers") {
        for (name, answer) in answers {
            if answer.get("type").and_then(Value::as_str).is_none() {
                return Err(Failure::invalid(
                    &format!("answers.{name}.type"),
                    "Expected an answer type",
                ));
            }
        }
    }
    let model = parse(member(&value, "model", "model")?, "model")?;
    let usage = member(&value, "usage", "usage")?;
    if !usage.is_object() {
        return Err(Failure::invalid("usage", "Expected a usage object"));
    }
    let usage = Usage {
        input_tokens: optional(usage, "input_tokens", "usage")?,
        output_tokens: optional(usage, "output_tokens", "usage")?,
    };
    let mut answers = BTreeMap::new();
    if let Some(raw_answers) = value.get("answers") {
        let raw_answers = raw_answers
            .as_object()
            .ok_or_else(|| Failure::invalid("answers", "Expected an answers object"))?;
        for (name, raw) in raw_answers {
            if let Some(answer) = answer(raw, &format!("answers.{name}"))? {
                answers.insert(name.clone(), answer);
            }
        }
    }
    Ok(SystemOneResponse {
        model,
        usage,
        answers,
    })
}

/// Decode one answer. Unknown kinds yield `None`; they stay in the raw body.
fn answer(raw: &Value, path: &str) -> Result<Option<Answer>, Failure> {
    let answer = match raw["type"].as_str() {
        Some("noul") => Answer::Noul(NoulAnswer {
            noul: required(raw, "noul", path)?,
        }),
        Some("choice") => Answer::Choice(ChoiceAnswer {
            choice: required(raw, "choice", path)?,
            confidence: required(raw, "confidence", path)?,
            probabilities: required(raw, "probabilities", path)?,
        }),
        Some("score") => Answer::Score(ScoreAnswer {
            score: required(raw, "score", path)?,
            confidence: required(raw, "confidence", path)?,
            legend: score_map::<Content>(raw, "legend", path, true)?,
            probabilities: score_map(raw, "probabilities", path, false)?,
        }),
        _ => return Ok(None),
    };
    Ok(Some(answer))
}

/// Validate in schema order, retaining array indices even for missing fields.
pub(crate) fn list_models(body: &[u8]) -> Result<ListModelsResponse, Failure> {
    let value = object(body)?;
    let entries = member(&value, "models", "models")?
        .as_array()
        .ok_or_else(|| Failure::invalid("models", "Expected a models array"))?;
    let models = entries
        .iter()
        .enumerate()
        .map(|(index, model)| {
            let path = format!("models[{index}]");
            if !model.is_object() {
                return Err(Failure::invalid(&path, "Expected a model object"));
            }
            Ok(ModelMetadata {
                name: required(model, "name", &path)?,
                description: required(model, "description", &path)?,
                release_date: required(model, "release_date", &path)?,
            })
        })
        .collect::<Result<_, _>>()?;
    Ok(ListModelsResponse { models })
}

/// Parse a complete body that must be a JSON object.
fn object(body: &[u8]) -> Result<Value, Failure> {
    let value: Value = serde_json::from_slice(body).map_err(Failure::root)?;
    if !value.is_object() {
        return Err(Failure::invalid("", "Expected a response object"));
    }
    Ok(value)
}

/// Deserialize `value`, reporting nested failures relative to `path`.
fn parse<T: DeserializeOwned>(value: &Value, path: &str) -> Result<T, Failure> {
    serde_path_to_error::deserialize(value).map_err(|error| {
        let path = if error.path().iter().next().is_none() {
            path.to_owned()
        } else {
            format!("{path}.{}", error.path())
        };
        Failure::new(path, error)
    })
}

/// A required member of `object`; `path` locates it when missing.
fn member<'a>(object: &'a Value, name: &str, path: &str) -> Result<&'a Value, Failure> {
    object
        .get(name)
        .ok_or_else(|| Failure::invalid(path, "Missing required field"))
}

/// Parse the required member `name` of the object at `parent`.
fn required<T: DeserializeOwned>(object: &Value, name: &str, parent: &str) -> Result<T, Failure> {
    let path = format!("{parent}.{name}");
    parse(member(object, name, &path)?, &path)
}

/// Parse the member `name` of the object at `parent`, treating absence as null.
fn optional<T: DeserializeOwned>(object: &Value, name: &str, parent: &str) -> Result<T, Failure> {
    parse(
        object.get(name).unwrap_or(&Value::Null),
        &format!("{parent}.{name}"),
    )
}

/// Parse the required score map `name`, whose keys are exact integer spellings.
fn score_map<T: DeserializeOwned>(
    answer: &Value,
    name: &str,
    parent: &str,
    content_values: bool,
) -> Result<BTreeMap<i64, T>, Failure> {
    let path = format!("{parent}.{name}");
    let entries = member(answer, name, &path)?
        .as_object()
        .ok_or_else(|| Failure::invalid(&path, "Expected an object"))?;
    let mut result = BTreeMap::new();
    for (raw_key, value) in entries {
        let item_path = format!("{path}.{raw_key}");
        let key = score_key(raw_key).map_err(|message| Failure::invalid(&item_path, message))?;
        // Pydantic reports the first branch of its content union for scalar values.
        let value_path = if content_values
            && !matches!(value, Value::String(_) | Value::Object(_) | Value::Array(_))
        {
            format!("{item_path}.str")
        } else {
            item_path
        };
        result.insert(key, parse(value, &value_path)?);
    }
    Ok(result)
}
