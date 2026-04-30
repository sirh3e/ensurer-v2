use chrono::Utc;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
    },
};

use common::model::{CalcStatus, Run, RunStatus};

use crate::{
    app::state::App,
    ui::colors::{calc_status_color, palette, run_status_color},
};

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let main_area = Rect {
        height: area.height.saturating_sub(1),
        ..area
    };

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(palette::BLUE))
        .style(Style::default().bg(palette::BASE))
        .title(Span::styled(
            " Dashboard ",
            Style::default()
                .fg(palette::LAVENDER)
                .add_modifier(Modifier::BOLD),
        ));

    let inner = outer.inner(main_area);
    f.render_widget(outer, main_area);

    // Vertical split: top = stat tiles, bottom = job lists.
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(10), Constraint::Min(0)])
        .split(inner);

    render_stats(f, sections[0], app);
    render_jobs(f, sections[1], app);
}

// ── Stats grid ────────────────────────────────────────────────────────────────

fn render_stats(f: &mut Frame, area: Rect, app: &App) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    render_run_stats(f, cols[0], app);
    render_calc_stats(f, cols[1], app);
}

fn render_run_stats(f: &mut Frame, area: Rect, app: &App) {
    let counts = |status: RunStatus| app.runs.iter().filter(|r| r.status == status).count();
    let total = app.runs.len();

    let rows = vec![
        stat_line(
            "pending",
            counts(RunStatus::Pending),
            run_status_color(RunStatus::Pending),
        ),
        stat_line(
            "running",
            counts(RunStatus::Running),
            run_status_color(RunStatus::Running),
        ),
        stat_line(
            "succeeded",
            counts(RunStatus::Succeeded),
            run_status_color(RunStatus::Succeeded),
        ),
        stat_line(
            "failed",
            counts(RunStatus::Failed),
            run_status_color(RunStatus::Failed),
        ),
        stat_line(
            "cancelled",
            counts(RunStatus::Cancelled),
            run_status_color(RunStatus::Cancelled),
        ),
        stat_line(
            "partial",
            counts(RunStatus::PartiallySucceeded),
            run_status_color(RunStatus::PartiallySucceeded),
        ),
        Line::from(Span::styled(
            "─".repeat(28),
            Style::default().fg(palette::SURFACE1),
        )),
        stat_line("total", total, palette::TEXT),
    ];

    let para = Paragraph::new(rows)
        .style(Style::default().bg(palette::BASE))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(palette::SURFACE1))
                .style(Style::default().bg(palette::BASE))
                .title(Span::styled(" Runs ", Style::default().fg(palette::MAUVE))),
        );
    f.render_widget(para, area);
}

fn render_calc_stats(f: &mut Frame, area: Rect, app: &App) {
    let all_calcs: Vec<CalcStatus> = app
        .runs
        .iter()
        .flat_map(|r| r.calculations.iter().map(|c| c.status))
        .collect();
    let counts = |status: CalcStatus| all_calcs.iter().filter(|&&s| s == status).count();
    let total = all_calcs.len();

    let rows = vec![
        stat_line(
            "pending",
            counts(CalcStatus::Pending),
            calc_status_color(CalcStatus::Pending),
        ),
        stat_line(
            "running",
            counts(CalcStatus::Running),
            calc_status_color(CalcStatus::Running),
        ),
        stat_line(
            "retrying",
            counts(CalcStatus::Retrying),
            calc_status_color(CalcStatus::Retrying),
        ),
        stat_line(
            "succeeded",
            counts(CalcStatus::Succeeded),
            calc_status_color(CalcStatus::Succeeded),
        ),
        stat_line(
            "failed",
            counts(CalcStatus::Failed),
            calc_status_color(CalcStatus::Failed),
        ),
        stat_line(
            "cancelled",
            counts(CalcStatus::Cancelled),
            calc_status_color(CalcStatus::Cancelled),
        ),
        Line::from(Span::styled(
            "─".repeat(28),
            Style::default().fg(palette::SURFACE1),
        )),
        stat_line("total", total, palette::TEXT),
    ];

    let para = Paragraph::new(rows)
        .style(Style::default().bg(palette::BASE))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(palette::SURFACE1))
                .style(Style::default().bg(palette::BASE))
                .title(Span::styled(
                    " Calculations ",
                    Style::default().fg(palette::MAUVE),
                )),
        );
    f.render_widget(para, area);
}

fn stat_line(label: &str, count: usize, color: ratatui::style::Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {:<16}", label),
            Style::default().fg(palette::SUBTEXT0),
        ),
        Span::styled(
            format!("{:>4}", count),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ])
}

// ── Active jobs + history ─────────────────────────────────────────────────────

fn render_jobs(f: &mut Frame, area: Rect, app: &App) {
    let now = Utc::now();

    let active: Vec<&Run> = app
        .runs
        .iter()
        .filter(|r| r.status == RunStatus::Running || r.status == RunStatus::Pending)
        .collect();

    let history: Vec<&Run> = {
        let mut v: Vec<&Run> = app
            .runs
            .iter()
            .filter(|r| !matches!(r.status, RunStatus::Running | RunStatus::Pending))
            .collect();
        v.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        v
    };

    let mut lines: Vec<Line> = Vec::new();

    // ── Active ────────────────────────────────────────────────────────────────
    lines.push(section_header("Active Jobs"));

    if active.is_empty() {
        lines.push(dim_line("  no active jobs"));
    } else {
        for run in &active {
            lines.push(active_job_line(run, now));
        }
    }

    lines.push(Line::raw(""));

    // ── History ───────────────────────────────────────────────────────────────
    lines.push(section_header("History"));

    if history.is_empty() {
        lines.push(dim_line("  no completed jobs yet"));
    } else {
        for run in &history {
            lines.push(history_line(run, now));
        }
    }

    let total = lines.len();
    let visible = area.height.saturating_sub(2) as usize;

    let para = Paragraph::new(lines)
        .style(Style::default().bg(palette::BASE).fg(palette::TEXT))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(palette::SURFACE1))
                .style(Style::default().bg(palette::BASE)),
        );

    f.render_widget(para, area);

    // Scrollbar when content overflows.
    if total > visible {
        let mut sb_state = ScrollbarState::new(total).position(0);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area,
            &mut sb_state,
        );
    }
}

fn section_header(title: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {title} "),
            Style::default()
                .fg(palette::YELLOW)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("─".repeat(40), Style::default().fg(palette::SURFACE1)),
    ])
}

fn dim_line(text: &'static str) -> Line<'static> {
    Line::from(Span::styled(text, Style::default().fg(palette::OVERLAY0)))
}

fn active_job_line(run: &Run, now: chrono::DateTime<Utc>) -> Line<'static> {
    let done = run
        .calculations
        .iter()
        .filter(|c| c.status.is_terminal())
        .count();
    let total = run.calculations.len();
    let bar = progress_bar(done, total, 12);
    let age = relative_time(run.created_at, now);
    let color = run_status_color(run.status);

    Line::from(vec![
        Span::styled("  ● ", Style::default().fg(color)),
        Span::styled(
            format!("{:<16}", run.jira_issue_id),
            Style::default()
                .fg(palette::TEXT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(bar, Style::default().fg(palette::TEAL)),
        Span::styled(
            format!("  {done}/{total}"),
            Style::default().fg(palette::SUBTEXT0),
        ),
        Span::styled(format!("  {age}"), Style::default().fg(palette::OVERLAY0)),
    ])
}

fn history_line(run: &Run, now: chrono::DateTime<Utc>) -> Line<'static> {
    let done = run
        .calculations
        .iter()
        .filter(|c| c.status == CalcStatus::Succeeded)
        .count();
    let total = run.calculations.len();
    let age = relative_time(run.updated_at, now);
    let color = run_status_color(run.status);
    let icon = match run.status {
        RunStatus::Succeeded => "✓",
        RunStatus::Failed => "✗",
        RunStatus::Cancelled => "⊘",
        RunStatus::PartiallySucceeded => "~",
        _ => " ",
    };

    Line::from(vec![
        Span::styled(format!("  {icon} "), Style::default().fg(color)),
        Span::styled(
            format!("{:<14}", run.status.to_string()),
            Style::default().fg(color),
        ),
        Span::styled(
            format!("{:<16}", run.jira_issue_id),
            Style::default().fg(palette::TEXT),
        ),
        Span::styled(
            format!("{done}/{total} calcs"),
            Style::default().fg(palette::SUBTEXT0),
        ),
        Span::styled(format!("  {age}"), Style::default().fg(palette::OVERLAY0)),
    ])
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn progress_bar(done: usize, total: usize, width: usize) -> String {
    if total == 0 {
        return format!(" [{}]", "░".repeat(width));
    }
    let filled = (done * width / total).min(width);
    format!(" [{}{}]", "█".repeat(filled), "░".repeat(width - filled))
}

fn relative_time(dt: chrono::DateTime<Utc>, now: chrono::DateTime<Utc>) -> String {
    let secs = now.signed_duration_since(dt).num_seconds().max(0);
    if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}
