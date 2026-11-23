//! Discord browser authorization for account identity and server membership.
mod callback;

use crate::model::GuildSummary;
use crate::transport::discord::guild_to_summary;
use aws_lc_rs::{
    digest::{SHA256, digest},
    rand::{SecureRandom, SystemRandom},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use color_eyre::eyre::{Context, Result, bail, eyre};
use reqwest::{Client, Response, Url};
use serde::{Deserialize, de::DeserializeOwned};
use std::time::Duration;
use zeroize::{Zeroize, Zeroizing};

const CLIENT_ID: &str = "1573648525403889684";
const SERVICE: &str = "disctui-browser-oauth";
const AUTH_RESPONSE_LIMIT: usize = 64 * 1024;
const TOKEN_URL: &str = "https://discord.com/api/oauth2/token";

#[derive(Clone, Copy, Debug)]
pub enum Operation {
    Restore,
    Authorize,
    SignOut,
}

#[derive(Deserialize)]
struct Tokens {
    access_token: String,
    refresh_token: String,
}

#[derive(Deserialize)]
struct User {
    username: String,
}

impl Drop for Tokens {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
    }
}

async fn json<T: DeserializeOwned>(mut response: Response, limit: usize) -> Result<T> {
    let mut bytes = Zeroizing::new(Vec::new());
    while let Some(chunk) = response.chunk().await? {
        if bytes.len().saturating_add(chunk.len()) > limit {
            bail!("Discord response was too large");
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| eyre!("Unexpected response from Discord"))
}

fn nonce() -> Result<String> {
    let mut bytes = [0u8; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| eyre!("System randomness unavailable"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

async fn authorize(
    client: &Client,
    mut cancel: tokio::sync::oneshot::Receiver<()>,
) -> Result<Option<Tokens>> {
    if !matches!(
        cancel.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ) {
        return Ok(None);
    }
    let listener = callback::bind()
        .await
        .wrap_err("Could not open the browser callback on port 38547")?;
    let verifier = Zeroizing::new(nonce()?);
    let state = nonce()?;
    let challenge = URL_SAFE_NO_PAD.encode(digest(&SHA256, verifier.as_bytes()));
    let mut url = Url::parse("https://discord.com/oauth2/authorize")?;
    url.query_pairs_mut().extend_pairs([
        ("client_id", CLIENT_ID),
        ("response_type", "code"),
        ("scope", "identify guilds"),
        ("redirect_uri", callback::REDIRECT),
        ("state", &state),
        ("code_challenge_method", "S256"),
        ("code_challenge", &challenge),
    ]);
    let status = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new("/usr/bin/open")
            .arg(url.as_str())
            .kill_on_drop(true)
            .status(),
    )
    .await??;
    if !status.success() {
        bail!("macOS could not open your browser");
    }
    let code = tokio::select! {
        result = callback::wait(listener, &state) => result?,
        _ = cancel => return Ok(None),
    };
    let Some(code) = code else { return Ok(None) };
    let response = client
        .post(TOKEN_URL)
        .form(&[
            ("client_id", CLIENT_ID),
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("code_verifier", &verifier),
            ("redirect_uri", callback::REDIRECT),
        ])
        .send()
        .await?
        .error_for_status()?;
    Ok(Some(json(response, AUTH_RESPONSE_LIMIT).await?))
}

fn entry() -> Result<keyring::Entry> {
    Ok(keyring::Entry::new(SERVICE, CLIENT_ID)?)
}

fn saved() -> Result<Option<Zeroizing<String>>> {
    match entry()?.get_password() {
        Ok(token) => Ok(Some(Zeroizing::new(token))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn forget() -> Result<()> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(err.into()),
    }
}

async fn restore(client: &Client, saved: &str) -> Result<Option<Tokens>> {
    let response = client
        .post(TOKEN_URL)
        .form(&[
            ("client_id", CLIENT_ID),
            ("grant_type", "refresh_token"),
            ("refresh_token", saved),
        ])
        .send()
        .await?;
    let status = response.status();
    if status == reqwest::StatusCode::BAD_REQUEST {
        #[derive(Deserialize)]
        struct ErrorBody {
            error: String,
        }
        if json::<ErrorBody>(response, AUTH_RESPONSE_LIMIT)
            .await?
            .error
            == "invalid_grant"
        {
            tokio::task::spawn_blocking(forget).await??;
            return Ok(None);
        }
        bail!("Discord rejected the refresh request (HTTP {status})");
    }
    Ok(Some(
        json(response.error_for_status()?, AUTH_RESPONSE_LIMIT).await?,
    ))
}

pub enum Outcome {
    Ready,
    Account(String, Vec<GuildSummary>),
    NeedsConsent,
    LocalSignOut,
}

/// Forget the saved grant, then ask Discord to revoke it.
async fn sign_out(client: &Client, token: Option<Zeroizing<String>>) -> Result<Outcome> {
    tokio::task::spawn_blocking(forget).await??;
    let Some(token) = token else {
        return Ok(Outcome::Ready);
    };
    let revoked = client
        .post("https://discord.com/api/oauth2/token/revoke")
        .form(&[
            ("client_id", CLIENT_ID),
            ("token", token.as_str()),
            ("token_type_hint", "refresh_token"),
        ])
        .send()
        .await
        .is_ok_and(|response| response.status().is_success());
    Ok(if revoked {
        Outcome::Ready
    } else {
        Outcome::LocalSignOut
    })
}

pub async fn run(op: Operation, cancel: tokio::sync::oneshot::Receiver<()>) -> Result<Outcome> {
    // Refresh tokens rotate. Concurrent TUI instances must not redeem the same grant.
    let dir = dirs::cache_dir()
        .ok_or_else(|| eyre!("No cache directory"))?
        .join("disctui");
    std::fs::create_dir_all(&dir)?;
    let lock = std::fs::File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("oauth.lock"))?;
    lock.try_lock()
        .wrap_err("Another disctui sign-in is in progress")?;
    let token = tokio::task::spawn_blocking(saved).await??;
    let client = crate::auth::http_client(Duration::from_secs(20))?;
    let tokens = match op {
        Operation::SignOut => return sign_out(&client, token).await,
        // Enter is explicit consent. Refreshing an old grant cannot expand its scopes.
        Operation::Authorize => authorize(&client, cancel).await?,
        Operation::Restore => match token {
            Some(token) => restore(&client, &token).await?,
            None => None,
        },
    };
    let Some(mut tokens) = tokens else {
        return Ok(Outcome::Ready);
    };
    if tokens.access_token.is_empty() || tokens.refresh_token.is_empty() {
        bail!("Discord returned an incomplete account session");
    }
    let refresh = Zeroizing::new(std::mem::take(&mut tokens.refresh_token));
    tokio::task::spawn_blocking(move || -> Result<()> { Ok(entry()?.set_password(&refresh)?) })
        .await??;
    let response = client
        .get("https://discord.com/api/v10/users/@me")
        .bearer_auth(&tokens.access_token)
        .send()
        .await?
        .error_for_status()?;
    let name = json::<User>(response, AUTH_RESPONSE_LIMIT).await?.username;
    let name: String = name.chars().filter(|c| !c.is_control()).take(64).collect();
    if name.is_empty() {
        bail!("Discord returned an empty account name");
    }
    let guilds = client
        .get("https://discord.com/api/v10/users/@me/guilds")
        .bearer_auth(&tokens.access_token)
        .send()
        .await?;
    account_from_guilds(name, guilds).await
}

#[derive(Deserialize)]
struct Guild {
    id: String,
    name: String,
    icon: Option<String>,
}

async fn account_from_guilds(name: String, response: Response) -> Result<Outcome> {
    if matches!(
        response.status(),
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
    ) {
        return Ok(Outcome::NeedsConsent);
    }
    // Discord returns up to 200 partial guilds, including feature and permission fields.
    let guilds: Vec<Guild> = json(response.error_for_status()?, 512 * 1024).await?;
    if guilds.len() > 200 {
        bail!("Discord returned too many servers");
    }
    let summaries = guilds
        .into_iter()
        .map(|guild| guild_to_summary(&guild.id, &guild.name, guild.icon.as_deref()))
        .collect();
    Ok(Outcome::Account(name, summaries))
}

#[cfg(test)]
mod tests {
    use crate::oauth::{Outcome, account_from_guilds};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    #[tokio::test]
    #[expect(clippy::unwrap_used, reason = "local HTTP fixture must succeed")]
    async fn guild_endpoint_maps_membership_and_distinguishes_consent_from_service_failure() {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let endpoint = format!("http://{}/users/@me/guilds", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            for (status, body) in [
                ("401 Unauthorized", ""),
                ("403 Forbidden", ""),
                ("503 Service Unavailable", ""),
                (
                    "200 OK",
                    r#"[{"id":"1","name":"Alpha","icon":"hash"},{"id":"2","name":"Beta","icon":null}]"#,
                ),
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut reader = BufReader::new(&mut stream);
                let mut line = String::new();
                loop {
                    line.clear();
                    let n = reader.read_line(&mut line).await.unwrap();
                    assert_ne!(n, 0);
                    if line == "\r\n" {
                        break;
                    }
                }
                let reply = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(reply.as_bytes()).await.unwrap();
            }
        });
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();
        let fetch = || async {
            account_from_guilds("pilot".into(), client.get(&endpoint).send().await.unwrap()).await
        };
        assert!(matches!(fetch().await.unwrap(), Outcome::NeedsConsent));
        assert!(matches!(fetch().await.unwrap(), Outcome::NeedsConsent));
        assert!(fetch().await.is_err());
        let Outcome::Account(name, guilds) = fetch().await.unwrap() else {
            unreachable!("expected a signed-in account")
        };
        let [alpha, beta] = guilds.as_slice() else {
            unreachable!("expected two memberships")
        };
        assert_eq!(name, "pilot");
        assert_eq!((&*alpha.id, &*alpha.name), ("1", "Alpha"));
        assert_eq!(
            alpha.avatar_url.as_deref(),
            Some("https://cdn.discordapp.com/icons/1/hash.png?size=64")
        );
        assert_eq!(beta.avatar_url, None);
        assert!(
            guilds
                .iter()
                .all(|guild| !guild.unread && guild.unread_count == 0)
        );
        server.await.unwrap();
    }
}
