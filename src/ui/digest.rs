use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Padding, Paragraph, Wrap};

use crate::app::DigestPaneState;
use crate::digest::DigestRow;
use crate::ui::theme;

pub fn render(frame: &mut Frame, area: Rect, state: &mut DigestPaneState) {
    let Some(report) = &state.report else {
        let text = if state.waiting_for_channels.is_some() {
            "Loading server channels…"
        } else if state.job.is_some() {
            "Reading channel history…"
        } else {
            "Press S to generate a digest."
        };
        frame.render_widget(
            Paragraph::new(text).style(theme::dim()),
            area.inner(Margin::new(1, 0)),
        );
        return;
    };

    let [list_area, detail_area] = if area.width >= 74 {
        Layout::horizontal([Constraint::Percentage(54), Constraint::Fill(1)]).areas(area)
    } else {
        Layout::vertical([Constraint::Fill(2), Constraint::Fill(1)]).areas(area)
    };

    if report.rows().next().is_none() {
        frame.render_widget(
            Paragraph::new(format!("Nothing new in the {}.", report.window)).style(theme::dim()),
            area.inner(Margin::new(1, 0)),
        );
        return;
    }
    let items = report.rows().enumerate().map(|(index, row)| {
        let mut lines = Vec::new();
        if index == 0 {
            lines.push(Line::styled(
                format!("NEEDS YOU  {}", report.needs_you.len()),
                theme::attention(),
            ));
        }
        match row {
            DigestRow::NeedsYou(item) => lines.push(Line::from(vec![
                Span::styled(if item.mention { "@ " } else { "" }, theme::attention()),
                Span::styled(item.channel.as_str(), theme::unread_name()),
                Span::styled(format!("  {}  {}", item.time, item.snippet), theme::dim()),
            ])),
            DigestRow::Channel {
                server,
                channel,
                first_in_server,
            } => {
                if index == report.needs_you.len() {
                    lines.extend([Line::default(), Line::styled("TOP CHANNELS", theme::dim())]);
                }
                if first_in_server {
                    lines.push(Line::styled(server.name.as_str(), theme::unread_name()));
                }
                let detail = channel.headline_or_error().unwrap_or_default();
                lines.push(Line::from(vec![
                    Span::raw(format!("  {}  ", channel.name)),
                    Span::styled(format!("{}  ", channel.messages), theme::count()),
                    Span::styled(detail, theme::dim()),
                ]));
            }
            DigestRow::Quiet(names) => {
                lines.extend([
                    Line::default(),
                    Line::styled("QUIET", theme::dim()),
                    Line::raw(names.join(" · ")),
                ]);
            }
        }
        let skip = lines.len().saturating_sub(usize::from(list_area.height));
        ListItem::new(lines.into_iter().skip(skip).collect::<Vec<_>>())
    });
    let list = List::new(items)
        .block(Block::new().padding(Padding::horizontal(1)))
        .highlight_style(theme::selected_row(true));
    frame.render_stateful_widget(list, list_area, &mut state.list);

    if let Some(row) = state
        .list
        .selected()
        .and_then(|index| report.rows().nth(index))
    {
        let border = if detail_area.x == list_area.x {
            Borders::TOP
        } else {
            Borders::LEFT
        };
        frame.render_widget(
            Paragraph::new(details(row))
                .block(
                    Block::new()
                        .borders(border)
                        .border_style(theme::divider())
                        .padding(Padding::horizontal(1)),
                )
                .wrap(Wrap { trim: true }),
            detail_area,
        );
    }
}

fn details(row: DigestRow<'_>) -> Vec<Line<'_>> {
    match row {
        DigestRow::NeedsYou(item) => vec![
            Line::styled(
                format!("{} · {}", item.channel, item.guild),
                theme::unread_name(),
            ),
            Line::default(),
            Line::styled(
                format!("{}  {}", item.author, item.time),
                theme::attention(),
            ),
            Line::raw(item.snippet.as_str()),
            Line::styled(item.reason.as_str(), theme::dim()),
        ],
        DigestRow::Channel {
            server, channel, ..
        } => vec![
            Line::styled(
                format!("{} · {}", channel.name, server.name),
                theme::unread_name(),
            ),
            Line::styled(
                format!(
                    "{} messages · {} mentions",
                    channel.messages, channel.mentions
                ),
                theme::dim(),
            ),
            Line::default(),
            Line::raw(
                channel
                    .summary
                    .as_deref()
                    .or(channel.error.as_deref())
                    .unwrap_or("No AI summary"),
            ),
        ],
        DigestRow::Quiet(names) => vec![
            Line::styled("Quiet servers", theme::unread_name()),
            Line::raw(names.join(" · ")),
        ],
    }
}
