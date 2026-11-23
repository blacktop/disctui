//! Program status reports (OSC 7501). Terminals that understand the sequence, such
//! as Rex, show whether disctui is working, done, or waiting on the user. Other
//! terminals ignore it.

use std::fmt::Write as _;
use std::io::{self, IsTerminal, Write};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

const APP: &str = "disctui";
/// The protocol caps a decoded message at 2048 bytes.
const MAX_MESSAGE_BYTES: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Working,
    Done,
    /// Waiting on a login, token, or credential.
    BlockedOnAuth,
    Error,
}

impl State {
    const fn pairs(self) -> &'static str {
        match self {
            Self::Idle => "state=idle",
            Self::Working => "state=working",
            Self::Done => "state=done",
            Self::BlockedOnAuth => "state=blocked:kind=auth",
            Self::Error => "state=error",
        }
    }

    const fn carries_progress(self) -> bool {
        match self {
            Self::Working | Self::BlockedOnAuth => true,
            Self::Idle | Self::Done | Self::Error => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub state: State,
    pub message: String,
    /// Percent complete, for `Working`.
    pub progress: Option<u8>,
}

impl Report {
    pub fn new(state: State, message: impl Into<String>) -> Self {
        Self {
            state,
            message: message.into(),
            progress: None,
        }
    }

    #[must_use]
    pub fn with_progress(mut self, done: usize, total: usize) -> Self {
        self.progress = done
            .saturating_mul(100)
            .checked_div(total)
            .and_then(|percent| u8::try_from(percent.min(100)).ok());
        self
    }

    /// The escape sequence for this report.
    pub fn sequence(&self) -> String {
        let mut body = format!("{}:app={APP}", self.state.pairs());
        if self.state.carries_progress()
            && let Some(progress) = self.progress
        {
            let _ = write!(body, ":progress={progress}");
        }
        let message = sanitize_message(&self.message);
        if !message.is_empty() {
            let _ = write!(body, ":msg={}", STANDARD.encode(message));
        }
        format!("\u{1b}]7501;{body}\u{1b}\\")
    }
}

/// Control characters would make a terminal discard the whole report.
fn sanitize_message(message: &str) -> String {
    let mut clean = String::new();
    for ch in message.chars() {
        let ch = if ch.is_control() { ' ' } else { ch };
        if clean.len() + ch.len_utf8() > MAX_MESSAGE_BYTES {
            break;
        }
        clean.push(ch);
    }
    clean
}

/// Sends reports to a terminal stream, skipping repeats and non-terminals.
pub struct Reporter<W: Write> {
    writer: W,
    enabled: bool,
    last: Option<Report>,
}

impl Reporter<io::Stderr> {
    /// For headless commands: stdout carries the result, so status goes to stderr.
    pub fn stderr() -> Self {
        let stream = io::stderr();
        let enabled = stream.is_terminal();
        Self::new(stream, enabled)
    }
}

impl Reporter<io::Stdout> {
    /// For the TUI, which owns stdout.
    pub fn stdout() -> Self {
        let stream = io::stdout();
        let enabled = stream.is_terminal();
        Self::new(stream, enabled)
    }
}

impl<W: Write> Reporter<W> {
    pub fn new(writer: W, enabled: bool) -> Self {
        Self {
            writer,
            enabled,
            last: None,
        }
    }

    pub fn report(&mut self, report: Report) {
        if !self.enabled || self.last.as_ref() == Some(&report) {
            return;
        }
        let written = self
            .writer
            .write_all(report.sequence().as_bytes())
            .and_then(|()| self.writer.flush());
        if let Err(err) = written {
            tracing::debug!("failed to write program status: {err}");
        }
        self.last = Some(report);
    }
}

#[cfg(test)]
mod tests {
    use crate::status::*;

    #[test]
    fn working_report_carries_progress_and_base64_message() {
        let report = Report::new(State::Working, "Reading #general").with_progress(1, 4);
        assert_eq!(
            report.sequence(),
            "\u{1b}]7501;state=working:app=disctui:progress=25:msg=UmVhZGluZyAjZ2VuZXJhbA==\u{1b}\\"
        );
    }

    #[test]
    fn done_report_drops_progress() {
        let mut report = Report::new(State::Done, "ok");
        report.progress = Some(50);
        assert_eq!(
            report.sequence(),
            "\u{1b}]7501;state=done:app=disctui:msg=b2s=\u{1b}\\"
        );
    }

    #[test]
    fn auth_block_names_its_kind_and_empty_message_is_omitted() {
        let report = Report::new(State::BlockedOnAuth, "");
        assert_eq!(
            report.sequence(),
            "\u{1b}]7501;state=blocked:kind=auth:app=disctui\u{1b}\\"
        );
    }

    #[test]
    fn control_characters_are_replaced_and_long_messages_cut() {
        assert_eq!(sanitize_message("a\nb\u{1b}c"), "a b c");
        let long = "\u{e9}".repeat(2000);
        let clean = sanitize_message(&long);
        assert!(clean.len() <= MAX_MESSAGE_BYTES);
        assert_eq!(clean.chars().count(), MAX_MESSAGE_BYTES / 2);
    }

    #[test]
    fn progress_handles_zero_total_and_overshoot() {
        assert_eq!(
            Report::new(State::Working, "x")
                .with_progress(1, 0)
                .progress,
            None
        );
        assert_eq!(
            Report::new(State::Working, "x")
                .with_progress(9, 3)
                .progress,
            Some(100)
        );
    }

    #[test]
    fn reporter_skips_repeats_and_disabled_streams() {
        let mut reporter = Reporter::new(Vec::new(), true);
        reporter.report(Report::new(State::Working, "a"));
        reporter.report(Report::new(State::Working, "a"));
        reporter.report(Report::new(State::Idle, ""));
        let written = String::from_utf8_lossy(&reporter.writer).into_owned();
        assert_eq!(written.matches("\u{1b}]7501;").count(), 2);

        let mut disabled = Reporter::new(Vec::new(), false);
        disabled.report(Report::new(State::Working, "a"));
        assert_eq!(disabled.writer, Vec::<u8>::new());
    }
}
