//! Test-only JSON-lines boundary; business behavior remains in the public SDK.
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, Read},
    time::Duration,
};
use typesafe_sdk::*;

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Policy {
    max_retries: Option<u32>,
    backoff_initial: Option<f64>,
    backoff_max: Option<f64>,
    backoff_jitter: Option<f64>,
    #[serde(default)]
    timeout: Field<f64>,
    http_statuses: Option<std::collections::BTreeSet<u16>>,
    respect_retry_after: Option<bool>,
    api_connection_error: Option<bool>,
    api_timeout_error: Option<bool>,
}
impl Policy {
    fn convert(self) -> Result<RetryPolicy, std::time::TryFromFloatSecsError> {
        let mut p = RetryPolicy::default();
        if let Some(v) = self.max_retries {
            p.max_retries = v;
        }
        if let Some(v) = self.backoff_initial {
            p.backoff_initial = Duration::try_from_secs_f64(v)?;
        }
        if let Some(v) = self.backoff_max {
            p.backoff_max = Duration::try_from_secs_f64(v)?;
        }
        if let Some(v) = self.backoff_jitter {
            p.backoff_jitter = v;
        }
        match self.timeout {
            Field::Null => p.timeout = None,
            Field::Value(v) => p.timeout = Some(Duration::try_from_secs_f64(v)?),
            _ => {}
        }
        if let Some(v) = self.http_statuses {
            p.http_statuses = v;
        }
        if let Some(v) = self.respect_retry_after {
            p.respect_retry_after = v;
        }
        if let Some(v) = self.api_connection_error {
            p.api_connection_error = v;
        }
        if let Some(v) = self.api_timeout_error {
            p.api_timeout_error = v;
        }
        Ok(p)
    }
}
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Config {
    api_key: Option<String>,
    model: Option<String>,
    timeout: Option<f64>,
    retry: Option<Policy>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Call {
    // Rust always uses typed questions; this selects Python's constructor path.
    #[serde(default, rename = "typed")]
    _typed: bool,
    state: Content,
    questions: BTreeMap<String, Question>,
    model: Option<String>,
    timeout: Option<f64>,
    retry: Option<Policy>,
    #[serde(default)]
    extra_headers: BTreeMap<String, String>,
    #[serde(default)]
    extra_body: serde_json::Map<String, Value>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scenario {
    base_url: String,
    origin: String,
    #[serde(default)]
    config: Config,
    calls: Vec<Call>,
}
fn headers(values: BTreeMap<String, String>) -> Result<HeaderMap, Box<dyn std::error::Error>> {
    let mut result = HeaderMap::new();
    for (k, v) in values {
        result.insert(
            HeaderName::from_bytes(k.as_bytes())?,
            HeaderValue::from_str(&v)?,
        );
    }
    Ok(result)
}
fn header_json(headers: &HeaderMap) -> Result<Value, Box<dyn std::error::Error>> {
    let mut result = serde_json::Map::new();
    for (name, value) in headers {
        result.insert(name.to_string(), Value::String(value.to_str()?.into()));
    }
    Ok(Value::Object(result))
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let scenario: Scenario = serde_json::from_str(&input)?;
    let c = scenario.config;
    let mut builder = Client::builder()
        .base_url(scenario.base_url)
        .headers(headers(c.headers)?);
    if let Some(v) = c.api_key {
        builder = builder.api_key(v);
    }
    if let Some(v) = c.model {
        builder = builder.model(v);
    }
    if let Some(v) = c.timeout {
        builder = builder.timeout(Duration::try_from_secs_f64(v)?);
    }
    if let Some(v) = c.retry {
        builder = builder.retry(v.convert()?);
    }
    let client = builder.build()?;
    let mut observations = Vec::new();
    for call in scenario.calls {
        let mut request = SystemOneRequest::new(call.state, call.questions)?;
        request.model = call.model;
        request.extra_body = call.extra_body;
        let options = RequestOptions {
            headers: headers(call.extra_headers)?,
            timeout: call.timeout.map(Duration::try_from_secs_f64).transpose()?,
            retry: call.retry.map(Policy::convert).transpose()?,
        };
        match client.system_one_with(&request, &options).await {
            Ok(r) => observations.push(json!({"ok": r.data, "metadata": {"status": r.metadata.status, "headers": header_json(&r.metadata.headers)?, "raw_hex": r.metadata.body.iter().map(|b| format!("{b:02x}")).collect::<String>()}})),
            Err(e) => if let Some(api) = e.api { observations.push(json!({"error": e.kind, "status": api.metadata.status, "body": api.body, "headers": header_json(&api.metadata.headers)?, "request_id": api.metadata.request_id(), "field_path": api.field_path, "endpoint": api.endpoint.replace(&scenario.origin, "<origin>")})); } else { observations.push(json!({"error": e.kind})); }
        }
    }
    println!("{}", serde_json::to_string(&observations)?);
    Ok(())
}

#[test]
fn unsupported_adapter_options_fail_instead_of_being_ignored() {
    assert!(serde_json::from_str::<Policy>(r#"{"api_timout_error":false}"#).is_err());
    assert!(serde_json::from_str::<Config>(r#"{"modle":"m"}"#).is_err());
    assert!(
        serde_json::from_str::<Call>(r#"{"state":"x","questions":{},"response_model":"ignored"}"#)
            .is_err()
    );
    let policy: Policy =
        serde_json::from_str(r#"{"api_connection_error":false,"api_timeout_error":false}"#)
            .unwrap();
    let policy = policy.convert().unwrap();
    assert!(!policy.api_connection_error);
    assert!(!policy.api_timeout_error);
}
