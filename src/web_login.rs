//! A fresh Discord web session whose user completes login in `WKWebView`.

use std::cell::Cell;
use std::io::{self, Write as _};
use std::time::{Duration, Instant};

use base64::Engine as _;
use color_eyre::eyre::{Result, bail, eyre};
use reqwest::Url;
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy},
    platform::run_return::EventLoopExtRunReturn,
    window::{Window, WindowBuilder},
};
use wry::WebViewBuilder;
use zeroize::Zeroizing;

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum LoginStage {
    Waiting,
    Verifying,
    Ready,
    Rejected,
}

#[derive(serde::Deserialize)]
struct Progress {
    stage: LoginStage,
}

#[derive(serde::Deserialize)]
struct ClientPropertiesMessage {
    client_properties: String,
}

enum LoginEvent {
    ClientProperties(serde_json::Value),
    Progress(LoginStage),
    Token(Zeroizing<String>),
}

/// A session created in the login window, with the client properties
/// Discord's page reported while creating it.
struct Captured {
    token: Zeroizing<String>,
    client_properties: Option<serde_json::Value>,
}

const MAX_CLIENT_PROPERTIES_BYTES: usize = 4096;
const MAX_CLIENT_PROPERTIES_FIELDS: usize = 64;

fn is_discord_origin(origin: &str) -> bool {
    Url::parse(origin).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("discord.com")
            && url.port_or_known_default() == Some(443)
    })
}

fn candidate(origin: &str, body: &str) -> Option<Zeroizing<String>> {
    if !is_discord_origin(origin) || body.len() > 4096 {
        return None;
    }
    let value = Zeroizing::new(serde_json::from_str::<String>(body).ok()?);
    if !(20..=2048).contains(&value.len())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
    {
        return None;
    }
    Some(value)
}

fn build_login_view(window: &Window, proxy: EventLoopProxy<LoginEvent>) -> Result<wry::WebView> {
    let accepted = Cell::new(false);
    Ok(WebViewBuilder::new()
        .with_incognito(true)
        .with_url("https://discord.com/login")
        .with_initialization_script(include_str!("web_login.js"))
        .with_ipc_handler(move |request| {
            if accepted.get() {
                return;
            }
            let origin = request.uri().to_string();
            if request.body().len() < 100
                && is_discord_origin(&origin)
                && let Ok(progress) = serde_json::from_str::<Progress>(request.body())
            {
                let _ = proxy.send_event(LoginEvent::Progress(progress.stage));
                return;
            }
            if let Some(properties) = client_properties(&origin, request.body()) {
                let _ = proxy.send_event(LoginEvent::ClientProperties(properties));
                return;
            }
            let Some(token) = candidate(&origin, request.body()) else {
                return;
            };
            accepted.set(true);
            let _ = proxy.send_event(LoginEvent::Token(token));
        })
        .build(window)?)
}

/// The `X-Super-Properties` value Discord's page sent: base64 of a flat JSON
/// object.
fn client_properties(origin: &str, body: &str) -> Option<serde_json::Value> {
    if !is_discord_origin(origin) || body.len() > MAX_CLIENT_PROPERTIES_BYTES * 2 {
        return None;
    }
    let message = serde_json::from_str::<ClientPropertiesMessage>(body).ok()?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(message.client_properties)
        .ok()?;
    if decoded.len() > MAX_CLIENT_PROPERTIES_BYTES {
        return None;
    }
    let properties = serde_json::from_slice::<serde_json::Value>(&decoded).ok()?;
    let fields = properties.as_object()?;
    let flat = fields.values().all(|value| match value {
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => true,
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => false,
    });
    if fields.is_empty() || fields.len() > MAX_CLIENT_PROPERTIES_FIELDS || !flat {
        return None;
    }
    Some(properties)
}

struct LoginWindow {
    view: wry::WebView,
    window: Window,
}

impl LoginWindow {
    fn open(events: &EventLoop<LoginEvent>) -> Result<Self> {
        let window = WindowBuilder::new()
            .with_title("Sign in to Discord — disctui")
            .with_inner_size(LogicalSize::new(1000, 760))
            .build(events)?;
        let view = build_login_view(&window, events.create_proxy())?;
        Ok(Self { view, window })
    }

    fn close(self) {
        self.window.set_visible(false);
        drop(self.view);
        drop(self.window);
    }
}

fn capture() -> Result<Captured> {
    let main = objc2::MainThreadMarker::new()
        .ok_or_else(|| eyre!("Discord web login must run on the main thread"))?;
    if objc2_app_kit::NSScreen::mainScreen(main).is_none() {
        bail!("No macOS display is available. Run disctui from your desktop terminal.");
    }
    let mut events = EventLoopBuilder::<LoginEvent>::with_user_event().build();
    let window = LoginWindow::open(&events)?;
    let mut deadline = Instant::now()
        .checked_add(Duration::from_secs(300))
        .ok_or_else(|| eyre!("Could not establish login deadline"))?;
    let window_id = window.window.id();
    let mut login_window = Some(window);
    let mut window_destroyed = false;
    let mut token = None;
    let mut client_properties = None;
    events.run_return(|event, _, control| {
        *control = ControlFlow::WaitUntil(deadline);
        if matches!(
            event,
            Event::WindowEvent {
                window_id: id,
                event: WindowEvent::Destroyed,
                ..
            } if id == window_id
        ) {
            window_destroyed = true;
            *control = ControlFlow::Exit;
            return;
        }
        let Some(window) = login_window.as_ref() else {
            if Instant::now() >= deadline {
                *control = ControlFlow::Exit;
            }
            return;
        };
        let mut finished = false;
        if let Event::UserEvent(LoginEvent::ClientProperties(properties)) = event {
            client_properties = Some(properties);
        } else if let Event::UserEvent(LoginEvent::Progress(stage)) = event {
            announce_stage(&window.window, &stage);
        } else if let Event::UserEvent(LoginEvent::Token(value)) = event {
            token = Some(value);
            finished = true;
        } else if matches!(
            event,
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            }
        ) || Instant::now() >= deadline
        {
            finished = true;
        }
        if finished {
            // AppKit must process the native close before the TUI takes over.
            if let Some(window) = login_window.take() {
                window.close();
            }
            let now = Instant::now();
            deadline = now.checked_add(Duration::from_secs(3)).unwrap_or(now);
            *control = ControlFlow::WaitUntil(deadline);
        }
    });
    if !window_destroyed {
        bail!("Discord login window did not finish closing");
    }
    let token = token.ok_or_else(|| eyre!("Discord web sign-in was cancelled or timed out"))?;
    Ok(Captured {
        token,
        client_properties,
    })
}

/// Run the sign-in window for a running TUI, reporting each stage on stdout.
///
/// The session is saved to the Keychain in this process. Nothing secret
/// reaches stdout, the command line, or the log; the parent reads the saved
/// session from the Keychain once this process exits cleanly.
pub async fn helper() -> Result<()> {
    let captured = capture()?;
    report_stage("verifying");
    validate_and_save(captured).await?;
    report_stage("saved");
    Ok(())
}

/// The longest plausible failure message kept from the helper's stderr.
const MAX_REASON_BYTES: usize = 300;

/// Drive the sign-in window in a child process, reporting progress to the TUI.
///
/// This runs as a task, the terminal keeps rendering, and the outcome arrives
/// as an action. Dropping the task kills the child and
/// closes its window.
pub async fn run_sign_in(tx: tokio::sync::mpsc::Sender<crate::action::Action>) {
    let outcome = match sign_in_child(&tx).await {
        Ok(()) => crate::action::Action::WebSignInSaved,
        Err(error) => crate::action::Action::WebSignInFailed(error.to_string()),
    };
    // Wait for room: the prompt has no other way to learn the outcome.
    let _ = tx.send(outcome).await;
}

async fn sign_in_child(tx: &tokio::sync::mpsc::Sender<crate::action::Action>) -> Result<()> {
    // The running binary, never a `disctui` found on PATH: an older build
    // reading the same Keychain session gets it revoked.
    let program = std::env::current_exe()
        .map_err(|error| eyre!("could not find the disctui binary to sign in with: {error}"))?;
    let mut child = tokio::process::Command::new(program)
        .arg("--web-login-helper")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| eyre!("could not open the Discord sign-in window: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| eyre!("the sign-in window reported no progress"))?;

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| eyre!("the sign-in window cannot report a failure"))?;

    // Both pipes are read together: a helper that fills the stderr pipe would
    // otherwise block before it closes stdout.
    let report_stages = async {
        let mut stages = tokio::io::AsyncBufReadExt::lines(tokio::io::BufReader::new(stdout));
        while let Some(line) = stages.next_line().await? {
            if let Some(stage) = line.trim().strip_prefix("stage ")
                && let Some(stage) = crate::app::WebStage::parse(stage)
            {
                let _ = tx.try_send(crate::action::Action::WebSignInProgress(stage));
            }
        }
        Ok::<(), io::Error>(())
    };
    let (reported, reason) = tokio::join!(report_stages, last_stderr_bytes(stderr));
    reported?;
    let reason = reason?;

    if child.wait().await?.success() {
        return Ok(());
    }
    bail!("{}", failure_reason(&reason));
}

/// How much of the helper's stderr is kept for the failure message.
const MAX_STDERR_TAIL_BYTES: usize = 8 * 1024;

/// Read the helper's stderr to its end, keeping only the tail.
async fn last_stderr_bytes(mut stderr: tokio::process::ChildStderr) -> io::Result<Vec<u8>> {
    let mut tail = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let read = tokio::io::AsyncReadExt::read(&mut stderr, &mut chunk).await?;
        let Some(bytes) = chunk.get(..read).filter(|bytes| !bytes.is_empty()) else {
            return Ok(tail);
        };
        tail.extend_from_slice(bytes);
        let excess = tail.len().saturating_sub(MAX_STDERR_TAIL_BYTES);
        tail.drain(..excess);
    }
}

/// Say where the sign-in has got: in the window title, and on stdout for the
/// TUI that spawned this process.
fn announce_stage(window: &Window, stage: &LoginStage) {
    let (label, word) = match stage {
        LoginStage::Waiting => ("Complete Discord sign-in", "waiting"),
        LoginStage::Verifying => ("Verifying Discord session", "verifying"),
        LoginStage::Ready => ("Opening disctui", "ready"),
        LoginStage::Rejected => ("Discord session check failed", "rejected"),
    };
    report_stage(word);
    window.set_title(&format!("{label} — disctui"));
}

/// The last meaningful line the helper left on stderr, bounded.
fn failure_reason(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let reason = text
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or("The Discord sign-in window closed without a session");
    reason.chars().take(MAX_REASON_BYTES).collect()
}

/// One progress word for the parent process. Flushed so it arrives as it happens.
fn report_stage(stage: &str) {
    let mut stdout = io::stdout().lock();
    let _ = writeln!(stdout, "stage {stage}");
    let _ = stdout.flush();
}

/// Confirm Discord accepts the captured session, then save it.
async fn validate_and_save(mut captured: Captured) -> Result<()> {
    let client = crate::auth::http_client(Duration::from_secs(20))?;
    let response = crate::auth::get_with_retry(
        &client,
        &captured.token,
        "https://discord.com/api/v10/users/@me",
    )
    .await?;
    if !response.status().is_success() {
        bail!(
            "Discord rejected the new web session (HTTP {})",
            response.status()
        );
    }
    // The saved session needs its client metadata to connect.
    let Some(properties) = captured.client_properties else {
        bail!(
            "Discord's page did not report its client properties, so the session was not \
             saved. Its Gateway connection would identify as a different client and Discord \
             would revoke it. Sign in again."
        );
    };
    let token = std::mem::take(&mut *captured.token);
    let session = crate::auth::MessageSession::new(token, Some(properties));
    crate::auth::save_session(&session)
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;

    use crate::web_login::{candidate, client_properties, failure_reason};

    fn properties_message(json: &str) -> String {
        let encoded = base64::engine::general_purpose::STANDARD.encode(json);
        format!(r#"{{"client_properties":"{encoded}"}}"#)
    }

    #[test]
    fn the_failure_shown_is_the_windows_last_words_bounded() {
        assert_eq!(
            failure_reason(b"opening a window\nError: Discord rejected the new web session\n\n"),
            "Error: Discord rejected the new web session"
        );
        assert_eq!(
            failure_reason(b""),
            "The Discord sign-in window closed without a session"
        );
        let long = vec![b'x'; 1000];
        assert_eq!(failure_reason(&long).chars().count(), 300);
    }

    #[test]
    fn client_properties_are_a_flat_object_from_the_discord_origin() {
        let body = properties_message(r#"{"os":"Mac OS X","client_build_number":1,"x":null}"#);
        let properties = client_properties("https://discord.com/login", &body);
        assert_eq!(
            properties,
            Some(serde_json::json!({"os": "Mac OS X", "client_build_number": 1, "x": null}))
        );
        assert!(client_properties("https://example.org", &body).is_none());
    }

    #[test]
    fn malformed_or_oversized_client_properties_are_ignored() {
        let origin = "https://discord.com";
        let large = format!(r#"{{"os":"{}"}}"#, "x".repeat(4096));
        for json in [
            r#"{"os":{"nested":1}}"#,
            r#"{"os":[1]}"#,
            "{}",
            "[]",
            "1",
            &large,
        ] {
            assert!(client_properties(origin, &properties_message(json)).is_none());
        }
        assert!(client_properties(origin, r#"{"client_properties":"not base64!"}"#).is_none());
        assert!(client_properties(origin, r#""fixture.abcdefghijklmnop.signature""#).is_none());
    }
    #[test]
    fn only_the_discord_origin_can_supply_a_bounded_session_token() {
        let body = r#""fixture.abcdefghijklmnop.signature""#;
        assert!(candidate("https://discord.com/channels/@me", body).is_some());
        for origin in [
            "http://discord.com",
            "https://discord.com:444",
            "https://discord.com.example.org",
            "https://example.org",
        ] {
            assert!(candidate(origin, body).is_none());
        }
        assert!(candidate("https://discord.com", r#""invalid token""#).is_none());
        assert!(candidate("https://discord.com", &"x".repeat(4097)).is_none());
    }
}
