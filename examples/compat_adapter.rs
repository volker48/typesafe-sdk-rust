//! Test-only JSON-lines boundary; business behavior remains in the public SDK.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Read},
    time::{Duration, TryFromFloatSecsError},
};
use typesafe_sdk::*;

type AdapterResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scenario {
    base_url: String,
    /// Replaced by `<origin>` in error endpoints, since the port is random.
    origin: String,
    #[serde(default)]
    config: Config,
    calls: Vec<Call>,
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

/// Retry overrides in seconds; omitted fields keep the SDK default.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Policy {
    max_retries: Option<u32>,
    backoff_initial: Option<f64>,
    backoff_max: Option<f64>,
    backoff_jitter: Option<f64>,
    #[serde(default)]
    timeout: Field<f64>,
    http_statuses: Option<BTreeSet<u16>>,
    respect_retry_after: Option<bool>,
    api_connection_error: Option<bool>,
    api_timeout_error: Option<bool>,
}

impl Policy {
    fn convert(self) -> Result<RetryPolicy, TryFromFloatSecsError> {
        let seconds = |value: Option<f64>| value.map(Duration::try_from_secs_f64).transpose();
        let default = RetryPolicy::default();
        Ok(RetryPolicy {
            max_retries: self.max_retries.unwrap_or(default.max_retries),
            backoff_initial: seconds(self.backoff_initial)?.unwrap_or(default.backoff_initial),
            backoff_max: seconds(self.backoff_max)?.unwrap_or(default.backoff_max),
            backoff_jitter: self.backoff_jitter.unwrap_or(default.backoff_jitter),
            http_statuses: self.http_statuses.unwrap_or(default.http_statuses),
            respect_retry_after: self
                .respect_retry_after
                .unwrap_or(default.respect_retry_after),
            api_connection_error: self
                .api_connection_error
                .unwrap_or(default.api_connection_error),
            api_timeout_error: self.api_timeout_error.unwrap_or(default.api_timeout_error),
            timeout: match self.timeout {
                Field::Omitted => default.timeout,
                Field::Null => None,
                Field::Value(value) => Some(Duration::try_from_secs_f64(value)?),
            },
        })
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Call {
    Models(ModelsCall),
    SystemOne(Box<SystemOneCall>),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelsCall {
    #[serde(rename = "operation")]
    _operation: ModelsOperation,
    #[serde(default)]
    observe_retry_after: bool,
    timeout: Option<f64>,
    retry: Option<Policy>,
    #[serde(default)]
    extra_headers: BTreeMap<String, String>,
}

#[derive(Deserialize)]
enum ModelsOperation {
    #[serde(rename = "list_models")]
    ListModels,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SystemOneCall {
    #[serde(default)]
    observe_retry_after: bool,
    /// Selects Python's typed constructors; Rust always builds typed questions
    /// unless `raw_questions` is set.
    #[serde(default)]
    typed: bool,
    #[serde(default)]
    raw_questions: bool,
    #[serde(default)]
    custom_response: bool,
    state: Content,
    questions: Value,
    model: Option<String>,
    timeout: Option<f64>,
    retry: Option<Policy>,
    #[serde(default)]
    extra_headers: BTreeMap<String, String>,
    #[serde(default)]
    extra_body: Map<String, Value>,
}

#[derive(Deserialize, Serialize)]
struct ExtensionResponse {
    model: String,
    answers: BTreeMap<String, ExtensionAnswer>,
}

#[derive(Deserialize, Serialize)]
struct ExtensionAnswer {
    value: Vec<String>,
}

#[tokio::main]
async fn main() -> AdapterResult<()> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let scenario: Scenario = serde_json::from_str(&input)?;
    let client = client(scenario.base_url, scenario.config)?;
    let mut observations = Vec::new();
    for call in scenario.calls {
        let observation = match call {
            Call::SystemOne(call) => system_one(&client, *call, &scenario.origin).await?,
            Call::Models(call) => {
                let options = options(call.extra_headers, call.timeout, call.retry)?;
                match client.list_models_with(&options).await {
                    Ok(response) => success(response)?,
                    Err(error) => failure(error, call.observe_retry_after, &scenario.origin)?,
                }
            }
        };
        observations.push(observation);
    }
    println!("{}", serde_json::to_string(&observations)?);
    Ok(())
}

fn client(base_url: String, config: Config) -> AdapterResult<Client> {
    let mut builder = Client::builder()
        .base_url(base_url)
        .headers(headers(config.headers)?);
    if let Some(api_key) = config.api_key {
        builder = builder.api_key(api_key);
    }
    if let Some(model) = config.model {
        builder = builder.model(model);
    }
    if let Some(timeout) = config.timeout {
        builder = builder.timeout(Duration::try_from_secs_f64(timeout)?);
    }
    if let Some(retry) = config.retry {
        builder = builder.retry(retry.convert()?);
    }
    Ok(builder.build()?)
}

async fn system_one(client: &Client, call: SystemOneCall, origin: &str) -> AdapterResult<Value> {
    if call.raw_questions && call.typed {
        return Err("Raw and typed question modes are mutually exclusive".into());
    }
    let request = if call.raw_questions {
        SystemOneRequest::from_raw_json(serde_json::to_value(call.state)?, call.questions)
    } else {
        SystemOneRequest::new(call.state, serde_json::from_value(call.questions)?)
    };
    let mut request = match request {
        Ok(request) => request,
        Err(error) => return Ok(json!({"error": error.kind})),
    };
    request.model = call.model;
    request.extra_body = call.extra_body;
    let options = options(call.extra_headers, call.timeout, call.retry)?;
    if call.custom_response {
        match client
            .system_one_as_with::<ExtensionResponse>(&request, &options)
            .await
        {
            // Standalone Pydantic models expose no HTTP metadata to compare.
            Ok(response) => Ok(json!({"ok": serde_json::to_value(response.data)?})),
            Err(error) => failure(error, call.observe_retry_after, origin),
        }
    } else {
        match client.system_one_with(&request, &options).await {
            Ok(response) => success(response),
            Err(error) => failure(error, call.observe_retry_after, origin),
        }
    }
}

fn options(
    extra_headers: BTreeMap<String, String>,
    timeout: Option<f64>,
    retry: Option<Policy>,
) -> AdapterResult<RequestOptions> {
    Ok(RequestOptions {
        headers: headers(extra_headers)?,
        timeout: timeout.map(Duration::try_from_secs_f64).transpose()?,
        retry: retry.map(Policy::convert).transpose()?,
    })
}

fn success<T: Serialize>(response: Response<T>) -> AdapterResult<Value> {
    let metadata = &response.metadata;
    Ok(json!({
        "ok": serde_json::to_value(&response.data)?,
        "metadata": {
            "status": metadata.status,
            "headers": header_json(&metadata.headers)?,
            "raw_hex": metadata.body.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        },
    }))
}

fn failure(error: Error, observe_retry_after: bool, origin: &str) -> AdapterResult<Value> {
    let Some(api) = error.api else {
        return Ok(json!({"error": error.kind}));
    };
    let mut observation = json!({
        "error": error.kind,
        "status": api.metadata.status,
        "body": api.body,
        "headers": header_json(&api.metadata.headers)?,
        "request_id": api.metadata.request_id(),
        "field_path": api.field_path,
        "endpoint": api.endpoint.replace(origin, "<origin>"),
    });
    if observe_retry_after {
        // Python exposes the delay only on rate-limit errors.
        let delay = api
            .retry_after
            .filter(|_| error.kind == ErrorKind::RateLimit);
        observation["retry_after_ms"] = json!(delay.map(|delay| delay.as_secs_f64() * 1000.0));
    }
    Ok(observation)
}

fn headers(values: BTreeMap<String, String>) -> AdapterResult<HeaderMap> {
    let mut result = HeaderMap::new();
    for (name, value) in values {
        result.insert(
            HeaderName::from_bytes(name.as_bytes())?,
            HeaderValue::from_str(&value)?,
        );
    }
    Ok(result)
}

fn header_json(headers: &HeaderMap) -> AdapterResult<Value> {
    let mut result = Map::new();
    for (name, value) in headers {
        result.insert(name.to_string(), Value::String(value.to_str()?.into()));
    }
    Ok(Value::Object(result))
}

#[test]
fn unsupported_adapter_options_fail_instead_of_being_ignored() {
    assert!(serde_json::from_str::<Policy>(r#"{"api_timout_error":false}"#).is_err());
    assert!(serde_json::from_str::<Config>(r#"{"modle":"m"}"#).is_err());
    assert!(
        serde_json::from_str::<Call>(r#"{"state":"x","questions":{},"response_model":"ignored"}"#)
            .is_err()
    );
    for invalid in [
        r#"{"operation":"unknown"}"#,
        r#"{"operation":"list_models","state":"x"}"#,
        r#"{"operation":"list_models","extra_body":{}}"#,
        r#"{"operation":"list_models","model":"m"}"#,
    ] {
        assert!(serde_json::from_str::<Call>(invalid).is_err());
    }
    let policy: Policy =
        serde_json::from_str(r#"{"api_connection_error":false,"api_timeout_error":false}"#)
            .unwrap();
    let policy = policy.convert().unwrap();
    assert!(!policy.api_connection_error);
    assert!(!policy.api_timeout_error);
}
