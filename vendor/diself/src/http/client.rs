use crate::error::{CaptchaInfo, Error, Result};
use rand::RngCore;
use reqwest::{Client as ReqwestClient, Method, RequestBuilder, StatusCode};
use serde::Serialize;
use serde_json::Value;
use std::fmt::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Type for captcha handler callback
/// Takes captcha info and returns the solved captcha key
pub type CaptchaHandler = Arc<
    dyn Fn(
            CaptchaInfo,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String>> + Send>>
        + Send
        + Sync,
>;

/// Minimal HTTP client for Discord API
#[derive(Clone)]
pub struct HttpClient {
    token: String,
    client: ReqwestClient,
    captcha_handler: Option<CaptchaHandler>,
    heartbeat_session: Arc<parking_lot::RwLock<HeartbeatSession>>,
    request_delay: Duration,
    max_rate_limit_retries: u32,
}

#[derive(Debug)]
struct HeartbeatSession {
    id: String,
    created_at: Instant,
}

impl HttpClient {
    const HEARTBEAT_SESSION_TTL: Duration = Duration::from_secs(30 * 60);

    /// Creates a new HTTP client
    pub fn new(token: impl Into<String>) -> Result<Self> {
        let client = ReqwestClient::builder()
            .timeout(Duration::from_secs(10))
            .gzip(false)
            .referer(false)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(Error::Http)?;

        Ok(Self {
            token: token.into(),
            client,
            captcha_handler: None,
            heartbeat_session: Arc::new(parking_lot::RwLock::new(HeartbeatSession {
                id: generate_uuid_v4_like(),
                created_at: Instant::now(),
            })),
            request_delay: Duration::from_millis(100),
            max_rate_limit_retries: 3,
        })
    }

    /// Sets a captcha handler for this HTTP client
    pub fn with_captcha_handler<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(CaptchaInfo) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<String>> + Send + 'static,
    {
        self.captcha_handler = Some(Arc::new(move |info| Box::pin(handler(info))));
        self
    }

    /// Sets the delay between requests (default 100ms)
    pub fn with_request_delay(mut self, delay: Duration) -> Self {
        self.request_delay = delay;
        self
    }

    /// Sets the maximum number of rate limit retries (default 3)
    pub fn with_max_rate_limit_retries(mut self, n: u32) -> Self {
        self.max_rate_limit_retries = n;
        self
    }

    /// Returns the current analytics heartbeat session id.
    ///
    /// The id rotates automatically every 30 minutes.
    pub fn analytics_heartbeat_session_id(&self) -> String {
        self.rotate_heartbeat_session_if_needed()
    }

    /// Sends a GET request
    pub async fn get(&self, url: impl AsRef<str>) -> Result<Value> {
        self.request(Method::GET, url.as_ref(), None::<&()>).await
    }

    /// Sends a POST request
    pub async fn post<T: Serialize>(&self, url: impl AsRef<str>, body: T) -> Result<Value> {
        self.request(Method::POST, url.as_ref(), Some(&body)).await
    }

    /// Sends a PATCH request
    pub async fn patch<T: Serialize>(&self, url: impl AsRef<str>, body: T) -> Result<Value> {
        self.request(Method::PATCH, url.as_ref(), Some(&body)).await
    }

    /// Sends a PUT request
    pub async fn put<T: Serialize>(&self, url: impl AsRef<str>, body: T) -> Result<Value> {
        self.request(Method::PUT, url.as_ref(), Some(&body)).await
    }

    /// Sends a DELETE request
    pub async fn delete(&self, url: impl AsRef<str>) -> Result<Value> {
        self.request(Method::DELETE, url.as_ref(), None::<&()>)
            .await
    }

    /// Generic HTTP request
    async fn request<T: Serialize>(
        &self,
        method: Method,
        url: &str,
        body: Option<&T>,
    ) -> Result<Value> {
        self.request_with_captcha(method, url, body, None).await
    }

    /// Build an authenticated request for the caller-supplied Discord API URL.
    fn build_request(&self, method: Method, url: &str) -> RequestBuilder {
        self.client
            .request(method, url)
            .header("Authorization", &self.token)
    }

    /// Generic HTTP request with optional captcha key
    async fn request_with_captcha<T: Serialize>(
        &self,
        method: Method,
        url: &str,
        body: Option<&T>,
        captcha_key: Option<String>,
    ) -> Result<Value> {
        // Add a small delay to mimic human behavior (anti-bot measure)
        tokio::time::sleep(self.request_delay).await;

        let mut request = self.build_request(method.clone(), url);

        // Prepare body with captcha key if provided
        if let Some(body) = body {
            let mut json_body = serde_json::to_value(body)?;
            if let Some(ref key) = captcha_key {
                if let Some(obj) = json_body.as_object_mut() {
                    obj.insert("captcha_key".to_string(), Value::String(key.clone()));
                }
            }
            request = request.json(&json_body);
        } else if let Some(key) = captcha_key {
            request = request.json(&serde_json::json!({ "captcha_key": key }));
        }

        let response = request.send().await?;

        // Handle response, check for rate limits with retry
        let result = self.handle_response(response).await;

        match result {
            Err(Error::RateLimit { retry_after }) => {
                self.retry_rate_limited(method, url, body, None, retry_after)
                    .await
            }
            Err(Error::CaptchaRequired(captcha_info)) => {
                self.handle_captcha(method, url, body, captcha_info).await
            }
            other => other,
        }
    }

    /// Handles captcha solving and retry
    async fn handle_captcha<T: Serialize>(
        &self,
        method: Method,
        url: &str,
        body: Option<&T>,
        captcha_info: CaptchaInfo,
    ) -> Result<Value> {
        if let Some(ref handler) = self.captcha_handler {
            tracing::info!("Captcha required, calling handler...");
            let session_id = captcha_info.captcha_session_id.clone();
            let rqtoken = captcha_info.captcha_rqtoken.clone();
            let solved_key = handler(captcha_info).await?;
            tracing::info!("Captcha solved, retrying request...");

            let body_json = if let Some(b) = body {
                Some(serde_json::to_value(b)?)
            } else {
                None
            };

            Box::pin(self.request_with_captcha_value(
                method,
                url,
                body_json,
                Some(solved_key),
                session_id,
                rqtoken,
            ))
            .await
        } else {
            Err(Error::CaptchaRequired(captcha_info))
        }
    }

    /// Retries a request after a rate limit, up to max_rate_limit_retries
    async fn retry_rate_limited<T: Serialize>(
        &self,
        method: Method,
        url: &str,
        body: Option<&T>,
        captcha_key: Option<String>,
        initial_retry_after: f64,
    ) -> Result<Value> {
        let mut retry_after = initial_retry_after;
        for attempt in 0..self.max_rate_limit_retries {
            tracing::warn!(
                "Rate limited, retrying in {:.2}s (attempt {}/{})",
                retry_after,
                attempt + 1,
                self.max_rate_limit_retries
            );
            tokio::time::sleep(Duration::from_secs_f64(retry_after)).await;
            tokio::time::sleep(self.request_delay).await;

            let mut request = self.build_request(method.clone(), url);

            if let Some(body) = body {
                let mut json_body = serde_json::to_value(body)?;
                if let Some(ref key) = captcha_key {
                    if let Some(obj) = json_body.as_object_mut() {
                        obj.insert("captcha_key".to_string(), Value::String(key.clone()));
                    }
                }
                request = request.json(&json_body);
            } else if let Some(ref key) = captcha_key {
                request = request.json(&serde_json::json!({ "captcha_key": key }));
            }

            let response = request.send().await?;
            match self.handle_response(response).await {
                Err(Error::RateLimit { retry_after: ra }) => {
                    retry_after = ra;
                    continue;
                }
                other => return other,
            }
        }

        Err(Error::RateLimit { retry_after })
    }

    /// Helper for recursion with owned values (captcha retry)
    async fn request_with_captcha_value(
        &self,
        method: Method,
        url: &str,
        body: Option<Value>,
        captcha_key: Option<String>,
        captcha_session_id: Option<String>,
        captcha_rqtoken: Option<String>,
    ) -> Result<Value> {
        let mut request = self.build_request(method, url);

        if let Some(ref key) = captcha_key {
            request = request.header("X-Captcha-Key", key.as_str());
        }
        if let Some(session_id) = captcha_session_id {
            request = request.header("X-Captcha-Session-Id", session_id);
        }
        if let Some(rqtoken) = captcha_rqtoken {
            request = request.header("X-Captcha-RqToken", rqtoken);
        }

        // Prepare body with captcha key if provided
        if let Some(mut json_body) = body {
            if let Some(ref key) = captcha_key {
                if let Some(obj) = json_body.as_object_mut() {
                    obj.insert("captcha_key".to_string(), Value::String(key.clone()));
                }
            }
            request = request.json(&json_body);
        } else if let Some(key) = captcha_key {
            request = request.json(&serde_json::json!({ "captcha_key": key }));
        }

        let response = request.send().await?;
        self.handle_response(response).await
    }

    /// Handles HTTP response
    async fn handle_response(&self, response: reqwest::Response) -> Result<Value> {
        let status = response.status();

        if status.is_success() {
            if status == StatusCode::NO_CONTENT {
                return Ok(Value::Null);
            }

            let json = response.json::<Value>().await?;
            Ok(json)
        } else if status == StatusCode::TOO_MANY_REQUESTS {
            let json = response.json::<Value>().await?;
            let retry_after = json["retry_after"].as_f64().unwrap_or(1.0);
            Err(Error::RateLimit { retry_after })
        } else if status == StatusCode::BAD_REQUEST {
            let json = response.json::<Value>().await?;

            if json.get("captcha_sitekey").is_some() {
                match serde_json::from_value::<CaptchaInfo>(json.clone()) {
                    Ok(captcha_info) => Err(Error::CaptchaRequired(captcha_info)),
                    Err(_) => Err(Error::Api {
                        status: status.as_u16(),
                        body: json.to_string(),
                    }),
                }
            } else {
                Err(Error::Api {
                    status: status.as_u16(),
                    body: json.to_string(),
                })
            }
        } else {
            let text = response.text().await.unwrap_or_default();
            Err(Error::Api {
                status: status.as_u16(),
                body: text,
            })
        }
    }

    fn rotate_heartbeat_session_if_needed(&self) -> String {
        {
            let session = self.heartbeat_session.read();
            if session.created_at.elapsed() < Self::HEARTBEAT_SESSION_TTL {
                return session.id.clone();
            }
        }

        let mut session = self.heartbeat_session.write();
        if session.created_at.elapsed() >= Self::HEARTBEAT_SESSION_TTL {
            session.id = generate_uuid_v4_like();
            session.created_at = Instant::now();
        }
        session.id.clone()
    }
}

fn generate_uuid_v4_like() -> String {
    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);

    // RFC4122 variant + version 4 bits.
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    let mut out = String::with_capacity(36);
    for (i, b) in bytes.iter().enumerate() {
        let _ = write!(&mut out, "{:02x}", b);
        if i == 3 || i == 5 || i == 7 || i == 9 {
            out.push('-');
        }
    }
    out
}

#[cfg(test)]
mod request_profile_tests {
    use crate::http::client::{HeartbeatSession, HttpClient};
    use reqwest::{Client, Method};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    #[test]
    fn request_profile_contains_only_the_authentication_header() {
        // This checks the produced request without accessing macOS proxy services.
        let client = HttpClient {
            token: "fixture".into(),
            client: Client::builder().no_proxy().build().expect("client"),
            captcha_handler: None,
            heartbeat_session: Arc::new(parking_lot::RwLock::new(HeartbeatSession {
                id: "fixture".into(),
                created_at: Instant::now(),
            })),
            request_delay: Duration::ZERO,
            max_rate_limit_retries: 0,
        };
        let request = client
            .build_request(Method::GET, "https://discord.com/api/v10/users/@me")
            .build()
            .expect("request");
        assert_eq!(request.headers().len(), 1);
        assert_eq!(
            request
                .headers()
                .get("authorization")
                .expect("authorization"),
            "fixture"
        );
    }
}
