use crate::config::ResolvedProfile;
use crate::error::{Error, Result};
use reqwest::blocking::{Client as ReqwestClient, RequestBuilder, Response};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_TYPE, USER_AGENT};
use reqwest::{Method, StatusCode};
use serde_json::Value;
use std::time::Duration;

pub struct Client {
    http: ReqwestClient,
    base_url: String,
    username: String,
    password: String,
}

pub struct ClientBuilder {
    timeout: Duration,
    user_agent: String,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            user_agent: format!("sn/{}", env!("CARGO_PKG_VERSION")),
        }
    }
}

impl ClientBuilder {
    pub fn timeout(mut self, d: Duration) -> Self {
        self.timeout = d;
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

impl Client {
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        let url = format!("{}{}", self.base_url, path);
        self.http
            .request(method, url)
            .basic_auth(&self.username, Some(&self.password))
    }

    pub fn get(&self, path: &str, query: &[(String, String)]) -> Result<Value> {
        let resp = self
            .request(Method::GET, path)
            .query(query)
            .send()
            .map_err(|e| Error::Transport(format!("GET {path}: {e}")))?;
        parse_response(resp)
    }

    pub fn post(&self, path: &str, query: &[(String, String)], body: &Value) -> Result<Value> {
        let resp = self
            .request(Method::POST, path)
            .query(query)
            .header(CONTENT_TYPE, "application/json")
            .json(body)
            .send()
            .map_err(|e| Error::Transport(format!("POST {path}: {e}")))?;
        parse_response(resp)
    }

    pub fn put(&self, path: &str, query: &[(String, String)], body: &Value) -> Result<Value> {
        let resp = self
            .request(Method::PUT, path)
            .query(query)
            .header(CONTENT_TYPE, "application/json")
            .json(body)
            .send()
            .map_err(|e| Error::Transport(format!("PUT {path}: {e}")))?;
        parse_response(resp)
    }

    pub fn patch(&self, path: &str, query: &[(String, String)], body: &Value) -> Result<Value> {
        let resp = self
            .request(Method::PATCH, path)
            .query(query)
            .header(CONTENT_TYPE, "application/json")
            .json(body)
            .send()
            .map_err(|e| Error::Transport(format!("PATCH {path}: {e}")))?;
        parse_response(resp)
    }

    pub fn delete(&self, path: &str, query: &[(String, String)]) -> Result<()> {
        let resp = self
            .request(Method::DELETE, path)
            .query(query)
            .send()
            .map_err(|e| Error::Transport(format!("DELETE {path}: {e}")))?;
        let status = resp.status();
        let tx = transaction_id(&resp);
        if status.is_success() {
            Ok(())
        } else {
            Err(from_http(status, tx, resp))
        }
    }
}

fn transaction_id(resp: &Response) -> Option<String> {
    resp.headers()
        .get("X-Transaction-ID")
        .and_then(|v| v.to_str().ok())
        .map(ToString::to_string)
}

fn parse_response(resp: Response) -> Result<Value> {
    let status = resp.status();
    let tx = transaction_id(&resp);
    if status.is_success() {
        resp.json::<Value>()
            .map_err(|e| Error::Transport(format!("parse response: {e}")))
    } else {
        Err(from_http(status, tx, resp))
    }
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
