//! Response data and the raw HTTP metadata returned alongside it.
pub(crate) mod decode;

use crate::{Content, HeaderMap};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt};

/// Decoded response data with the HTTP exchange that produced it.
#[derive(Clone, Debug)]
pub struct Response<T> {
    pub data: T,
    pub metadata: Metadata,
}

/// Raw transport metadata is separate from the serializable response data.
#[derive(Clone)]
pub struct Metadata {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Metadata {
    /// First ASCII request-ID header, or `None` if absent/non-ASCII.
    /// Duplicate and non-ASCII ID handling differs from Python; all raw values
    /// remain accessible through `headers`.
    pub fn request_id(&self) -> Option<&str> {
        self.headers.get("x-typesafe-request-id")?.to_str().ok()
    }
}

impl fmt::Debug for Metadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Metadata")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
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
        self.answers
            .iter()
            .filter_map(|(name, answer)| match answer {
                Answer::Choice(answer) => Some((name.as_str(), answer)),
                _ => None,
            })
    }

    pub fn scores(&self) -> impl Iterator<Item = (&str, &ScoreAnswer)> {
        self.answers
            .iter()
            .filter_map(|(name, answer)| match answer {
                Answer::Score(answer) => Some((name.as_str(), answer)),
                _ => None,
            })
    }

    pub fn nouls(&self) -> impl Iterator<Item = (&str, f64)> {
        self.answers
            .iter()
            .filter_map(|(name, answer)| match answer {
                Answer::Noul(answer) => Some((name.as_str(), answer.noul)),
                _ => None,
            })
    }
}

/// Missing/null counts become `None`; integer counts must fit in `i64`.
/// The Python reference also supports integers outside this range.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Usage {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
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

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

/// Parse a score-map key exactly, as documented on [`ScoreAnswer`].
fn score_key(raw: &str) -> Result<i64, &'static str> {
    const NOT_INTEGER: &str = "Expected an integer score key";
    let raw = raw.trim();
    let integer = match raw.split_once('.') {
        Some((integer, fraction))
            if !fraction.is_empty() && fraction.bytes().all(|b| b == b'0') =>
        {
            integer
        }
        Some(_) => return Err(NOT_INTEGER),
        None => raw,
    };
    let digits = integer
        .strip_prefix(['+', '-'])
        .unwrap_or(integer)
        .as_bytes();
    let valid_digit = |(i, b): (usize, &u8)| {
        b.is_ascii_digit()
            || (*b == b'_'
                && i > 0
                && digits[i - 1].is_ascii_digit()
                && digits.get(i + 1).is_some_and(u8::is_ascii_digit))
    };
    if digits.is_empty() || !digits.iter().enumerate().all(valid_digit) {
        return Err(NOT_INTEGER);
    }
    // Never round through f64: score levels can exceed its exact integer range.
    integer
        .replace('_', "")
        .parse()
        .map_err(|_| "Score key is outside the i64 range")
}

/// Available models in server order, including any duplicate entries.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListModelsResponse {
    pub models: Vec<ModelMetadata>,
}

/// Metadata for an available model. Unknown response fields are ignored.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelMetadata {
    pub name: String,
    pub description: String,
    /// Server-provided string, conventionally YYYY-MM-DD; no date parsing is applied.
    pub release_date: String,
}
