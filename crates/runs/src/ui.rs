pub mod colors;
pub mod overlays;
pub mod screens;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::state::{App, Overlay, Screen};
use colors::palette;

pub fn render(f: &mut Frame, app: &App) {
    let area = f.area();

    // Reserve the bottom row for the status bar before passing area to screens.
    let main_area = Rect {
        height: area.height.saturating_sub(1),
        ..area
    };

    match &app.screen {
        Screen::RunList => screens::run_list::render(f, main_area, app),
        Screen::RunDetail(id) => screens::run_detail::render(f, main_area, app, id),
        Screen::CalcDetail(id) => screens::calc_detail::render(f, main_area, app, id),
        Screen::Dashboard => screens::dashboard::render(f, main_area, app),
    }

    // Overlays render on top of the screen content.
    match &app.overlay {
        Overlay::None => {}
        Overlay::Help => overlays::help::render(f, main_area),
        Overlay::Filter => overlays::filter::render(f, main_area, app),
        Overlay::Command(buf) => overlays::command::render(f, area, buf),
        Overlay::Confirm(dialog) => overlays::confirm::render(f, main_area, &dialog.message),
    }

    // Status bar always rendered last so it's never obscured.
    render_status_bar(f, area, app);
}

// ── Status bar ────────────────────────────────────────────────────────────────

fn render_status_bar(f: &mut Frame, area: Rect, app: &App) {
    let bar_area = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(1),
        width: area.width,
        height: 1,
    };

    let (mode_label, mode_fg, mode_bg) = current_mode(app);

    // Position string (right-aligned).
    let pos = position_string(app);

    // Build the bar as three logical sections using a layout so widths work out.
    // Left:   [▌ MODE ▐] ● sse
    // Center: status message
    // Right:  position
    let mode_width = mode_label.len() as u16 + 4; // padding + separators
    let pos_width = pos.len() as u16 + 2;

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(mode_width),
            Constraint::Min(0),
            Constraint::Length(pos_width),
        ])
        .split(bar_area);

    // ── Mode badge ──
    let mode_span = Line::from(vec![Span::styled(
        format!("  {mode_label}  "),
        Style::default()
            .fg(mode_fg)
            .bg(mode_bg)
            .add_modifier(Modifier::BOLD),
    )]);
    f.render_widget(
        Paragraph::new(mode_span).style(Style::default().bg(mode_bg)),
        chunks[0],
    );

    // ── SSE dot + spinner + status message ──
    const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let (dot, dot_color) = if app.sse_connected {
        (" ● ", palette::GREEN)
    } else {
        (" ○ ", palette::RED)
    };
    let spinner_span = if app.loading {
        let frame = SPINNER_FRAMES[(app.tick as usize / 3) % SPINNER_FRAMES.len()];
        Span::styled(
            format!(" {frame} "),
            Style::default().fg(palette::PEACH).bg(palette::MANTLE),
        )
    } else {
        Span::raw("")
    };
    let mid = Line::from(vec![
        Span::styled(dot, Style::default().fg(dot_color).bg(palette::MANTLE)),
        spinner_span,
        Span::styled(
            app.status_bar.as_str(),
            Style::default().fg(palette::SUBTEXT0).bg(palette::MANTLE),
        ),
    ]);
    f.render_widget(
        Paragraph::new(mid).style(Style::default().bg(palette::MANTLE)),
        chunks[1],
    );

    // ── Position ──
    let right = Line::from(Span::styled(
        format!(" {pos} "),
        Style::default().fg(palette::OVERLAY0).bg(palette::MANTLE),
    ));
    f.render_widget(
        Paragraph::new(right).style(Style::default().bg(palette::MANTLE)),
        chunks[2],
    );
}

/// Derive the current vim-style mode label, foreground, and background from app state.
fn current_mode(app: &App) -> (&'static str, Color, Color) {
    match &app.overlay {
        Overlay::Command(buf) if buf.starts_with('/') => ("SEARCH", palette::BASE, palette::GREEN),
        Overlay::Command(_) => ("COMMAND", palette::BASE, palette::YELLOW),
        Overlay::Filter => ("FILTER", palette::BASE, palette::MAUVE),
        Overlay::Confirm(_) => ("CONFIRM", palette::BASE, palette::RED),
        Overlay::Help => ("HELP", palette::BASE, palette::TEAL),
        Overlay::None => {
            if app.visual_anchor.is_some() {
                return ("VISUAL", palette::BASE, palette::PEACH);
            }
            match app.screen {
                Screen::Dashboard => ("DASHBOARD", palette::BASE, palette::PEACH),
                _ => ("NORMAL", palette::BASE, palette::BLUE),
            }
        }
    }
}

/// Cursor position shown on the right side of the status bar.
fn position_string(app: &App) -> String {
    match &app.screen {
        Screen::RunList => {
            let total = app.visible_runs().len();
            if total == 0 {
                "0/0".into()
            } else {
                format!("{}/{total}", app.run_cursor + 1)
            }
        }
        Screen::RunDetail(run_id) => {
            let calcs = app
                .runs
                .iter()
                .find(|r| &r.id == run_id)
                .map(|r| r.calculations.len())
                .unwrap_or(0);
            if calcs == 0 {
                "0/0".into()
            } else {
                format!("{}/{calcs}", app.calc_cursor + 1)
            }
        }
        Screen::CalcDetail(_) | Screen::Dashboard => String::new(),
    }
}
