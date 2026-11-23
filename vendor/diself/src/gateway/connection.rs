use crate::error::{Error, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::handshake::client::Request;
use tokio_tungstenite::tungstenite::http::header::{ORIGIN, USER_AGENT};
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

const WEB_ORIGIN: &str = "https://discord.com";

pub struct Connection {
    pub ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl Connection {
    /// Connects to the Discord Gateway.
    ///
    /// `web_user_agent` is the user agent of the web client that created the
    /// session; with it the handshake carries that client's origin and agent.
    pub async fn connect(url: &str, web_user_agent: Option<&str>) -> Result<Self> {
        tracing::info!("Connecting to Discord Gateway at {}", url);

        let request = handshake_request(url, web_user_agent)?;
        let (ws, _response) = connect_async(request)
            .await
            .map_err(|e| Error::GatewayConnection(e.to_string()))?;

        tracing::info!("Successfully connected!");
        Ok(Self { ws })
    }

    pub async fn receive(&mut self) -> Result<Option<Value>> {
        while let Some(msg) = self.ws.next().await {
            let msg = msg?;

            match msg {
                Message::Text(text) => {
                    let payload: Value = serde_json::from_str(&text)?;
                    let redacted = redact_gateway_payload(&payload);
                    let redacted_text = serde_json::to_string(&redacted)?;
                    tracing::debug!("Received: {}", redacted_text);
                    return Ok(Some(payload));
                }
                Message::Close(frame) => {
                    tracing::warn!("WebSocket closed: {:?}", frame);
                    return closed(frame.map(|frame| u16::from(frame.code)));
                }
                _ => {
                    //ignore other message types (binary, ping, pong)
                    continue;
                }
            }
        }
        Ok(None)
    }

    pub async fn send(&mut self, payload: &Value) -> Result<()> {
        let text = serde_json::to_string(payload)?;
        let redacted = redact_gateway_payload(payload);
        let redacted_text = serde_json::to_string(&redacted)?;
        tracing::debug!("Sending: {}", redacted_text);

        self.ws.send(Message::Text(text)).await?;
        Ok(())
    }

    pub async fn close(&mut self) -> Result<()> {
        self.ws.close(None).await?;
        Ok(())
    }
}

fn handshake_request(url: &str, web_user_agent: Option<&str>) -> Result<Request> {
    let mut request = url
        .into_client_request()
        .map_err(|e| Error::GatewayConnection(e.to_string()))?;
    if let Some(user_agent) = web_user_agent {
        let user_agent = HeaderValue::from_str(user_agent)
            .map_err(|e| Error::GatewayConnection(e.to_string()))?;
        let headers = request.headers_mut();
        headers.insert(ORIGIN, HeaderValue::from_static(WEB_ORIGIN));
        headers.insert(USER_AGENT, user_agent);
    }
    Ok(request)
}

fn closed(code: Option<u16>) -> Result<Option<Value>> {
    if code == Some(4004) {
        return Err(Error::GatewayAuthenticationFailed);
    }
    Ok(None)
}

fn redact_gateway_payload(payload: &Value) -> Value {
    let mut out = payload.clone();
    redact_sensitive_keys(&mut out);
    out
}

fn redact_sensitive_keys(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                let lower = key.to_ascii_lowercase();
                if lower == "token"
                    || lower == "access_token"
                    || lower == "refresh_token"
                    || lower.ends_with("_token")
                {
                    *child = Value::String("[REDACTED]".to_string());
                } else {
                    redact_sensitive_keys(child);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_sensitive_keys(item);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use crate::gateway::connection::redact_gateway_payload;
    use serde_json::json;

    #[test]
    fn redact_gateway_payload_masks_tokens_recursively() {
        let payload = json!({
            "op": 0,
            "d": {
                "token": "abc",
                "nested": {
                    "access_token": "def"
                },
                "items": [
                    {
                        "refresh_token": "ghi"
                    }
                ]
            }
        });

        let redacted = redact_gateway_payload(&payload);
        assert_eq!(redacted["d"]["token"], "[REDACTED]");
        assert_eq!(redacted["d"]["nested"]["access_token"], "[REDACTED]");
        assert_eq!(redacted["d"]["items"][0]["refresh_token"], "[REDACTED]");
    }
}

#[cfg(test)]
mod handshake_tests {
    use crate::gateway::connection::handshake_request;

    const URL: &str = "wss://gateway.discord.gg/?v=10&encoding=json";

    #[test]
    fn web_session_handshake_carries_the_web_origin_and_user_agent() {
        let request = handshake_request(URL, Some("Mozilla/5.0 fixture")).expect("request");
        let header = |name: &str| {
            request
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
        };
        assert_eq!(header("origin"), Some("https://discord.com"));
        assert_eq!(header("user-agent"), Some("Mozilla/5.0 fixture"));
    }

    #[test]
    fn application_handshake_claims_no_browser() {
        let request = handshake_request(URL, None).expect("request");
        assert!(request.headers().get("origin").is_none());
        assert!(request.headers().get("user-agent").is_none());
    }

    #[test]
    fn user_agent_that_cannot_be_a_header_is_rejected() {
        assert!(handshake_request(URL, Some("line\nbreak")).is_err());
    }
}

#[cfg(test)]
mod close_code_tests {
    use crate::error::Error;
    use crate::gateway::connection::closed;

    #[test]
    fn authentication_failure_stops_gateway_reconnection() {
        assert!(matches!(
            closed(Some(4004)),
            Err(Error::GatewayAuthenticationFailed)
        ));
        assert!(matches!(closed(Some(1000)), Ok(None)));
    }
}
