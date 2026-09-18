//! Decode one field at a time so Serde enum buffering cannot erase input paths.
use crate::{Content, Error, Field, InputErrorKind, NoulCriteria, Question, SystemOneRequest};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub(crate) fn pointer(parent: &str, key: &str) -> String {
    format!("{parent}/{}", key.replace('~', "~0").replace('/', "~1"))
}

fn decode<T: DeserializeOwned>(value: Value, path: &str, kind: InputErrorKind) -> Result<T, Error> {
    serde_json::from_value(value)
        .map_err(|source| Error::request_input(kind, path).with_source(source))
}

fn content(value: Value, path: &str) -> Result<Content, Error> {
    decode(value, path, InputErrorKind::InvalidContent)
}

fn object(value: Value, path: &str) -> Result<Map<String, Value>, Error> {
    decode(value, path, InputErrorKind::InvalidType)
}

fn required(map: &mut Map<String, Value>, key: &str, path: &str) -> Result<Value, Error> {
    map.remove(key)
        .ok_or_else(|| Error::request_input(InputErrorKind::MissingField, pointer(path, key)))
}

fn known_fields(map: &Map<String, Value>, allowed: &[&str], path: &str) -> Result<(), Error> {
    if let Some(key) = map
        .keys()
        .filter(|key| !allowed.contains(&key.as_str()))
        .min()
    {
        return Err(Error::request_input(
            InputErrorKind::UnknownField,
            pointer(path, key),
        ));
    }
    Ok(())
}

fn field<T>(
    value: Option<Value>,
    path: &str,
    parse: impl FnOnce(Value, &str) -> Result<T, Error>,
) -> Result<Field<T>, Error> {
    match value {
        None => Ok(Field::Omitted),
        Some(Value::Null) => Ok(Field::Null),
        Some(value) => parse(value, path).map(Field::Value),
    }
}

fn noul_criteria(value: Value, path: &str) -> Result<NoulCriteria, Error> {
    let mut map = object(value, path)?;
    known_fields(&map, &["true", "false"], path)?;
    let r#true = field(map.remove("true"), &pointer(path, "true"), content)?;
    let r#false = field(map.remove("false"), &pointer(path, "false"), content)?;
    Ok(NoulCriteria { r#true, r#false })
}

fn question(value: Value, path: &str) -> Result<Question, Error> {
    let mut map = object(value, path)?;
    let kind: String = decode(
        required(&mut map, "type", path)?,
        &pointer(path, "type"),
        InputErrorKind::InvalidType,
    )?;
    if !matches!(kind.as_str(), "noul" | "choice" | "score") {
        return Err(Error::request_input(
            InputErrorKind::UnknownQuestionKind,
            pointer(path, "type"),
        ));
    }
    known_fields(&map, &["instructions", "criteria"], path)?;
    let instructions = field(
        map.remove("instructions"),
        &pointer(path, "instructions"),
        content,
    )?;
    let criteria_path = pointer(path, "criteria");
    match kind.as_str() {
        "noul" => Ok(Question::Noul {
            instructions,
            criteria: field(map.remove("criteria"), &criteria_path, noul_criteria)?,
        }),
        "choice" => {
            let entries: BTreeMap<_, _> =
                object(required(&mut map, "criteria", path)?, &criteria_path)?
                    .into_iter()
                    .collect();
            let criteria = entries
                .into_iter()
                .map(|(name, value)| {
                    let content = if value.is_null() {
                        None
                    } else {
                        Some(content(value, &pointer(&criteria_path, &name))?)
                    };
                    Ok((name, content))
                })
                .collect::<Result<_, Error>>()?;
            Ok(Question::Choice {
                instructions,
                criteria,
            })
        }
        // The discriminator was checked before inspecting any other fields.
        _ => {
            let values: Vec<Value> = decode(
                required(&mut map, "criteria", path)?,
                &criteria_path,
                InputErrorKind::InvalidType,
            )?;
            let criteria = values
                .into_iter()
                .enumerate()
                .map(|(index, value)| content(value, &pointer(&criteria_path, &index.to_string())))
                .collect::<Result<_, _>>()?;
            Ok(Question::Score {
                instructions,
                criteria,
            })
        }
    }
}

pub(crate) fn from_json(state: Value, questions: Value) -> Result<SystemOneRequest, Error> {
    let state = content(state, "/state")?;
    // Sorting here keeps local failure selection independent of preserve_order.
    let entries: BTreeMap<_, _> = object(questions, "/questions")?.into_iter().collect();
    let questions = entries
        .into_iter()
        .map(|(name, value)| {
            let question = question(value, &pointer("/questions", &name))?;
            validate_question(&name, &question)?;
            Ok((name, question))
        })
        .collect::<Result<_, Error>>()?;
    SystemOneRequest::new(state, questions)
}

pub(crate) fn validate_question(name: &str, question: &Question) -> Result<(), Error> {
    if matches!(question, Question::Score { criteria, .. } if criteria.is_empty()) {
        return Err(Error::request_input(
            InputErrorKind::EmptyScoreCriteria,
            pointer(&pointer("/questions", name), "criteria"),
        ));
    }
    Ok(())
}
