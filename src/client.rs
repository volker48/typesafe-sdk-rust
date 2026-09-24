use crate::{
    Error, HeaderMap, HeaderValue, ListModelsResponse, Metadata, Response, RetryPolicy,
    SystemOneRequest, SystemOneResponse,
    response::decode::{self, Decoder},
};
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::time::Duration;

const DEFAULT_BASE_URL: &str = "https://api.typesafe.ai";
const DEFAULT_MODEL: &str = "jev-latest";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const SDK_IDENTITY: &str = concat!("typesafe-sdk-rust/", env!("CARGO_PKG_VERSION"));
const RETRY_COUNT_HEADER: &str = "x-typesafe-retry-count";

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

impl ClientBuilder {
    /// Defaults to `TYPESAFE_API_KEY`; building fails without a key.
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// Defaults to `TYPESAFE_BASE_URL`, then `https://api.typesafe.ai`.
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Defaults to `TYPESAFE_DEFAULT_MODEL`, then `jev-latest`.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Headers sent with every call. Per-call headers replace same-named values;
    /// SDK-owned authentication and identity headers always win.
    pub fn headers(mut self, headers: HeaderMap) -> Self {
        self.headers = headers;
        self
    }

    /// Total time for one attempt, including receipt of the response body.
    /// Defaults to 10 seconds. Python's per-connect/read/write/pool timeout
    /// semantics are not implemented.
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
        let key = setting(self.api_key, "TYPESAFE_API_KEY")
            .ok_or_else(|| Error::input("No API key was provided"))?;
        let base_url =
            setting(self.base_url, "TYPESAFE_BASE_URL").unwrap_or_else(|| DEFAULT_BASE_URL.into());
        let base_url = parse_base_url(&base_url)?;
        let model =
            setting(self.model, "TYPESAFE_DEFAULT_MODEL").unwrap_or_else(|| DEFAULT_MODEL.into());
        let timeout = positive_timeout(self.timeout.unwrap_or(DEFAULT_TIMEOUT))?;
        self.retry.validate()?;
        let mut authorization = HeaderValue::from_str(&format!("Bearer {key}"))
            .map_err(|_| Error::input("Invalid API key header"))?;
        authorization.set_sensitive(true);
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()
            .map_err(Error::transport)?;
        Ok(Client {
            http,
            base_url,
            headers: self.headers,
            authorization,
            model,
            timeout,
            retry: self.retry,
        })
    }
}

/// The explicit value, else the trimmed, nonempty environment variable.
fn setting(explicit: Option<String>, variable: &str) -> Option<String> {
    explicit.or_else(|| {
        std::env::var(variable)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    })
}

fn parse_base_url(raw: &str) -> Result<reqwest::Url, Error> {
    let url = reqwest::Url::parse(raw).map_err(|_| Error::input("Invalid base URL"))?;
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
    Ok(url)
}

fn positive_timeout(timeout: Duration) -> Result<Duration, Error> {
    if timeout.is_zero() {
        return Err(Error::input("Timeout must be positive"));
    }
    Ok(timeout)
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
        let model = request.model.as_ref().unwrap_or(&self.model).clone();
        let mut body = serde_json::Map::from_iter([
            (
                "state".into(),
                serde_json::to_value(&request.state).map_err(Error::encoding)?,
            ),
            ("model".into(), Value::String(model)),
            (
                "questions".into(),
                serde_json::to_value(&request.questions).map_err(Error::encoding)?,
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
        self.post_system_one(request, options, decode::system_one)
            .await
    }

    /// Decode the complete response JSON directly into a caller-defined Serde type.
    ///
    /// `T` controls required fields, unknown fields, and answer representations.
    /// No standard-envelope validation, answer filtering, or Python-style answer
    /// lifting is applied. Even `T = SystemOneResponse` uses its Serde contract;
    /// use [`Self::system_one`] for the standard Python-compatible decoder.
    /// HTTP metadata, errors, retries and cancellation retain their usual behavior.
    ///
    /// ```no_run
    /// # async fn example(client: &typesafe_sdk::Client, request: &typesafe_sdk::SystemOneRequest)
    /// # -> Result<(), typesafe_sdk::Error> {
    /// #[derive(serde::Deserialize)]
    /// struct Answers { urgent: typesafe_sdk::NoulAnswer }
    /// #[derive(serde::Deserialize)]
    /// struct ResultData { answers: Answers }
    /// let response = client.system_one_as::<ResultData>(request).await?;
    /// println!("{}", response.data.answers.urgent.noul);
    /// # Ok(()) }
    /// ```
    pub async fn system_one_as<T: DeserializeOwned>(
        &self,
        request: &SystemOneRequest,
    ) -> Result<Response<T>, Error> {
        self.system_one_as_with(request, &RequestOptions::default())
            .await
    }

    /// Custom response decoding with per-call headers, timeout and retry options.
    ///
    /// The body must be exactly one JSON value; malformed JSON and trailing content
    /// fail validation. Validation failures retain raw metadata and a Serde source.
    /// Field paths follow Serde: missing fields identify the containing object, and
    /// untagged enums/custom deserializers may report only their outer path. The
    /// empty path denotes the root. Validation errors are not retried by the
    /// default policy.
    pub async fn system_one_as_with<T: DeserializeOwned>(
        &self,
        request: &SystemOneRequest,
        options: &RequestOptions,
    ) -> Result<Response<T>, Error> {
        self.post_system_one(request, options, decode::custom::<T>)
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
            Method::GET,
            "/v1/models",
            None,
            options,
            decode::list_models,
        )
        .await
    }

    async fn post_system_one<T>(
        &self,
        request: &SystemOneRequest,
        options: &RequestOptions,
        decode: Decoder<T>,
    ) -> Result<Response<T>, Error> {
        let body = serde_json::to_vec(&self.system_one_body(request)?).map_err(Error::encoding)?;
        self.send(Method::POST, "/v1/systemone", Some(body), options, decode)
            .await
    }

    /// Execute one call, retrying failed attempts as the effective policy allows.
    async fn send<T>(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
        options: &RequestOptions,
        decode: Decoder<T>,
    ) -> Result<Response<T>, Error> {
        let retry = options.retry.as_ref().unwrap_or(&self.retry);
        retry.validate()?;
        let timeout = positive_timeout(options.timeout.unwrap_or(self.timeout))?;
        let mut request = self.prepare(method, path, body, &options.headers, timeout)?;
        let started = tokio::time::Instant::now();
        let mut attempt = 0;
        loop {
            if attempt > 0 {
                request
                    .headers_mut()
                    .insert(RETRY_COUNT_HEADER, HeaderValue::from(attempt));
            }
            let error = match self.attempt(&request, decode).await {
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

    /// Build the request every attempt resends.
    fn prepare(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
        call_headers: &HeaderMap,
        timeout: Duration,
    ) -> Result<reqwest::Request, Error> {
        let mut url = self.base_url.clone();
        url.set_path(&format!("{}{path}", url.path().trim_end_matches('/')));
        let mut headers = self.headers.clone();
        headers.extend(call_headers.clone());
        headers.remove(RETRY_COUNT_HEADER);
        // Authentication and SDK identity always win over custom headers.
        headers.insert("authorization", self.authorization.clone());
        for (name, value) in [
            ("accept", "application/json"),
            ("user-agent", SDK_IDENTITY),
            ("x-typesafe-sdk", SDK_IDENTITY),
            ("x-typesafe-runtime", "rust"),
        ] {
            headers.insert(name, HeaderValue::from_static(value));
        }
        let mut request = self.http.request(method, url).timeout(timeout);
        if let Some(body) = body {
            headers.insert("content-type", HeaderValue::from_static("application/json"));
            request = request.body(body);
        }
        request.headers(headers).build().map_err(Error::transport)
    }

    async fn attempt<T>(
        &self,
        request: &reqwest::Request,
        decode: Decoder<T>,
    ) -> Result<Response<T>, Error> {
        // Requests contain only owned bytes or no body, so cloning cannot fail.
        let request_copy = request
            .try_clone()
            .expect("SDK request bodies are cloneable");
        let response = self
            .http
            .execute(request_copy)
            .await
            .map_err(Error::transport)?;
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.bytes().await.map_err(Error::transport)?.to_vec();
        let metadata = Metadata {
            status: status.as_u16(),
            headers,
            body,
        };
        let endpoint = || format!("{} {}", request.method(), request.url());
        if !status.is_success() {
            return Err(Error::http_status(metadata, endpoint()));
        }
        match decode(&metadata.body) {
            Ok(data) => Ok(Response { data, metadata }),
            Err(failure) => Err(Error::validation(metadata, endpoint(), failure)),
        }
    }
}
