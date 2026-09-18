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
        crate::request::from_json(state, questions)
    }

    /// Validate typed questions. Rejects an empty map or empty score criteria.
    /// Request input diagnostics use the same paths/reasons as [`Self::from_json`].
    pub fn new(state: Content, questions: BTreeMap<String, Question>) -> Result<Self, Error> {
        if questions.is_empty() {
            return Err(Error::request_input(
                InputErrorKind::EmptyQuestions,
                "/questions",
            ));
        }
        for (name, q) in &questions {
            crate::request::validate_question(name, q)?;
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
/// Score maps use signed 64-bit integer keys. Whitespace, signs, underscores
/// between integer digits and zero-only fractional parts (`"1.00"`) are accepted.
/// Conversion is exact; exponents, fractional values and out-of-range keys fail.
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
    struct Entries<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Entries<T> {
        type Value = BTreeMap<i64, T>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("an object with integer score keys")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            let mut result = BTreeMap::new();
            while let Some(raw) = map.next_key::<String>()? {
                let key = score_key(&raw).map_err(serde::de::Error::custom)?;
                result.insert(key, map.next_value()?);
            }
            Ok(result)
        }
    }
    deserializer.deserialize_map(Entries(std::marker::PhantomData))
}

pub(crate) fn score_key(raw: &str) -> Result<i64, &'static str> {
    let raw = raw.trim();
    let integer = if let Some((integer, fraction)) = raw.split_once('.') {
        if fraction.is_empty() || !fraction.bytes().all(|b| b == b'0') {
            return Err("Expected an integer score key");
        }
        integer
    } else {
        raw
    };
    let digits = integer
        .strip_prefix(['+', '-'])
        .unwrap_or(integer)
        .as_bytes();
    if digits.is_empty()
        || !digits.iter().enumerate().all(|(i, b)| {
            b.is_ascii_digit()
                || (*b == b'_'
                    && i > 0
                    && digits[i - 1].is_ascii_digit()
                    && digits.get(i + 1).is_some_and(u8::is_ascii_digit))
        })
    {
        return Err("Expected an integer score key");
    }
    // Never round through f64: score levels can exceed its exact integer range.
    integer
        .replace('_', "")
        .parse()
        .map_err(|_| "Score key is outside the i64 range")
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
