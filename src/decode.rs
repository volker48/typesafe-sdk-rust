//! Response validation follows schema field order and visits maps in wire order.
use crate::{Answer, ChoiceAnswer, Content, NoulAnswer, ScoreAnswer, SystemOneResponse, Usage};
use serde::de::{DeserializeOwned, Error as _};
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) type Failure = (String, Option<Box<dyn std::error::Error + Send + Sync>>);

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
        (path, Some(Box::new(error) as _))
    })?;
    // Deserialize alone accepts a valid prefix; the HTTP body must be one JSON value.
    deserializer
        .end()
        .map_err(|error| (String::new(), Some(Box::new(error) as _)))?;
    Ok(data)
}

fn invalid(path: &str, message: &str) -> Failure {
    (
        path.into(),
        Some(Box::new(serde_json::Error::custom(message))),
    )
}

fn parse<T: DeserializeOwned>(value: &Value, path: &str) -> Result<T, Failure> {
    serde_path_to_error::deserialize(value).map_err(|error| {
        let nested = error.path().to_string();
        let path = if error.path().iter().next().is_none() {
            path.into()
        } else {
            format!("{path}.{nested}")
        };
        (path, Some(Box::new(error) as _))
    })
}

fn field<'a>(value: &'a Value, name: &str, path: &str) -> Result<&'a Value, Failure> {
    value
        .get(name)
        .ok_or_else(|| invalid(path, "Missing required field"))
}

fn score_map<T: DeserializeOwned>(
    value: &Value,
    path: &str,
    content: bool,
) -> Result<BTreeMap<i64, T>, Failure> {
    let entries = value
        .as_object()
        .ok_or_else(|| invalid(path, "Expected an object"))?;
    let mut result = BTreeMap::new();
    for (raw_key, value) in entries {
        let item_path = format!("{path}.{raw_key}");
        let key =
            crate::models::score_key(raw_key).map_err(|message| invalid(&item_path, message))?;
        // Pydantic reports the first branch of its content union for scalar values.
        let value_path =
            if content && !matches!(value, Value::String(_) | Value::Object(_) | Value::Array(_)) {
                format!("{item_path}.str")
            } else {
                item_path
            };
        result.insert(key, parse(value, &value_path)?);
    }
    Ok(result)
}

pub(crate) fn system_one(body: &[u8]) -> Result<SystemOneResponse, Failure> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|error| (String::new(), Some(Box::new(error) as _)))?;
    if !value.is_object() {
        return Err(invalid("", "Expected a response object"));
    }
    // Python checks every discriminator before validating any envelope/answer field.
    if let Some(Value::Object(answers)) = value.get("answers") {
        for (name, raw) in answers {
            if raw.get("type").and_then(Value::as_str).is_none() {
                return Err(invalid(
                    &format!("answers.{name}.type"),
                    "Expected an answer type",
                ));
            }
        }
    }
    let model = parse(field(&value, "model", "model")?, "model")?;
    let usage = field(&value, "usage", "usage")?;
    if !usage.is_object() {
        return Err(invalid("usage", "Expected a usage object"));
    }
    let usage = Usage {
        input_tokens: parse(
            usage.get("input_tokens").unwrap_or(&Value::Null),
            "usage.input_tokens",
        )?,
        output_tokens: parse(
            usage.get("output_tokens").unwrap_or(&Value::Null),
            "usage.output_tokens",
        )?,
    };
    let mut answers = BTreeMap::new();
    if let Some(raw_answers) = value.get("answers") {
        let raw_answers = raw_answers
            .as_object()
            .ok_or_else(|| invalid("answers", "Expected an answers object"))?;
        for (name, raw) in raw_answers {
            let path = format!("answers.{name}");
            let get = |name: &str| field(raw, name, &format!("{path}.{name}"));
            let answer = match raw["type"].as_str() {
                Some("noul") => Answer::Noul(NoulAnswer {
                    noul: parse(get("noul")?, &format!("{path}.noul"))?,
                }),
                Some("choice") => Answer::Choice(ChoiceAnswer {
                    choice: parse(get("choice")?, &format!("{path}.choice"))?,
                    confidence: parse(get("confidence")?, &format!("{path}.confidence"))?,
                    probabilities: parse(get("probabilities")?, &format!("{path}.probabilities"))?,
                }),
                Some("score") => Answer::Score(ScoreAnswer {
                    score: parse(get("score")?, &format!("{path}.score"))?,
                    confidence: parse(get("confidence")?, &format!("{path}.confidence"))?,
                    legend: score_map::<Content>(get("legend")?, &format!("{path}.legend"), true)?,
                    probabilities: score_map(
                        get("probabilities")?,
                        &format!("{path}.probabilities"),
                        false,
                    )?,
                }),
                _ => continue,
            };
            answers.insert(name.clone(), answer);
        }
    }
    Ok(SystemOneResponse {
        model,
        usage,
        answers,
    })
}

/// Validate in schema order, retaining array indices even for missing fields.
pub(crate) fn list_models(body: &[u8]) -> Result<crate::ListModelsResponse, Failure> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|error| (String::new(), Some(Box::new(error) as _)))?;
    if !value.is_object() {
        return Err(invalid("", "Expected a response object"));
    }
    let entries = field(&value, "models", "models")?
        .as_array()
        .ok_or_else(|| invalid("models", "Expected a models array"))?;
    let mut models = Vec::with_capacity(entries.len());
    for (index, value) in entries.iter().enumerate() {
        let path = format!("models[{index}]");
        if !value.is_object() {
            return Err(invalid(&path, "Expected a model object"));
        }
        let get = |name: &str| {
            let path = format!("{path}.{name}");
            parse(field(value, name, &path)?, &path)
        };
        models.push(crate::ModelMetadata {
            name: get("name")?,
            description: get("description")?,
            release_date: get("release_date")?,
        });
    }
    Ok(crate::ListModelsResponse { models })
}
