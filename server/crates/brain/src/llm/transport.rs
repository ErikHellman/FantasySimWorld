//! How the client reaches the endpoint: a trait, the real HTTP implementation
//! and a scripted fake for tests.
//!
//! The transport sends one request and reads the whole body. It has no
//! timeout of its own: the client bounds every call, and dropping the future
//! abandons the request (the equivalent of aborting a `fetch`).

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::Value;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    /// in order; names as given
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

impl HttpRequest {
    /// The value of a header, by exact name.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    /// The body parsed as JSON (`Null` if there is none or it does not parse).
    pub fn json(&self) -> Value {
        self.body.as_deref().and_then(|body| serde_json::from_str(body).ok()).unwrap_or(Value::Null)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    /// decoded as UTF-8, invalid sequences replaced
    pub body: String,
}

impl HttpResponse {
    pub fn json(status: u16, body: &Value) -> Self {
        Self { status, headers: vec![("content-type".into(), "application/json".into())], body: body.to_string() }
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// The value of a header, by case-insensitive name.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
    }

    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// Sends one HTTP request. The error is a human-readable reason the request
/// never got an answer (refused connection, DNS, TLS, bad header).
pub trait LlmTransport: Send + Sync {
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, String>>;
}

/// The real transport: reqwest with rustls (aws-lc-rs) and the platform's roots.
#[derive(Debug, Clone)]
pub struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    pub fn new() -> Result<Self, String> {
        let client = reqwest::Client::builder().build().map_err(|error| describe(&error))?;
        Ok(Self { client })
    }

    pub fn from_client(client: reqwest::Client) -> Self {
        Self { client }
    }
}

/// An error with its causes, which is where reqwest keeps the useful part.
fn describe(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        if !text.contains(&cause_text) {
            text.push_str(": ");
            text.push_str(&cause_text);
        }
        source = cause.source();
    }
    text
}

impl LlmTransport for ReqwestTransport {
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, String>> {
        Box::pin(async move {
            let mut builder = match request.method {
                Method::Get => self.client.get(&request.url),
                Method::Post => self.client.post(&request.url),
            };
            for (name, value) in &request.headers {
                builder = builder.header(name, value);
            }
            if let Some(body) = request.body {
                builder = builder.body(body);
            }
            let response = builder.send().await.map_err(|error| describe(&error))?;
            let status = response.status().as_u16();
            let headers = response
                .headers()
                .iter()
                .map(|(name, value)| {
                    (name.as_str().to_string(), String::from_utf8_lossy(value.as_bytes()).into_owned())
                })
                .collect();
            let bytes = response.bytes().await.map_err(|error| describe(&error))?;
            Ok(HttpResponse { status, headers, body: String::from_utf8_lossy(&bytes).into_owned() })
        })
    }
}

type Handler = dyn Fn(HttpRequest) -> BoxFuture<'static, Result<HttpResponse, String>> + Send + Sync;

/// A transport that answers from a closure and remembers every request, for tests.
pub struct FakeTransport {
    handler: Box<Handler>,
    seen: Mutex<Vec<HttpRequest>>,
}

impl FakeTransport {
    pub fn new<F, Fut>(handler: F) -> Arc<Self>
    where
        F: Fn(HttpRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<HttpResponse, String>> + Send + 'static,
    {
        Arc::new(Self { handler: Box::new(move |request| Box::pin(handler(request))), seen: Mutex::new(Vec::new()) })
    }

    /// Answers every request with the same JSON body.
    pub fn always(status: u16, body: Value) -> Arc<Self> {
        Self::new(move |_| {
            let response = HttpResponse::json(status, &body);
            async move { Ok(response) }
        })
    }

    /// Every request so far, oldest first.
    pub fn requests(&self) -> Vec<HttpRequest> {
        self.seen.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// The JSON bodies of every request so far.
    pub fn bodies(&self) -> Vec<Value> {
        self.requests().iter().map(HttpRequest::json).collect()
    }
}

impl LlmTransport for FakeTransport {
    fn send(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, String>> {
        self.seen.lock().unwrap_or_else(PoisonError::into_inner).push(request.clone());
        (self.handler)(request)
    }
}
