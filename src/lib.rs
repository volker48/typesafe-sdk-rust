//! Native asynchronous TypeSafe client. Requires a Tokio runtime.
//!
//! ```no_run
//! use std::collections::BTreeMap;
//! use typesafe_sdk::{Client, Content, Question, SystemOneRequest};
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = Client::builder().build()?;
//! let request = SystemOneRequest::new(Content::from("Please help"), BTreeMap::from([
//!     ("urgent".into(), Question::noul("Is this urgent?")),
//! ]))?;
//! let response = client.system_one(&request).await?;
//! println!("{:?}", response.data.answers);
//! # Ok(()) }
//! ```
mod client;
mod error;
mod models;
mod retry;
pub use client::{Client, ClientBuilder, RequestOptions};
pub use error::{ApiError, Error, ErrorKind, Metadata, Response};
pub use models::*;
pub use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
pub use retry::RetryPolicy;
