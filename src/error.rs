use crate::{Metadata, response::decode::Failure};
use serde::Serialize;
use serde_json::Value;
use std::{
    error::Error as StdError,
    fmt,
    time::{Duration, SystemTime},
};

pub(crate) type BoxError = Box<dyn StdError + Send + Sync>;

/// Broad failure category, serialized as snake_case.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// Invalid configuration or request input, detected before any HTTP request.
    Input,
    /// HTTP transport failure other than a timeout.
    Connection,
    /// An attempt exceeded its timeout.
    Timeout,
    /// A successful HTTP response could not be decoded.
    Validation,
    /// HTTP 400.
    BadRequest,
    /// HTTP 401.
    Authentication,
    /// HTTP 403.
    PermissionDenied,
    /// HTTP 404.
    NotFound,
    /// HTTP 422.
    UnprocessableEntity,
    /// HTTP 429.
    RateLimit,
    /// HTTP 500 or above.
    InternalServer,
    /// Any other unsuccessful HTTP status.
    Api,
}

impl ErrorKind {
    fn from_status(status: u16) -> Self {
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

pub struct Error {
    pub kind: ErrorKind,
    pub api: Option<Box<ApiError>>,
    message: String,
    source: Option<BoxError>,
    input: Option<Box<InputError>>,
}

impl Error {
    /// Structured local request failure, absent for configuration/HTTP errors.
    /// Both the path here and an underlying Serde source may contain input data.
    pub fn input_details(&self) -> Option<&InputError> {
        self.input.as_deref()
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

    pub(crate) fn request_input(kind: InputErrorKind, path: impl Into<String>) -> Self {
        Self {
            input: Some(Box::new(InputError {
                kind,
                path: path.into(),
            })),
            ..Self::input(kind.message())
        }
    }

    pub(crate) fn encoding(source: serde_json::Error) -> Self {
        Self::input("Request could not be encoded as JSON").with_source(source)
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

    /// An unsuccessful HTTP status.
    pub(crate) fn http_status(metadata: Metadata, endpoint: String) -> Self {
        let kind = ErrorKind::from_status(metadata.status);
        Self::api(kind, metadata, endpoint, None, None)
    }

    /// A successful HTTP status whose body failed validation.
    pub(crate) fn validation(metadata: Metadata, endpoint: String, failure: Failure) -> Self {
        Self::api(
            ErrorKind::Validation,
            metadata,
            endpoint,
            Some(failure.path),
            Some(failure.source),
        )
    }

    pub(crate) fn with_source(mut self, source: impl StdError + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    fn api(
        kind: ErrorKind,
        metadata: Metadata,
        endpoint: String,
        field_path: Option<String>,
        source: Option<BoxError>,
    ) -> Self {
        let retry_after = crate::retry::retry_after(&metadata.headers, SystemTime::now());
        let body = parse_body(&metadata.body);
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

/// JSON when possible, otherwise the body as lossy text; empty is null.
fn parse_body(bytes: &[u8]) -> Value {
    if bytes.is_empty() {
        return Value::Null;
    }
    serde_json::from_slice(bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(bytes).into_owned()))
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

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some(api) = &self.api else {
            return f.write_str(&self.message);
        };
        write!(f, "{}: {}", api.endpoint, self.message)?;
        if let Some(id) = api.metadata.request_id() {
            write!(f, " (request_id={id})")?;
        }
        Ok(())
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source.as_deref().map(|source| source as _)
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
    EmptyQuestionKind,
    EmptyQuestions,
    EmptyScoreCriteria,
}

impl InputErrorKind {
    fn message(self) -> &'static str {
        match self {
            Self::InvalidContent => "Expected text, an object, or an array",
            Self::InvalidType => "Unexpected JSON type in request",
            Self::MissingField => "Required request field is missing",
            Self::UnknownField => "Unknown request field",
            Self::UnknownQuestionKind => "Expected question type noul, choice, or score",
            Self::EmptyQuestionKind => "Question type must be a nonempty string",
            Self::EmptyQuestions => "At least one question is required",
            Self::EmptyScoreCriteria => "At least one score criterion is required",
        }
    }
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
