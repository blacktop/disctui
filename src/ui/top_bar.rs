use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, MessagePaneView};
use crate::ui::theme;

/// What the top bar shows: where you are on the left, what is happening on the right.
pub struct TopBar {
    guild: Option<String>,
    channel: Option<String>,
    guild_muted: bool,
    channel_muted: bool,
    summary_view: bool,
    /// Shown in place of the breadcrumb while no server is selected.
    idle_label: &'static str,
    /// Label and animated bar of the load in progress, if any.
    active_load: Option<(String, String)>,
    loaded_messages: usize,
    digest_window: Option<String>,
    digest_status: Option<String>,
}

impl TopBar {
    pub fn from_app(app: &App) -> Self {
        let channel = app.selected_channel_name().map(|name| {
            let marker = app
                .selected_channel_kind()
                .map_or("#", |kind| kind.marker());
            format!("{marker} {name}")
        });
        Self {
            guild: app.selected_guild_name().map(str::to_string),
            channel,
            guild_muted: app.selected_guild_muted(),
            channel_muted: app.selected_channel_directly_muted(),
            summary_view: app.message_pane_view == MessagePaneView::Summary,
            idle_label: if app.browser_account().is_some() {
                "Discord account"
            } else if app.has_sign_in() {
                "not signed in"
            } else {
                "no server selected"
            },
            active_load: app
                .active_load_label()
                .map(str::to_string)
                .zip(app.active_load_progress_bar()),
            loaded_messages: app.messages.len(),
            digest_window: app.digest.visible.then(|| {
                app.digest.report.as_ref().map_or_else(
                    || crate::digest::window_label(crate::digest::DEFAULT_WINDOW),
                    |report| report.window.clone(),
                )
            }),
            digest_status: if app.digest.visible && app.digest.waiting_for_channels.is_some() {
                Some("Loading channels…".into())
            } else if app.digest.job.is_some() {
                Some("Generating…".into())
            } else if app.digest.visible {
                app.digest.report.as_ref().map(|report| {
                    let time = format!("generated {}", report.generated_at);
                    report
                        .model
                        .as_ref()
                        .map_or_else(|| time.clone(), |model| format!("{model} · {time}"))
                })
            } else {
                None
            },
        }
    }

    fn breadcrumb(&self) -> Vec<Span<'static>> {
        let mut spans = vec![
            Span::styled(" disctui ", theme::brand_chip()),
            Span::raw(" "),
        ];
        if let Some(window) = &self.digest_window {
            spans.extend([
                Span::styled("Digest", theme::unread_name()),
                Span::styled(format!(" › {window}"), theme::dim()),
            ]);
            return spans;
        }
        let Some(guild) = &self.guild else {
            spans.push(Span::styled(self.idle_label, theme::dim()));
            return spans;
        };

        let guild_style = if self.channel.is_some() {
            theme::dim()
        } else {
            theme::unread_name()
        };
        spans.push(Span::styled(guild.clone(), guild_style));
        if self.guild_muted {
            spans.push(Span::styled(
                format!(" {}", theme::MUTE_GLYPH),
                theme::muted(),
            ));
        }
        if let Some(channel) = &self.channel {
            spans.push(Span::styled(" \u{203a} ", theme::dim()));
            spans.push(Span::styled(channel.clone(), theme::unread_name()));
            if self.channel_muted {
                spans.push(Span::styled(
                    format!(" {}", theme::MUTE_GLYPH),
                    theme::muted(),
                ));
            }
        }
        if self.summary_view {
            spans.push(Span::styled(" \u{203a} ", theme::dim()));
            spans.push(Span::styled("summary", theme::title()));
        }
        spans
    }

    fn activity(&self) -> Vec<Span<'static>> {
        if let Some(status) = &self.digest_status {
            return vec![Span::styled(format!("{status} "), theme::dim())];
        }
        if self.digest_window.is_some() {
            return Vec::new();
        }
        if let Some((label, bar)) = &self.active_load {
            return vec![
                Span::styled(label.clone(), theme::dim()),
                Span::raw(" "),
                Span::styled(bar.clone(), theme::count()),
                Span::raw(" "),
            ];
        }
        if self.channel.is_some() && self.loaded_messages > 0 {
            return vec![Span::styled(
                format!("{} loaded ", self.loaded_messages),
                theme::dim(),
            )];
        }
        Vec::new()
    }
}

pub fn render(frame: &mut Frame, area: Rect, bar: &TopBar) {
    let activity = Line::from(bar.activity());
    let mut activity_width = u16::try_from(activity.width()).unwrap_or(u16::MAX);
    if bar.digest_window.is_some() {
        activity_width = activity_width.min(area.width / 2);
    }
    let [left, right] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Length(activity_width)]).areas(area);

    frame.render_widget(
        Paragraph::new(Line::from(bar.breadcrumb())).style(theme::bar()),
        left,
    );
    frame.render_widget(Paragraph::new(activity).style(theme::bar()), right);
}
