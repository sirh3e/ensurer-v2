use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
};

use common::types::CalcId;

use crate::{app::state::App, ui::colors::{calc_status_color, palette}};

pub fn render(f: &mut Frame, area: Rect, app: &App, calc_id: &CalcId) {

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    let calc = app
        .runs
        .iter()
        .flat_map(|r| r.calculations.iter())
        .find(|c| &c.id == calc_id);

    // Top: metadata.
    let meta = calc
        .map(|c| {
            vec![
                Line::from(format!("ID:      {}", c.id)),
                Line::from(format!("Run:     {}", c.run_id)),
                Line::from(format!("Kind:    {}", c.kind)),
                Line::from(format!(
                    "Status:  {}",
                    c.status
                )),
                Line::from(format!("Attempt: {}/{}", c.attempt, c.max_attempts)),
                Line::from(format!(
                    "Created: {}",
                    c.created_at.format("%Y-%m-%d %H:%M:%S UTC")
                )),
                Line::from(format!(
                    "Started: {}",
                    c.started_at
                        .map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                        .unwrap_or_else(|| "-".into())
                )),
                Line::from(format!(
                    "Completed: {}",
                    c.completed_at
                        .map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                        .unwrap_or_else(|| "-".into())
                )),
                Line::raw(""),
                Line::from(format!(
                    "Error: {}",
                    c.error_message.as_deref().unwrap_or("-")
                )),
                Line::from(format!(
                    "Result path: {}",
                    c.result_path.as_deref().unwrap_or("-")
                )),
            ]
        })
        .unwrap_or_else(|| vec![Line::raw("Calculation not found")]);

    let status_color = calc
        .map(|c| calc_status_color(c.status))
        .unwrap_or(ratatui::style::Color::White);

    let meta_block = Paragraph::new(meta)
        .style(Style::default().bg(palette::BASE).fg(palette::TEXT))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(status_color))
                .style(Style::default().bg(palette::BASE))
                .title(Span::styled(" Calculation ", Style::default().fg(palette::LAVENDER))),
        );
    f.render_widget(meta_block, chunks[0]);

    // Bottom: input JSON.
    let input_text = calc
        .map(|c| serde_json::to_string_pretty(&c.input_json).unwrap_or_default())
        .unwrap_or_default();

    let input_lines: Vec<Line> = input_text.lines().map(|l| Line::raw(l.to_owned())).collect();
    let input_block = Paragraph::new(input_lines)
        .scroll((app.detail_scroll.min(u16::MAX as usize) as u16, 0))
        .style(Style::default().bg(palette::BASE).fg(palette::SUBTEXT0))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(palette::SURFACE1))
                .style(Style::default().bg(palette::BASE))
                .title(Span::styled(" Input JSON ", Style::default().fg(palette::LAVENDER))),
        );
    f.render_widget(input_block, chunks[1]);
}
