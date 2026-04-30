use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState},
};

use crate::{
    app::state::{App, Pane},
    ui::colors::{border_color, calc_status_color, palette, run_status_color},
};

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    render_runs_pane(f, chunks[0], app);
    render_calcs_preview(f, chunks[1], app);
}

/// Highlight `query` occurrences inside `text`, returning a vec of Spans.
fn highlight_match<'a>(text: &'a str, query: &str, base_color: Color) -> Vec<Span<'a>> {
    if query.is_empty() {
        return vec![Span::styled(text, Style::default().fg(base_color))];
    }
    let mut spans = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.to_lowercase().find(&query.to_lowercase()) {
        if pos > 0 {
            spans.push(Span::styled(&rest[..pos], Style::default().fg(base_color)));
        }
        spans.push(Span::styled(
            &rest[pos..pos + query.len()],
            Style::default().fg(palette::BASE).bg(palette::YELLOW).add_modifier(Modifier::BOLD),
        ));
        rest = &rest[pos + query.len()..];
    }
    if !rest.is_empty() {
        spans.push(Span::styled(rest, Style::default().fg(base_color)));
    }
    spans
}

fn render_runs_pane(f: &mut Frame, area: Rect, app: &App) {
    let runs = app.visible_runs();
    let focused = app.pane == Pane::RunList;
    let visual_range = app.visual_range();
    let search_query = app.filter.search.as_deref().unwrap_or("");

    let items: Vec<ListItem> = runs
        .iter()
        .enumerate()
        .map(|(idx, r)| {
            let color = run_status_color(r.status);
            let in_visual = visual_range.as_ref().is_some_and(|range| range.contains(&idx));
            let mut spans = vec![
                Span::styled(format!("{:<14}", r.status.to_string()), Style::default().fg(color)),
            ];
            spans.extend(highlight_match(r.jira_issue_id.as_str(), search_query, palette::TEXT));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format!("({} calcs)", r.calculations.len()),
                Style::default().fg(palette::OVERLAY0),
            ));
            let item = ListItem::new(Line::from(spans));
            if in_visual {
                item.style(Style::default().bg(palette::SURFACE1).fg(palette::TEXT))
            } else {
                item
            }
        })
        .collect();

    let mut state = ListState::default();
    if !runs.is_empty() {
        state.select(Some(app.run_cursor));
    }

    let filter_hint = app.filter.status.as_deref()
        .map(|s| format!(" [filter: {s}]"))
        .unwrap_or_default();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(border_color(focused)))
                .style(Style::default().bg(palette::BASE))
                .title(Span::styled(
                    format!(" Runs{filter_hint} "),
                    Style::default().fg(palette::LAVENDER),
                )),
        )
        .highlight_style(
            Style::default()
                .bg(palette::SURFACE0)
                .fg(palette::TEXT)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(palette::BASE));

    f.render_stateful_widget(list, area, &mut state);
}

fn render_calcs_preview(f: &mut Frame, area: Rect, app: &App) {
    let focused = app.pane == Pane::CalcList;

    let title = app
        .selected_run()
        .map(|r| format!(" {} ", r.jira_issue_id))
        .unwrap_or_else(|| " Calculations ".into());

    let items: Vec<ListItem> = app
        .selected_run()
        .map(|r| {
            r.calculations
                .iter()
                .map(|c| {
                    let color = calc_status_color(c.status);
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{:<14}", c.status.to_string()), Style::default().fg(color)),
                        Span::styled(c.kind.as_str(), Style::default().fg(palette::TEXT)),
                        Span::raw(" "),
                        Span::styled(
                            format!("#{}", c.attempt),
                            Style::default().fg(palette::OVERLAY0),
                        ),
                    ]))
                })
                .collect()
        })
        .unwrap_or_default();

    let mut state = ListState::default();
    if !items.is_empty() {
        state.select(Some(app.calc_cursor));
    }

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(border_color(focused)))
                .style(Style::default().bg(palette::BASE))
                .title(Span::styled(title, Style::default().fg(palette::LAVENDER))),
        )
        .highlight_style(
            Style::default()
                .bg(palette::SURFACE0)
                .fg(palette::TEXT)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(palette::BASE));

    f.render_stateful_widget(list, area, &mut state);
}
