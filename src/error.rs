use reqwest::header::HeaderMap;
use serde::Serialize;
use serde_json::Value;
use std::{error::Error as StdError, fmt, time::Duration};

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
#[derive(Clone, Debug)]
pub struct Response<T> {
    pub data: T,
    pub metadata: Metadata,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Input,
    Connection,
    Timeout,
    Validation,
    BadRequest,
    Authentication,
    PermissionDenied,
    NotFound,
    UnprocessableEntity,
    RateLimit,
    InternalServer,
    Api,
}
impl ErrorKind {
    pub(crate) fn status(status: u16) -> Self {
        match status {
            400 => Self::BadRequest,
            401 => Self::Authentication,
            403 => Self::PermissionDenied,
            404 => Self::NotFound,
            422 => Self::UnprocessableEntity,
            429 => Self::RateLimit,
            500.. => Self::InternalServer,
            _ => Self::Api,
        }
    }
}
/// Server data is accessible explicitly, but omitted from Debug and Display.
#[derive(Clone)]
pub struct ApiError {
    pub metadata: Metadata,
    pub body: Value,
    pub endpoint: String,
    pub field_path: Option<String>,
    /// Server-requested retry delay, captured when the response is received.
    /// Milliseconds take precedence over seconds/HTTP dates. Delays are rounded
    /// to nanoseconds; invalid, negative or out-of-range delays are `None`.
    /// Uses the first value of each header and standard HTTP-date syntax;
    /// Python's numeric separators and broader email-date formats are not supported.
    /// Available for every HTTP error status.
    pub retry_after: Option<Duration>,
}
impl fmt::Debug for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiError")
            .field("status", &self.metadata.status)
            .field("field_path", &self.field_path)
            .finish_non_exhaustive()
    }
}
pub struct Error {
    pub kind: ErrorKind,
    pub api: Option<Box<ApiError>>,
    message: String,
    source: Option<Box<dyn StdError + Send + Sync>>,
    input: Option<Box<InputError>>,
}

/// Stable reasons for request-construction failures, serialized as snake_case.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum InputErrorKind {
    InvalidContent,
    InvalidType,
    MissingField,
    UnknownField,
    UnknownQuestionKind,
    EmptyQuestions,
    EmptyScoreCriteria,
}

/// Local request diagnostic. The path can contain sensitive caller-defined keys.
/// Debug omits the path; read it explicitly when needed for input repair.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct InputError {
    pub kind: InputErrorKind,
    /// RFC 6901 JSON Pointer into the conceptual `{state, questions}` input.
    pub path: String,
}
impl fmt::Debug for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InputError")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}
impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Error")
            .field("kind", &self.kind)
            .field("api", &self.api)
            .field("input", &self.input)
            .finish_non_exhaustive()
    }
}
impl Error {
    /// Structured local request failure, absent for configuration/HTTP errors.
    /// Both the path here and an underlying Serde source may contain input data.
    pub fn input_details(&self) -> Option<&InputError> {
        self.input.as_deref()
    }

    pub(crate) fn request_input(kind: InputErrorKind, path: impl Into<String>) -> Self {
        let message = match kind {
            InputErrorKind::InvalidContent => "Expected text, an object, or an array",
            InputErrorKind::InvalidType => "Unexpected JSON type in request",
            InputErrorKind::MissingField => "Required request field is missing",
            InputErrorKind::UnknownField => "Unknown request field",
            InputErrorKind::UnknownQuestionKind => "Expected question type noul, choice, or score",
            InputErrorKind::EmptyQuestions => "At least one question is required",
            InputErrorKind::EmptyScoreCriteria => "At least one score criterion is required",
        };
        Self {
            input: Some(Box::new(InputError {
                kind,
                path: path.into(),
            })),
            ..Self::input(message)
        }
    }

    pub(crate) fn with_source(mut self, source: impl StdError + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    pub(crate) fn input(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Input,
            api: None,
            message: message.into(),
            source: None,
            input: None,
        }
    }
    pub(crate) fn transport(error: reqwest::Error) -> Self {
        Self {
            kind: if error.is_timeout() {
                ErrorKind::Timeout
            } else {
                ErrorKind::Connection
            },
            api: None,
            message: "HTTP transport failed".into(),
            source: Some(Box::new(error.without_url())),
            input: None,
        }
    }
    pub(crate) fn response(
        kind: ErrorKind,
        metadata: Metadata,
        endpoint: String,
        field_path: Option<String>,
        source: Option<Box<dyn StdError + Send + Sync>>,
    ) -> Self {
        let retry_after =
            crate::retry::retry_after(&metadata.headers, std::time::SystemTime::now());
        let body = if metadata.body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&metadata.body).unwrap_or_else(|_| {
                Value::String(String::from_utf8_lossy(&metadata.body).into_owned())
            })
        };
        Self {
            kind,
            message: format!("HTTP {} ({kind:?})", metadata.status),
            api: Some(Box::new(ApiError {
                metadata,
                body,
                endpoint,
                field_path,
                retry_after,
            })),
            source,
            input: None,
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(api) = &self.api {
            write!(f, "{}: {}", api.endpoint, self.message)?;
            if let Some(id) = api.metadata.request_id() {
                write!(f, " (request_id={id})")?;
            }
            Ok(())
        } else {
            f.write_str(&self.message)
        }
    }
}
impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source.as_deref().map(|s| s as _)
    }
}
