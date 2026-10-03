//! Native asynchronous TypeSafe client. Requires a Tokio runtime.
//!
//! ```no_run
//! use serde_json::json;
//! use typesafe_sdk::{Client, SystemOneRequest};
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = Client::builder().build()?;
//! let request = SystemOneRequest::from_json(
//!     json!("Please help"),
//!     json!({
//!         "urgent": {"type": "noul", "instructions": "Is this urgent?"}
//!     }),
//! )?;
//! let response = client.system_one(&request).await?;
//! println!("{:?}", response.data.answers);
//! # Ok(()) }
//! ```
mod client;
mod error;
mod request;
mod response;
mod retry;

pub use client::{Client, ClientBuilder, RequestOptions};
pub use error::{ApiError, Error, ErrorKind, InputError, InputErrorKind};
pub use request::{Content, Field, NoulCriteria, Question, SystemOneRequest};
pub use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
pub use response::{
    Answer, ChoiceAnswer, ListModelsResponse, Metadata, ModelMetadata, NoulAnswer, Response,
    ScoreAnswer, SystemOneResponse, Usage,
};
pub use retry::RetryPolicy;
