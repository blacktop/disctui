//! Receiver for a single browser authorization on the registered loopback port.
use std::net::Ipv4Addr;
use std::time::Duration;

use color_eyre::eyre::{Result, bail, eyre};
use reqwest::Url;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

pub const REDIRECT: &str = "http://127.0.0.1:38547/callback";

pub async fn bind() -> Result<TcpListener> {
    Ok(TcpListener::bind((Ipv4Addr::LOCALHOST, 38547)).await?)
}

enum Decision {
    Ignore,
    Denied,
    Code(String),
    Failed,
}

fn decision(line: &str, state: &str) -> Decision {
    let Some(path) = line
        .trim_end()
        .strip_prefix("GET ")
        .and_then(|line| line.strip_suffix(" HTTP/1.1"))
    else {
        return Decision::Ignore;
    };
    let Some(query) = path.strip_prefix("/callback?") else {
        return Decision::Ignore;
    };
    let Ok(mut url) = Url::parse(REDIRECT) else {
        return Decision::Ignore;
    };
    url.set_query(Some(query));
    let mut states = url.query_pairs().filter(|(name, _)| name == "state");
    if states.next().is_none_or(|(_, value)| value != state)
        || states.next().is_some()
        || url
            .query_pairs()
            .any(|(name, value)| name == "iss" && value != "https://discord.com")
    {
        return Decision::Ignore;
    }
    if let Some((_, error)) = url.query_pairs().find(|(name, _)| name == "error") {
        return if error == "access_denied" {
            Decision::Denied
        } else {
            Decision::Failed
        };
    }
    let mut codes = url.query_pairs().filter(|(name, _)| name == "code");
    let Some((_, code)) = codes.next() else {
        return Decision::Ignore;
    };
    if code.is_empty() || codes.next().is_some() {
        return Decision::Ignore;
    }
    Decision::Code(code.into_owned())
}

async fn receive(stream: &mut TcpStream, state: &str) -> Result<Decision> {
    let mut reader = BufReader::new(stream.take(8192));
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let answer = decision(&line, state);
    loop {
        line.clear();
        if reader.read_line(&mut line).await? == 0 {
            return Ok(Decision::Ignore);
        }
        if line == "\r\n" {
            break;
        }
    }
    let (status, message) = match &answer {
        Decision::Ignore => ("400 Bad Request", "Invalid callback."),
        Decision::Failed => ("400 Bad Request", "Discord didn't approve sign-in."),
        Decision::Denied => ("200 OK", "Sign-in cancelled."),
        Decision::Code(_) => ("200 OK", "Discord approval received."),
    };
    let (content_type, body) = if matches!(answer, Decision::Ignore) {
        ("text/plain", message.to_owned())
    } else {
        // The page contains only fixed text. Remove the OAuth query before asking
        // the browser to close this tab. Some browsers require a manual close.
        (
            "text/html",
            format!(
                r#"<!doctype html>
<html lang="en">
<meta charset="utf-8">
<title>disctui sign-in</title>
<p>{message} Close this tab and return to disctui.</p>
<script>
history.replaceState(null, "", location.pathname);
window.close();
</script>"#
            ),
        )
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}; charset=utf-8\r\n\
         Content-Security-Policy: default-src 'none'; script-src 'unsafe-inline'; frame-ancestors 'none'; base-uri 'none'\r\n\
         Cache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    // The terminal may complete even if the browser tab closes before reading the reply.
    let _ = reader
        .into_inner()
        .into_inner()
        .write_all(response.as_bytes())
        .await;
    Ok(answer)
}

pub async fn wait(listener: TcpListener, state: &str) -> Result<Option<String>> {
    timeout(Duration::from_secs(180), async {
        loop {
            let (mut stream, _) = listener.accept().await?;
            if let Ok(Ok(answer)) =
                timeout(Duration::from_secs(2), receive(&mut stream, state)).await
            {
                match answer {
                    Decision::Code(code) => return Ok(Some(code)),
                    Decision::Denied => return Ok(None),
                    Decision::Ignore => {}
                    Decision::Failed => bail!("Discord declined the authorization request."),
                }
            }
        }
    })
    .await
    .map_err(|_| eyre!("Browser approval expired. Try signing in again."))?
}

#[cfg(test)]
mod tests {
    use crate::oauth::callback::{Decision, decision};
    #[tokio::test]
    #[expect(
        clippy::unwrap_used,
        reason = "local test listener and callback must succeed"
    )]
    async fn unrelated_browser_request_leaves_the_callback_listening() {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(crate::oauth::callback::wait(listener, "attempt"));
        let client = reqwest::Client::new();
        let stray = client
            .get(format!("http://{addr}/favicon.ico"))
            .send()
            .await
            .unwrap();
        assert_eq!(stray.status(), reqwest::StatusCode::BAD_REQUEST);
        assert_eq!(stray.text().await.unwrap(), "Invalid callback.");
        let callback = client
            .get(format!(
                "http://{addr}/callback?state=attempt&code=fixture-authorization-code"
            ))
            .send()
            .await
            .unwrap();
        assert!(callback.status().is_success());
        let page = callback.text().await.unwrap();
        assert!(page.contains("window.close()"));
        assert!(!page.contains("fixture-authorization-code"));
        assert_eq!(
            task.await.unwrap().unwrap().as_deref(),
            Some("fixture-authorization-code")
        );
    }

    #[test]
    fn only_the_active_attempt_can_complete_or_cancel() {
        for query in [
            "state=other&code=c",
            "state=a&state=a&code=c",
            "state=a&code=c&iss=https://other.invalid",
            "state=a&code=c&code=d",
        ] {
            assert!(matches!(
                decision(&format!("GET /callback?{query} HTTP/1.1\r\n"), "a"),
                Decision::Ignore
            ));
        }
        assert!(
            matches!(decision("GET /callback?state=a&code=c HTTP/1.1\r\n", "a"), Decision::Code(code) if code == "c")
        );
        assert!(matches!(
            decision(
                "GET /callback?state=a&error=access_denied HTTP/1.1\r\n",
                "a"
            ),
            Decision::Denied
        ));
    }
}
