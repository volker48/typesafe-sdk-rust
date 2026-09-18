//! Native asynchronous TypeSafe client. Requires a Tokio runtime.
//!
//! ```no_run
//! use serde_json::json;
//! use typesafe_sdk::{Client, SystemOneRequest};
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = Client::builder().build()?;
//! let request = SystemOneRequest::new(
//!     "Please help".into(),
//!     serde_json::from_value(json!({
//!         "urgent": {"type": "noul", "instructions": "Is this urgent?"}
//!     }))?,
//! )?;
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
