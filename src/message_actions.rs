//! Full message text and validated browser destinations for the message pane.

use std::ops::Range;

use linkify::{LinkFinder, LinkKind};
use pulldown_cmark::{Event, Parser, Tag};
use reqwest::Url;

pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_URL_BYTES: usize = 8192;
const MAX_LINKS: usize = 32;

#[derive(Debug, Clone)]
pub enum DesktopAction {
    Copy(String),
    Open(Url),
}

#[derive(Default)]
pub struct MessageCursor {
    pub selected: Option<String>,
    pub visible: Option<String>,
    pub reveal: bool,
}

#[derive(Debug)]
pub struct LinkPicker {
    pub links: Vec<Url>,
    pub selected: usize,
    pub confirm_visible: bool,
}

fn web_url(input: &str) -> Option<Url> {
    if input.len() > MAX_URL_BYTES || input.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    let (scheme, _) = input.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("https") && !scheme.eq_ignore_ascii_case("http") {
        return None;
    }
    let url = Url::parse(input).ok()?;
    url.host_str()?;
    (url.username().is_empty() && url.password().is_none()).then_some(url)
}

fn push_link(urls: &mut Vec<Url>, input: &str) {
    if urls.len() < MAX_LINKS
        && let Some(url) = web_url(input)
        && !urls.contains(&url)
    {
        urls.push(url);
    }
}

fn bare_links(finder: &LinkFinder, urls: &mut Vec<Url>, text: &str) {
    for link in finder.links(text) {
        if urls.len() >= MAX_LINKS {
            break;
        }
        push_link(urls, link.as_str());
    }
}

/// Collect destinations in message order, skipping code and Markdown labels.
pub fn links(text: &str) -> Vec<Url> {
    if text.len() > MAX_MESSAGE_BYTES {
        return Vec::new();
    }
    let parser = Parser::new(text);
    let mut spans: Vec<(Range<usize>, Option<_>)> = parser
        .reference_definitions()
        .iter()
        .map(|(_, definition)| (definition.span.clone(), None))
        .collect();
    spans.extend(
        parser
            .into_offset_iter()
            .filter_map(|(event, range)| match event {
                Event::Start(Tag::Link { dest_url, .. }) => Some((range, Some(dest_url))),
                Event::Code(_) | Event::Start(Tag::CodeBlock(_) | Tag::Image { .. }) => {
                    Some((range, None))
                }
                _ => None,
            }),
    );
    spans.sort_unstable_by_key(|(span, _)| (span.start, std::cmp::Reverse(span.end)));

    let mut finder = LinkFinder::new();
    finder.kinds(&[LinkKind::Url]);
    let mut urls = Vec::new();
    let mut cursor = 0;
    for (span, destination) in spans {
        if span.start < cursor {
            continue;
        }
        if let Some(before) = text.get(cursor..span.start) {
            bare_links(&finder, &mut urls, before);
        }
        if let Some(url) = destination {
            push_link(&mut urls, &url);
        }
        if urls.len() >= MAX_LINKS {
            return urls;
        }
        cursor = span.end;
    }
    if let Some(rest) = text.get(cursor..) {
        bare_links(&finder, &mut urls, rest);
    }
    urls
}

impl DesktopAction {
    pub async fn run(self) -> Result<&'static str, &'static str> {
        use std::process::Stdio;
        use std::time::Duration;

        use tokio::io::AsyncWriteExt;
        use tokio::process::Command;

        if !cfg!(target_os = "macos") {
            return Err(match self {
                Self::Copy(_) => "Copy needs the macOS clipboard",
                Self::Open(_) => "Opening links from disctui needs macOS",
            });
        }

        let result = tokio::time::timeout(Duration::from_secs(5), async {
            let (mut command, text, success, failure) = match self {
                Self::Copy(text) => (
                    Command::new("/usr/bin/pbcopy"),
                    Some(text),
                    "Copied message",
                    "Could not copy message",
                ),
                Self::Open(url) => {
                    let mut command = Command::new("/usr/bin/open");
                    command.arg(url.as_str());
                    (command, None, "Opened link", "Could not open link")
                }
            };
            command
                .stdin(if text.is_some() {
                    Stdio::piped()
                } else {
                    Stdio::null()
                })
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            let mut child = command.spawn().map_err(|_| failure)?;
            if let Some(text) = text {
                let mut input = child.stdin.take().ok_or(failure)?;
                input
                    .write_all(text.as_bytes())
                    .await
                    .map_err(|_| failure)?;
            }
            if child.wait().await.map_err(|_| failure)?.success() {
                Ok(success)
            } else {
                Err(failure)
            }
        })
        .await;
        result.unwrap_or(Err("Clipboard or browser request timed out"))
    }
}

#[cfg(test)]
mod tests {
    use crate::message_actions::links;

    #[test]
    fn link_choices_use_destinations_and_skip_code_and_non_web_schemes() {
        let urls = links(
            "https://first.example/path(a). [https://label.example](https://next.example/long)             `https://code.example` [local](file:///etc/passwd) <https://first.example/path(a)>             https://user:pw@host.example/",
        );
        let urls: Vec<_> = urls.iter().map(reqwest::Url::as_str).collect();
        assert_eq!(
            urls,
            ["https://first.example/path(a)", "https://next.example/long"]
        );
    }
}
