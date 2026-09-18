use crate::Error;
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
            Self::Value(v) => v.serialize(serializer),
            _ => serializer.serialize_none(),
        }
    }
}
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Field<T> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Option::<T>::deserialize(d)?.map_or(Self::Null, Self::Value))
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

/// Known question types. Unknown/raw question extensions are a later milestone.
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

/// A validated request. Use options for per-call transport overrides.
#[derive(Clone, Debug)]
pub struct SystemOneRequest {
    pub(crate) state: Content,
    pub(crate) questions: BTreeMap<String, Question>,
    pub model: Option<String>,
    /// Shallow last-write-wins overrides, including null and reserved body fields.
    pub extra_body: Map<String, Value>,
}
impl SystemOneRequest {
    pub fn new(state: Content, questions: BTreeMap<String, Question>) -> Result<Self, Error> {
        if questions.is_empty() {
            return Err(Error::input("At least one question is required."));
        }
        for (name, q) in &questions {
            if matches!(q, Question::Score { criteria, .. } if criteria.is_empty()) {
                return Err(Error::input(format!(
                    "Score question {name:?} has no criteria; at least one score is required."
                )));
            }
        }
        Ok(Self {
            state,
            questions,
            model: None,
            extra_body: Map::new(),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Answer {
    Noul(NoulAnswer),
    Choice(ChoiceAnswer),
    Score(ScoreAnswer),
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NoulAnswer {
    pub noul: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ChoiceAnswer {
    pub choice: String,
    pub confidence: f64,
    pub probabilities: BTreeMap<String, f64>,
}
/// Score maps use signed 64-bit integer keys. Decimal or whitespace key spellings
/// accepted by Python (such as `"0.0"`) are not supported in this milestone.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ScoreAnswer {
    pub score: f64,
    pub confidence: f64,
    #[serde(deserialize_with = "integer_keys")]
    pub legend: BTreeMap<i64, Content>,
    #[serde(deserialize_with = "integer_keys")]
    pub probabilities: BTreeMap<i64, f64>,
}
fn integer_keys<'de, D, T>(deserializer: D) -> Result<BTreeMap<i64, T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    let strings = BTreeMap::<String, T>::deserialize(deserializer)?;
    strings
        .into_iter()
        .map(|(key, value)| {
            key.parse()
                .map(|key| (key, value))
                .map_err(serde::de::Error::custom)
        })
        .collect()
}
/// Missing/null counts become `None`; integer counts must fit in `i64`.
/// The Python reference also supports integers outside this range.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Usage {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
}
/// Known answers decoded from a standard JSON response. Nonstandard NaN/Infinity
/// literals are rejected. HTTP decoding silently omits future answer kinds from
/// this map and retains them in `Response::metadata.body` (no logging yet).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SystemOneResponse {
    pub model: String,
    pub usage: Usage,
    #[serde(default)]
    pub answers: BTreeMap<String, Answer>,
}
impl SystemOneResponse {
    pub fn choices(&self) -> impl Iterator<Item = (&str, &ChoiceAnswer)> {
        self.answers.iter().filter_map(|(name, a)| match a {
            Answer::Choice(answer) => Some((name.as_str(), answer)),
            _ => None,
        })
    }
    pub fn scores(&self) -> impl Iterator<Item = (&str, &ScoreAnswer)> {
        self.answers.iter().filter_map(|(name, a)| match a {
            Answer::Score(answer) => Some((name.as_str(), answer)),
            _ => None,
        })
    }
    pub fn nouls(&self) -> impl Iterator<Item = (&str, f64)> {
        self.answers.iter().filter_map(|(name, a)| match a {
            Answer::Noul(answer) => Some((name.as_str(), answer.noul)),
            _ => None,
        })
    }
}
