use crate::{
    Error, ErrorKind, HeaderMap, HeaderValue, ListModelsResponse, Metadata, Response, RetryPolicy,
    SystemOneRequest, SystemOneResponse,
};
use serde_json::Value;
use std::time::Duration;

/// Cheap clones share the connection pool; no global state or background SDK tasks.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base_url: reqwest::Url,
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
        let url = reqwest::Url::parse(&base).map_err(|_| Error::input("Invalid base URL"))?;
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
            base_url: url,
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
    /// Render the effective JSON body without HTTP or transport headers.
    ///
    /// Includes the resolved model and shallow final `extra_body` overrides.
    /// Reserved-field overrides can replace validated state/questions with raw
    /// values, including null. The result contains potentially sensitive evidence;
    /// it is never automatically logged. This is a snapshot: later request
    /// mutations are reflected only by subsequent previews or execution.
    pub fn system_one_body(&self, request: &SystemOneRequest) -> Result<Value, Error> {
        let encoding_error =
            |source| Error::input("Request could not be encoded as JSON").with_source(source);
        let mut body = serde_json::Map::from_iter([
            (
                "state".into(),
                serde_json::to_value(&request.state).map_err(encoding_error)?,
            ),
            (
                "model".into(),
                Value::String(request.model.as_ref().unwrap_or(&self.model).clone()),
            ),
            (
                "questions".into(),
                serde_json::to_value(&request.questions).map_err(encoding_error)?,
            ),
        ]);
        body.extend(request.extra_body.clone());
        Ok(Value::Object(body))
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
        let body = self.system_one_body(request)?;
        let bytes = serde_json::to_vec(&body).map_err(|source| {
            Error::input("Request could not be encoded as JSON").with_source(source)
        })?;
        self.send(
            reqwest::Method::POST,
            "/v1/systemone",
            Some(bytes),
            options,
            crate::decode::system_one,
        )
        .await
    }

    /// List the available models in server order. No pagination is performed.
    ///
    /// ```no_run
    /// # async fn example(client: &typesafe_sdk::Client) -> Result<(), typesafe_sdk::Error> {
    /// let response = client.list_models().await?;
    /// for model in response.data.models {
    ///     println!("{}: {}", model.name, model.description);
    /// }
    /// # Ok(()) }
    /// ```
    pub async fn list_models(&self) -> Result<Response<ListModelsResponse>, Error> {
        self.list_models_with(&RequestOptions::default()).await
    }

    /// List models with per-call headers, timeout and retry overrides.
    /// Sends a bodyless GET; dropping the future cancels the call or retry delay.
    /// No Content-Type is added; a caller-supplied Content-Type is preserved.
    pub async fn list_models_with(
        &self,
        options: &RequestOptions,
    ) -> Result<Response<ListModelsResponse>, Error> {
        self.send(
            reqwest::Method::GET,
            "/v1/models",
            None,
            options,
            crate::decode::list_models,
        )
        .await
    }

    async fn send<T>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Vec<u8>>,
        options: &RequestOptions,
        decode: fn(&[u8]) -> Result<T, crate::decode::Failure>,
    ) -> Result<Response<T>, Error> {
        let retry = options.retry.as_ref().unwrap_or(&self.retry);
        retry.validate()?;
        let timeout = options.timeout.unwrap_or(self.timeout);
        if timeout.is_zero() {
            return Err(Error::input("Timeout must be positive"));
        }
        let mut url = self.base_url.clone();
        url.set_path(&format!("{}{path}", url.path().trim_end_matches('/')));
        let mut headers = self.headers.clone();
        headers.extend(options.headers.clone());
        headers.remove("x-typesafe-retry-count");
        // Authentication and SDK identity always win over custom headers.
        headers.insert("authorization", self.authorization.clone());
        for (name, value) in [
            ("accept", "application/json"),
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
        let mut request = self.http.request(method, url).timeout(timeout);
        if let Some(body) = body {
            headers.insert("content-type", HeaderValue::from_static("application/json"));
            request = request.body(body);
        }
        let mut request = request.headers(headers).build().map_err(Error::transport)?;
        let started = tokio::time::Instant::now();
        let mut attempt = 0;
        loop {
            if attempt > 0 {
                request.headers_mut().insert(
                    "x-typesafe-retry-count",
                    HeaderValue::from_str(&attempt.to_string())
                        .map_err(|_| Error::input("Invalid retry count"))?,
                );
            }
            let result = self.attempt(&request, decode).await;
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
    async fn attempt<T>(
        &self,
        request: &reqwest::Request,
        decode: fn(&[u8]) -> Result<T, crate::decode::Failure>,
    ) -> Result<Response<T>, Error> {
        let response = self
            .http
            // Requests contain only owned bytes or no body, so cloning cannot fail.
            .execute(
                request
                    .try_clone()
                    .expect("SDK request bodies are cloneable"),
            )
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
        let endpoint = format!("{} {}", request.method(), request.url());
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
