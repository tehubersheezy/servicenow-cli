use crate::config::ResolvedProfile;
use crate::error::{Error, Result};
use reqwest::blocking::{Client as ReqwestClient, Response};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_TYPE, USER_AGENT};
use reqwest::{Method, StatusCode};
use serde_json::Value;
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
pub struct RetryPolicy {
    pub enabled: bool,
    pub max_attempts: u32,
    pub initial_backoff: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            max_attempts: 3,
            initial_backoff: Duration::from_millis(500),
        }
    }
}

pub struct Client {
    http: ReqwestClient,
    base_url: String,
    username: String,
    password: String,
    retry: RetryPolicy,
}

pub struct ClientBuilder {
    timeout: Duration,
    user_agent: String,
    retry: RetryPolicy,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            user_agent: format!("sn/{}", env!("CARGO_PKG_VERSION")),
            retry: RetryPolicy::default(),
        }
    }
}

impl ClientBuilder {
    pub fn timeout(mut self, d: Duration) -> Self {
        self.timeout = d;
        self
    }

    pub fn retry(mut self, policy: RetryPolicy) -> Self {
        self.retry = policy;
        self
    }

    pub fn build(self, profile: &ResolvedProfile) -> Result<Client> {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(USER_AGENT, HeaderValue::from_str(&self.user_agent).unwrap());
        let http = ReqwestClient::builder()
            .timeout(self.timeout)
            .default_headers(headers)
            .build()
            .map_err(|e| Error::Transport(format!("build client: {e}")))?;
        let base_url = normalize_base_url(&profile.instance);
        Ok(Client {
            http,
            base_url,
            username: profile.username.clone(),
            password: profile.password.clone(),
            retry: self.retry,
        })
    }
}

fn normalize_base_url(instance: &str) -> String {
    if instance.starts_with("http://") || instance.starts_with("https://") {
        instance.trim_end_matches('/').to_string()
    } else {
        format!("https://{}", instance.trim_end_matches('/'))
    }
}

fn should_retry(status: StatusCode) -> bool {
    status.as_u16() == 429 || matches!(status.as_u16(), 502..=504)
}

fn jittered(d: Duration) -> Duration {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|t| t.subsec_nanos())
        .unwrap_or(0) as u64;
    let jitter_ms = nanos % 250;
    d + Duration::from_millis(jitter_ms)
}

fn execute_with_retry<F>(policy: RetryPolicy, mut send: F) -> Result<Value>
where
    F: FnMut() -> std::result::Result<Response, reqwest::Error>,
{
    let mut attempt: u32 = 0;
    let mut backoff = policy.initial_backoff;
    loop {
        attempt += 1;
        match send() {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    return resp
                        .json::<Value>()
                        .map_err(|e| Error::Transport(format!("parse response: {e}")));
                }
                let retryable =
                    policy.enabled && should_retry(status) && attempt < policy.max_attempts;
                if !retryable {
                    let tx = transaction_id(&resp);
                    return Err(from_http(status, tx, resp));
                }
                std::thread::sleep(jittered(backoff));
                backoff = backoff.saturating_mul(2);
            }
            Err(e) => return Err(Error::Transport(format!("{e}"))),
        }
    }
}

fn execute_no_body_with_retry<F>(policy: RetryPolicy, mut send: F) -> Result<()>
where
    F: FnMut() -> std::result::Result<Response, reqwest::Error>,
{
    let mut attempt: u32 = 0;
    let mut backoff = policy.initial_backoff;
    loop {
        attempt += 1;
        match send() {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    return Ok(());
                }
                let retryable =
                    policy.enabled && should_retry(status) && attempt < policy.max_attempts;
                if !retryable {
                    let tx = transaction_id(&resp);
                    return Err(from_http(status, tx, resp));
                }
                std::thread::sleep(jittered(backoff));
                backoff = backoff.saturating_mul(2);
            }
            Err(e) => return Err(Error::Transport(format!("{e}"))),
        }
    }
}

impl Client {
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    pub fn get(&self, path: &str, query: &[(String, String)]) -> Result<Value> {
        let url = format!("{}{}", self.base_url, path);
        let http = self.http.clone();
        let user = self.username.clone();
        let pass = self.password.clone();
        let query = query.to_vec();
        execute_with_retry(self.retry, move || {
            http.request(Method::GET, &url)
                .basic_auth(&user, Some(&pass))
                .query(&query)
                .send()
        })
    }

    pub fn post(&self, path: &str, query: &[(String, String)], body: &Value) -> Result<Value> {
        let url = format!("{}{}", self.base_url, path);
        let http = self.http.clone();
        let user = self.username.clone();
        let pass = self.password.clone();
        let query = query.to_vec();
        let body = body.clone();
        execute_with_retry(self.retry, move || {
            http.request(Method::POST, &url)
                .basic_auth(&user, Some(&pass))
                .query(&query)
                .header(CONTENT_TYPE, "application/json")
                .json(&body)
                .send()
        })
    }

    pub fn put(&self, path: &str, query: &[(String, String)], body: &Value) -> Result<Value> {
        let url = format!("{}{}", self.base_url, path);
        let http = self.http.clone();
        let user = self.username.clone();
        let pass = self.password.clone();
        let query = query.to_vec();
        let body = body.clone();
        execute_with_retry(self.retry, move || {
            http.request(Method::PUT, &url)
                .basic_auth(&user, Some(&pass))
                .query(&query)
                .header(CONTENT_TYPE, "application/json")
                .json(&body)
                .send()
        })
    }

    pub fn patch(&self, path: &str, query: &[(String, String)], body: &Value) -> Result<Value> {
        let url = format!("{}{}", self.base_url, path);
        let http = self.http.clone();
        let user = self.username.clone();
        let pass = self.password.clone();
        let query = query.to_vec();
        let body = body.clone();
        execute_with_retry(self.retry, move || {
            http.request(Method::PATCH, &url)
                .basic_auth(&user, Some(&pass))
                .query(&query)
                .header(CONTENT_TYPE, "application/json")
                .json(&body)
                .send()
        })
    }

    pub fn delete(&self, path: &str, query: &[(String, String)]) -> Result<()> {
        let url = format!("{}{}", self.base_url, path);
        let http = self.http.clone();
        let user = self.username.clone();
        let pass = self.password.clone();
        let query = query.to_vec();
        execute_no_body_with_retry(self.retry, move || {
            http.request(Method::DELETE, &url)
                .basic_auth(&user, Some(&pass))
                .query(&query)
                .send()
        })
    }
}

fn transaction_id(resp: &Response) -> Option<String> {
    resp.headers()
        .get("X-Transaction-ID")
        .and_then(|v| v.to_str().ok())
        .map(ToString::to_string)
}

fn from_http(status: StatusCode, tx: Option<String>, resp: Response) -> Error {
    let body: Option<Value> = resp.json().ok();
    let (message, detail, sn_error) = body
        .as_ref()
        .and_then(|v| v.get("error"))
        .map(|err| {
            (
                err.get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("ServiceNow error")
                    .to_string(),
                err.get("detail")
                    .and_then(|d| d.as_str())
                    .map(ToString::to_string),
                Some(err.clone()),
            )
        })
        .unwrap_or_else(|| (format!("HTTP {status}"), None, None));
    match status.as_u16() {
        401 | 403 => Error::Auth {
            status: status.as_u16(),
            message,
            transaction_id: tx,
        },
        s => Error::Api {
            status: s,
            message,
            detail,
            transaction_id: tx,
            sn_error,
        },
    }
}
