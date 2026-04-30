use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph},
};

use common::types::RunId;

use crate::{
    app::state::{App, Pane},
    ui::colors::{border_color, calc_status_color, palette},
};

pub fn render(f: &mut Frame, area: Rect, app: &App, run_id: &RunId) {

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let run = app.runs.iter().find(|r| &r.id == run_id);

    // Left: calculation list.
    let focused_left = matches!(app.pane, Pane::CalcList);

    let title = run
        .map(|r| format!(" {} ", r.jira_issue_id))
        .unwrap_or_else(|| " Run ".into());

    let items: Vec<ListItem> = run
        .map(|r| {
            r.calculations
                .iter()
                .map(|c| {
                    let color = calc_status_color(c.status);
                    let err = c
                        .error_message
                        .as_deref()
                        .unwrap_or("")
                        .chars()
                        .take(40)
                        .collect::<String>();
                    let line = Line::from(vec![
                        Span::styled(
                            format!("{:<12}", c.status.to_string()),
                            Style::default().fg(color),
                        ),
                        Span::raw(format!(" {}", c.kind)),
                        if err.is_empty() {
                            Span::raw("")
                        } else {
                            Span::styled(
                                format!(" — {err}"),
                                Style::default().fg(ratatui::style::Color::Red),
                            )
                        },
                    ]);
                    ListItem::new(line)
                })
                .collect()
        })
        .unwrap_or_default();

    let mut list_state = ListState::default();
    if !items.is_empty() {
        list_state.select(Some(app.calc_cursor));
    }

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(border_color(focused_left)))
                .style(Style::default().bg(palette::BASE))
                .title(Span::styled(title, Style::default().fg(palette::LAVENDER))),
        )
        .highlight_style(
            Style::default().bg(palette::SURFACE0).fg(palette::TEXT).add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(palette::BASE));
    f.render_stateful_widget(list, chunks[0], &mut list_state);

    // Right: selected calculation mini-detail.
    let focused_right = matches!(app.pane, Pane::Detail);

    let detail_text = run
        .and_then(|r| r.calculations.get(app.calc_cursor))
        .map(|c| {
            vec![
                Line::from(format!("Kind:    {}", c.kind)),
                Line::from(format!("Status:  {}", c.status)),
                Line::from(format!("Attempt: {}/{}", c.attempt, c.max_attempts)),
                Line::from(format!(
                    "Input:   {}",
                    serde_json::to_string(&c.input_json)
                        .unwrap_or_default()
                        .chars()
                        .take(80)
                        .collect::<String>()
                )),
                Line::raw(""),
                Line::from(format!(
                    "Error:   {}",
                    c.error_message.as_deref().unwrap_or("-")
                )),
                Line::from(format!(
                    "Result:  {}",
                    c.result_path.as_deref().unwrap_or("-")
                )),
            ]
        })
        .unwrap_or_else(|| vec![Line::raw("No calculation selected")]);

    let detail = Paragraph::new(detail_text)
        .scroll((app.detail_scroll.min(u16::MAX as usize) as u16, 0))
        .style(Style::default().bg(palette::BASE).fg(palette::TEXT))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(border_color(focused_right)))
                .style(Style::default().bg(palette::BASE))
                .title(Span::styled(" Detail ", Style::default().fg(palette::LAVENDER))),
        );
    f.render_widget(detail, chunks[1]);
}
