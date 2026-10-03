//! Decode one field at a time so Serde enum buffering cannot erase input paths.
use super::{
    Content, Field, NoulCriteria, QUESTIONS, Question, Questions, SystemOneRequest, pointer,
    validate_question,
};
use crate::{Error, InputErrorKind};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub(super) fn from_json(state: Value, questions: Value) -> Result<SystemOneRequest, Error> {
    let state = content(state, "/state")?;
    let questions = sorted_questions(questions)?
        .into_iter()
        .map(|(name, value)| {
            let question = question(value, &pointer(QUESTIONS, &name))?;
            validate_question(&name, &question)?;
            Ok((name, question))
        })
        .collect::<Result<_, Error>>()?;
    SystemOneRequest::new(state, questions)
}

pub(super) fn from_raw_json(state: Value, questions: Value) -> Result<SystemOneRequest, Error> {
    let state = content(state, "/state")?;
    let questions = sorted_questions(questions)?;
    if questions.is_empty() {
        return Err(Error::request_input(
            InputErrorKind::EmptyQuestions,
            QUESTIONS,
        ));
    }
    let questions = questions
        .into_iter()
        .map(|(name, value)| {
            let question = raw_question(value, &pointer(QUESTIONS, &name))?;
            Ok((name, question))
        })
        .collect::<Result<_, Error>>()?;
    Ok(SystemOneRequest::from_parts(
        state,
        Questions::Raw(questions),
    ))
}

/// Sorting keeps local failure selection independent of `preserve_order`.
fn sorted_questions(value: Value) -> Result<BTreeMap<String, Value>, Error> {
    Ok(object(value, QUESTIONS)?.into_iter().collect())
}

enum QuestionKind {
    Noul,
    Choice,
    Score,
}

fn question(value: Value, path: &str) -> Result<Question, Error> {
    let mut map = object(value, path)?;
    let type_path = pointer(path, "type");
    let kind: String = decode(
        required(&mut map, "type", path)?,
        &type_path,
        InputErrorKind::InvalidType,
    )?;
    // The discriminator is checked before inspecting any other fields.
    let kind = match kind.as_str() {
        "noul" => QuestionKind::Noul,
        "choice" => QuestionKind::Choice,
        "score" => QuestionKind::Score,
        _ => {
            return Err(Error::request_input(
                InputErrorKind::UnknownQuestionKind,
                type_path,
            ));
        }
    };
    known_fields(&map, &["instructions", "criteria"], path)?;
    let instructions = field(
        map.remove("instructions"),
        &pointer(path, "instructions"),
        content,
    )?;
    let criteria_path = pointer(path, "criteria");
    Ok(match kind {
        QuestionKind::Noul => Question::Noul {
            instructions,
            criteria: field(map.remove("criteria"), &criteria_path, noul_criteria)?,
        },
        QuestionKind::Choice => Question::Choice {
            instructions,
            criteria: choice_criteria(required(&mut map, "criteria", path)?, &criteria_path)?,
        },
        QuestionKind::Score => Question::Score {
            instructions,
            criteria: score_criteria(required(&mut map, "criteria", path)?, &criteria_path)?,
        },
    })
}

fn noul_criteria(value: Value, path: &str) -> Result<NoulCriteria, Error> {
    let mut map = object(value, path)?;
    known_fields(&map, &["true", "false"], path)?;
    let r#true = field(map.remove("true"), &pointer(path, "true"), content)?;
    let r#false = field(map.remove("false"), &pointer(path, "false"), content)?;
    Ok(NoulCriteria { r#true, r#false })
}

/// Labels are checked in name order; each maps to content or null.
fn choice_criteria(value: Value, path: &str) -> Result<BTreeMap<String, Option<Content>>, Error> {
    let labels: BTreeMap<_, _> = object(value, path)?.into_iter().collect();
    labels
        .into_iter()
        .map(|(label, value)| {
            let content = match value {
                Value::Null => None,
                value => Some(content(value, &pointer(path, &label))?),
            };
            Ok((label, content))
        })
        .collect()
}

/// Levels are checked in array order, which is also their meaning.
fn score_criteria(value: Value, path: &str) -> Result<Vec<Content>, Error> {
    let levels: Vec<Value> = decode(value, path, InputErrorKind::InvalidType)?;
    levels
        .into_iter()
        .enumerate()
        .map(|(index, level)| content(level, &pointer(path, &index.to_string())))
        .collect()
}

/// Apply only Python's raw-question checks; the API validates everything else.
fn raw_question(value: Value, path: &str) -> Result<Map<String, Value>, Error> {
    let question = object(value, path)?;
    let type_path = pointer(path, "type");
    let kind = question
        .get("type")
        .ok_or_else(|| Error::request_input(InputErrorKind::MissingField, &type_path))?;
    let kind: String = decode(kind.clone(), &type_path, InputErrorKind::InvalidType)?;
    if kind.is_empty() {
        return Err(Error::request_input(
            InputErrorKind::EmptyQuestionKind,
            type_path,
        ));
    }
    if matches!(kind.as_str(), "choice" | "score") {
        let criteria_path = pointer(path, "criteria");
        let criteria = question
            .get("criteria")
            .ok_or_else(|| Error::request_input(InputErrorKind::MissingField, &criteria_path))?;
        if kind == "score" && is_falsy(criteria) {
            return Err(Error::request_input(
                InputErrorKind::EmptyScoreCriteria,
                criteria_path,
            ));
        }
    }
    Ok(question)
}

/// Python truthiness, which its raw score questions apply to `criteria`.
fn is_falsy(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(value) => !value,
        Value::Number(value) => value.as_f64() == Some(0.0),
        Value::String(value) => value.is_empty(),
        Value::Array(value) => value.is_empty(),
        Value::Object(value) => value.is_empty(),
    }
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

/// Report the first unknown key in sorted order.
fn known_fields(map: &Map<String, Value>, allowed: &[&str], path: &str) -> Result<(), Error> {
    match map
        .keys()
        .filter(|key| !allowed.contains(&key.as_str()))
        .min()
    {
        Some(key) => Err(Error::request_input(
            InputErrorKind::UnknownField,
            pointer(path, key),
        )),
        None => Ok(()),
    }
}

/// Decode an optional member, keeping omission and explicit null distinct.
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
