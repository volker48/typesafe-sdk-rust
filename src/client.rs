use crate::{
    Error, ErrorKind, HeaderMap, HeaderValue, Metadata, Response, RetryPolicy, SystemOneRequest,
    SystemOneResponse,
};
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

/// Cheap clones share the connection pool; no global state or background SDK tasks.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    url: reqwest::Url,
    headers: HeaderMap,
    authorization: HeaderValue,
    model: String,
    timeout: Duration,
    retry: RetryPolicy,
}
#[derive(Default)]
pub struct ClientBuilder {
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    headers: HeaderMap,
    timeout: Option<Duration>,
    retry: RetryPolicy,
}
/// Overrides apply to this call only; protected headers cannot be replaced.
#[derive(Clone, Debug, Default)]
pub struct RequestOptions {
    pub headers: HeaderMap,
    /// Total time for one attempt, through receipt of the whole response body.
    /// Unlike Python's phase timeout, this is not reset as data arrives.
    pub timeout: Option<Duration>,
    pub retry: Option<RetryPolicy>,
}
fn resolve(explicit: Option<String>, name: &str) -> Option<String> {
    explicit.or_else(|| {
        std::env::var(name)
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
    })
}
impl ClientBuilder {
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }
    pub fn headers(mut self, headers: HeaderMap) -> Self {
        self.headers = headers;
        self
    }
    /// Total time for one attempt, including receipt of the response body.
    /// Python's per-connect/read/write/pool timeout semantics are not implemented.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
    pub fn retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }
    /// Resolve explicit options, then trimmed environment values, then SDK defaults.
    /// URL userinfo/query/fragment are rejected to avoid ambiguous endpoint joining.
    pub fn build(self) -> Result<Client, Error> {
        let key = resolve(self.api_key, "TYPESAFE_API_KEY")
            .ok_or_else(|| Error::input("No API key was provided."))?;
        let base = resolve(self.base_url, "TYPESAFE_BASE_URL")
            .unwrap_or_else(|| "https://api.typesafe.ai".into());
        let mut url = reqwest::Url::parse(&base).map_err(|_| Error::input("Invalid base URL"))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(Error::input(
                "Base URL must be HTTP(S) without credentials, query or fragment",
            ));
        }
        url.set_path(&format!(
            "{}/v1/systemone",
            url.path().trim_end_matches('/')
        ));
        let model =
            resolve(self.model, "TYPESAFE_DEFAULT_MODEL").unwrap_or_else(|| "jev-latest".into());
        let timeout = self.timeout.unwrap_or(Duration::from_secs(10));
        if timeout.is_zero() {
            return Err(Error::input("Timeout must be positive"));
        }
        self.retry.validate()?;
        let mut auth = HeaderValue::from_str(&format!("Bearer {key}"))
            .map_err(|_| Error::input("Invalid API key header"))?;
        auth.set_sensitive(true);
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()
            .map_err(Error::transport)?;
        Ok(Client {
            http,
            url,
            headers: self.headers,
            authorization: auth,
            model,
            timeout,
            retry: self.retry,
        })
    }
}
impl Client {
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }
    pub async fn system_one(
        &self,
        request: &SystemOneRequest,
    ) -> Result<Response<SystemOneResponse>, Error> {
        self.system_one_with(request, &RequestOptions::default())
            .await
    }
    /// Dropping this future cancels the request or pending retry delay.
    pub async fn system_one_with(
        &self,
        request: &SystemOneRequest,
        options: &RequestOptions,
    ) -> Result<Response<SystemOneResponse>, Error> {
        let retry = options.retry.as_ref().unwrap_or(&self.retry);
        retry.validate()?;
        let timeout = options.timeout.unwrap_or(self.timeout);
        if timeout.is_zero() {
            return Err(Error::input("Timeout must be positive"));
        }
        let mut body = serde_json::json!({"state": request.state, "model": request.model.as_ref().unwrap_or(&self.model), "questions": request.questions});
        if let Value::Object(map) = &mut body {
            map.extend(request.extra_body.clone());
        }
        let bytes = serde_json::to_vec(&body)
            .map_err(|_| Error::input("Request could not be encoded as JSON"))?;
        let mut headers = self.headers.clone();
        headers.extend(options.headers.clone());
        headers.remove("x-typesafe-retry-count");
        // Authentication and SDK identity always win over custom headers.
        headers.insert("authorization", self.authorization.clone());
        for (name, value) in [
            ("accept", "application/json"),
            ("content-type", "application/json"),
            (
                "user-agent",
                concat!("typesafe-sdk-rust/", env!("CARGO_PKG_VERSION")),
            ),
            (
                "x-typesafe-sdk",
                concat!("typesafe-sdk-rust/", env!("CARGO_PKG_VERSION")),
            ),
            ("x-typesafe-runtime", "rust"),
        ] {
            headers.insert(name, HeaderValue::from_static(value));
        }
        let started = tokio::time::Instant::now();
        let mut attempt = 0;
        loop {
            if attempt > 0 {
                headers.insert(
                    "x-typesafe-retry-count",
                    HeaderValue::from_str(&attempt.to_string())
                        .map_err(|_| Error::input("Invalid retry count"))?,
                );
            }
            let result = self.attempt(bytes.clone(), headers.clone(), timeout).await;
            let error = match result {
                Ok(response) => return Ok(response),
                Err(error) => error,
            };
            if attempt >= retry.max_retries || !retry.retryable(&error) {
                return Err(error);
            }
            let delay = retry.delay(attempt, &error);
            if retry
                .timeout
                .is_some_and(|budget| started.elapsed().saturating_add(delay) >= budget)
            {
                return Err(error);
            }
            tokio::time::sleep(delay).await;
            attempt += 1;
        }
    }
    async fn attempt(
        &self,
        body: Vec<u8>,
        headers: HeaderMap,
        timeout: Duration,
    ) -> Result<Response<SystemOneResponse>, Error> {
        let response = self
            .http
            .post(self.url.clone())
            .headers(headers)
            .body(body)
            .timeout(timeout)
            .send()
            .await
            .map_err(Error::transport)?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = response.bytes().await.map_err(Error::transport)?.to_vec();
        let metadata = Metadata {
            status,
            headers,
            body,
        };
        let endpoint = format!("POST {}", self.url);
        if !(200..300).contains(&status) {
            return Err(Error::response(
                ErrorKind::status(status),
                metadata,
                endpoint,
                None,
                None,
            ));
        }
        match decode(&metadata.body) {
            Ok(data) => Ok(Response { data, metadata }),
            Err((path, source)) => Err(Error::response(
                ErrorKind::Validation,
                metadata,
                endpoint,
                Some(path),
                source,
            )),
        }
    }
}
type DecodeFailure = (String, Option<Box<dyn std::error::Error + Send + Sync>>);
fn decode(body: &[u8]) -> Result<SystemOneResponse, DecodeFailure> {
    let value: Value =
        serde_json::from_slice(body).map_err(|e| (String::new(), Some(Box::new(e) as _)))?;
    if !value.is_object() {
        return Err((String::new(), None));
    }
    if let Some(Value::Object(answers)) = value.get("answers") {
        for (name, raw) in answers.iter() {
            if raw.get("type").and_then(Value::as_str).is_none() {
                return Err((format!("answers.{name}.type"), None));
            }
        }
    }
    // Decode answers separately so paths do not expose Serde's enum internals.
    #[derive(Deserialize)]
    struct Envelope {
        model: String,
        usage: crate::Usage,
        #[serde(default)]
        answers: std::collections::BTreeMap<String, Value>,
    }
    let envelope: Envelope = parse(value)?;
    let mut answers = std::collections::BTreeMap::new();
    for (name, raw) in envelope.answers {
        let decoded = match raw["type"].as_str() {
            Some("noul") => parse(raw).map(crate::Answer::Noul),
            Some("choice") => parse(raw).map(crate::Answer::Choice),
            Some("score") => parse(raw).map(crate::Answer::Score),
            _ => continue,
        };
        let answer = decoded.map_err(|(path, source)| {
            (
                format!(
                    "answers.{name}{}",
                    if path.is_empty() {
                        String::new()
                    } else {
                        format!(".{path}")
                    }
                ),
                source,
            )
        })?;
        answers.insert(name, answer);
    }
    Ok(SystemOneResponse {
        model: envelope.model,
        usage: envelope.usage,
        answers,
    })
}
fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, DecodeFailure> {
    serde_path_to_error::deserialize(value).map_err(|e| {
        let mut path = e.path().to_string();
        if path == "." {
            path.clear();
        }
        let message = e.inner().to_string();
        if let Some(missing) = message
            .strip_prefix("missing field `")
            .and_then(|s| s.split('`').next())
        {
            if !path.is_empty() {
                path.push('.');
            }
            path.push_str(missing);
        }
        (path, Some(Box::new(e) as _))
    })
}
