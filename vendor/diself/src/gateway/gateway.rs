use crate::error::{Error, Result};
use crate::gateway::{Connection, Identify};
use rand::Rng;
use serde_json::{json, Value};
use tokio::time::{self, Duration, Instant, Interval};

const DEFAULT_GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=10&encoding=json";
const INVALID_SESSION_RETRY_DELAY: Duration = Duration::from_secs(1);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_RECONNECT_BACKOFF: Duration = Duration::from_secs(60);

pub struct Gateway {
    token: String,
    client_properties: Option<Value>,
    connection: Option<Connection>,
    heartbeat: Option<Interval>,
    awaiting_heartbeat_ack: bool,
    pending_heartbeat: bool,
    sequence: Option<u64>,
    session_id: Option<String>,
    resume_gateway_url: Option<String>,
    reconnect_attempts: u32,
    max_reconnect_attempts: u32,
}

impl Gateway {
    /// Opens a Gateway session for `token`.
    ///
    /// `client_properties` are the properties of the client that created the
    /// session. Discord revokes a session whose client changes, so a session
    /// created in a browser must keep presenting that browser's properties.
    pub async fn connect(
        token: impl Into<String>,
        client_properties: Option<Value>,
    ) -> Result<Self> {
        let mut gateway = Self {
            token: token.into(),
            client_properties,
            connection: None,
            heartbeat: None,
            awaiting_heartbeat_ack: false,
            pending_heartbeat: false,
            sequence: None,
            session_id: None,
            resume_gateway_url: None,
            reconnect_attempts: 0,
            max_reconnect_attempts: 10,
        };

        gateway.reconnect(true).await?;
        Ok(gateway)
    }

    pub async fn next_event(&mut self) -> Result<Option<Value>> {
        loop {
            if self.pending_heartbeat {
                self.send_heartbeat().await?;
                self.pending_heartbeat = false;
            }

            let heartbeat = self.heartbeat.as_mut().ok_or(Error::InvalidPayload)?;
            let connection = self.connection.as_mut().ok_or(Error::InvalidPayload)?;

            tokio::select! {
                _ = heartbeat.tick() => {
                    if self.awaiting_heartbeat_ack {
                        tracing::warn!("Heartbeat ACK timeout, reconnecting gateway");
                        self.reconnect(true).await?;
                        continue;
                    }
                    self.pending_heartbeat = true;
                }
                payload = connection.receive() => {
                    let Some(payload) = payload? else {
                        tracing::warn!("Gateway connection closed, reconnecting");
                        self.reconnect(true).await?;
                        continue;
                    };

                    if let Some(seq) = payload.get("s").and_then(|s| s.as_u64()) {
                        self.sequence = Some(seq);
                    }

                    if let Some(next) = self.handle_control_opcode(&payload).await? {
                        return Ok(Some(next));
                    }
                }
            }
        }
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        self.awaiting_heartbeat_ack = false;
        self.pending_heartbeat = false;
        self.heartbeat = None;

        if let Some(mut connection) = self.connection.take() {
            connection.close().await?;
        }

        Ok(())
    }

    async fn handle_control_opcode(&mut self, payload: &Value) -> Result<Option<Value>> {
        let op = payload.get("op").and_then(|op| op.as_u64());

        match op {
            Some(0) => {
                if let Some(event_type) = payload.get("t").and_then(|t| t.as_str()) {
                    match event_type {
                        "READY" => {
                            self.reconnect_attempts = 0;
                            self.session_id =
                                payload["d"]["session_id"].as_str().map(ToOwned::to_owned);
                            self.resume_gateway_url = payload["d"]["resume_gateway_url"]
                                .as_str()
                                .map(|url| format!("{url}/?v=10&encoding=json"));
                            tracing::info!(
                                "Gateway READY received (session resumable: {})",
                                self.can_resume()
                            );
                        }
                        "RESUMED" => {
                            self.reconnect_attempts = 0;
                            tracing::info!("Gateway session resumed successfully");
                        }
                        _ => {}
                    }
                }
                Ok(Some(payload.clone()))
            }
            Some(1) => {
                self.pending_heartbeat = true;
                Ok(None)
            }
            Some(7) => {
                tracing::info!("Gateway requested reconnect");
                self.reconnect(true).await?;
                Ok(None)
            }
            Some(9) => {
                let can_resume = payload["d"].as_bool().unwrap_or(false);
                if !can_resume {
                    self.session_id = None;
                    self.sequence = None;
                }
                tracing::warn!(
                    "Received INVALID_SESSION (resumable: {}), reconnecting",
                    can_resume
                );
                time::sleep(INVALID_SESSION_RETRY_DELAY).await;
                self.reconnect(can_resume).await?;
                Ok(None)
            }
            Some(10) => {
                tracing::debug!("Received unexpected HELLO after handshake");
                Ok(None)
            }
            Some(11) => {
                self.awaiting_heartbeat_ack = false;
                tracing::trace!("Heartbeat ACK received");
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    /// Subscribes to typing, thread, and activity events for the guilds of a
    /// READY payload.
    pub async fn subscribe_to_guilds(&mut self, guilds: &[Value]) {
        let Some(conn) = self.connection.as_mut() else {
            return;
        };
        for guild in guilds {
            let Some(guild_id) = guild["id"].as_str() else {
                continue;
            };
            let op14 = json!({
                "op": 14,
                "d": {
                    "guild_id": guild_id,
                    "typing": true,
                    "threads": true,
                    "activities": true,
                }
            });
            if let Err(e) = conn.send(&op14).await {
                tracing::warn!("Failed to subscribe to guild {}: {}", guild_id, e);
            }
        }
    }

    async fn send_heartbeat(&mut self) -> Result<()> {
        let payload = json!({
            "op": 1,
            "d": self.sequence,
        });

        let connection = self.connection.as_mut().ok_or(Error::InvalidPayload)?;
        connection.send(&payload).await?;
        self.awaiting_heartbeat_ack = true;
        tracing::trace!("Heartbeat sent (seq: {:?})", self.sequence);
        Ok(())
    }

    async fn reconnect(&mut self, prefer_resume: bool) -> Result<()> {
        self.connection = None;
        self.heartbeat = None;
        self.awaiting_heartbeat_ack = false;
        self.pending_heartbeat = false;

        let mut use_resume = prefer_resume && self.can_resume();

        loop {
            let previous_attempts = self.reconnect_attempts;
            self.record_reconnect_attempt()?;
            if previous_attempts > 0 {
                let backoff = self.backoff_with_jitter(previous_attempts);
                tracing::warn!(
                    "Reconnect attempt {}/{} in {:?}",
                    self.reconnect_attempts,
                    self.max_reconnect_attempts,
                    backoff
                );
                time::sleep(backoff).await;
            }

            let opened = match time::timeout(HANDSHAKE_TIMEOUT, self.open_session(use_resume)).await
            {
                Ok(result) => result,
                Err(_) => Err(Error::GatewayConnection(
                    "gateway handshake timed out".into(),
                )),
            };
            match opened {
                Ok(()) => return Ok(()),
                Err(Error::GatewayAuthenticationFailed) => {
                    return Err(Error::GatewayAuthenticationFailed)
                }
                Err(err) => {
                    tracing::error!("Failed to reconnect gateway: {}", err);

                    if use_resume {
                        tracing::warn!("Resume failed, falling back to fresh IDENTIFY");
                        use_resume = false;
                    }
                }
            }
        }
    }

    fn record_reconnect_attempt(&mut self) -> Result<()> {
        if self.reconnect_attempts >= self.max_reconnect_attempts {
            return Err(Error::GatewayConnection(
                "max reconnect attempts exceeded".into(),
            ));
        }
        self.reconnect_attempts = self.reconnect_attempts.saturating_add(1);
        Ok(())
    }

    async fn open_session(&mut self, resume: bool) -> Result<()> {
        let url = self
            .resume_gateway_url
            .as_deref()
            .unwrap_or(DEFAULT_GATEWAY_URL);
        let web_user_agent = self
            .client_properties
            .as_ref()
            .and_then(|properties| properties.get("browser_user_agent"))
            .and_then(Value::as_str)
            .filter(|user_agent| !user_agent.is_empty());
        let mut connection = Connection::connect(url, web_user_agent).await?;

        let hello = connection.receive().await?.ok_or(Error::InvalidPayload)?;
        let heartbeat_interval = heartbeat_interval(&hello)?;
        // The first scheduled heartbeat runs one interval after HELLO.
        let first_heartbeat = Instant::now()
            .checked_add(heartbeat_interval)
            .ok_or(Error::InvalidPayload)?;
        let mut heartbeat = time::interval_at(first_heartbeat, heartbeat_interval);
        heartbeat.set_missed_tick_behavior(time::MissedTickBehavior::Delay);

        if resume {
            self.send_resume(&mut connection).await?;
        } else {
            self.send_identify(&mut connection).await?;
        }

        tracing::info!(
            "Gateway connected with heartbeat={}ms (resume={})",
            heartbeat_interval.as_millis(),
            resume
        );

        self.connection = Some(connection);
        self.heartbeat = Some(heartbeat);
        self.awaiting_heartbeat_ack = false;
        self.pending_heartbeat = false;
        Ok(())
    }

    async fn send_identify(&self, connection: &mut Connection) -> Result<()> {
        let identify_payload = json!({
            "op": 2,
            "d": Identify::new(self.token.clone(), self.client_properties.as_ref())?,
        });
        connection.send(&identify_payload).await
    }

    async fn send_resume(&self, connection: &mut Connection) -> Result<()> {
        let payload = json!({
            "op": 6,
            "d": {
                "token": self.token,
                "session_id": self.session_id,
                "seq": self.sequence,
            }
        });
        connection.send(&payload).await
    }

    fn can_resume(&self) -> bool {
        self.session_id.is_some() && self.sequence.is_some()
    }

    fn backoff_with_jitter(&self, attempt: u32) -> Duration {
        let capped = attempt.min(6);
        let base_secs = 2_u64.saturating_pow(capped);
        let base = Duration::from_secs(base_secs).min(MAX_RECONNECT_BACKOFF);

        let max_jitter_ms = (base.as_millis() / 5) as u64;
        let jitter_ms = if max_jitter_ms == 0 {
            0
        } else {
            rand::thread_rng().gen_range(0..=max_jitter_ms)
        };

        base + Duration::from_millis(jitter_ms)
    }
}

fn heartbeat_interval(hello: &Value) -> Result<Duration> {
    if hello.get("op").and_then(Value::as_u64) != Some(10) {
        return Err(Error::InvalidPayload);
    }
    let Some(milliseconds) = hello
        .get("d")
        .and_then(|data| data.get("heartbeat_interval"))
        .and_then(Value::as_u64)
        .filter(|value| (1000..=300_000).contains(value))
    else {
        return Err(Error::InvalidPayload);
    };
    Ok(Duration::from_millis(milliseconds))
}

#[cfg(test)]
mod tests {
    use crate::error::Error;
    use crate::gateway::gateway::{heartbeat_interval, Gateway};
    use serde_json::json;

    fn disconnected_gateway() -> Gateway {
        Gateway {
            token: "fixture".into(),
            client_properties: None,
            connection: None,
            heartbeat: None,
            awaiting_heartbeat_ack: false,
            pending_heartbeat: false,
            sequence: None,
            session_id: None,
            resume_gateway_url: None,
            reconnect_attempts: 0,
            max_reconnect_attempts: 2,
        }
    }

    #[tokio::test]
    async fn only_ready_or_resumed_replenishes_the_reconnect_budget() {
        let mut gateway = disconnected_gateway();
        assert!(gateway.record_reconnect_attempt().is_ok());
        assert!(gateway.record_reconnect_attempt().is_ok());
        assert!(matches!(
            gateway.record_reconnect_attempt(),
            Err(Error::GatewayConnection(_))
        ));
        assert!(gateway
            .handle_control_opcode(&json!({"op":0,"t":"READY","d":{"session_id":"fixture"}}))
            .await
            .is_ok());
        assert!(gateway.record_reconnect_attempt().is_ok());
        assert!(gateway.record_reconnect_attempt().is_ok());
        assert!(gateway
            .handle_control_opcode(&json!({"op":0,"t":"RESUMED","d":{}}))
            .await
            .is_ok());
        assert!(gateway.record_reconnect_attempt().is_ok());
    }

    #[test]
    fn invalid_heartbeat_values_are_rejected_before_timer_creation() {
        for value in [0, 999, 300_001, u64::MAX] {
            assert!(matches!(
                heartbeat_interval(&json!({"op":10,"d":{"heartbeat_interval":value}})),
                Err(Error::InvalidPayload)
            ));
        }
        assert!(heartbeat_interval(&json!({"op":10,"d":{"heartbeat_interval":41250}})).is_ok());
    }
}
