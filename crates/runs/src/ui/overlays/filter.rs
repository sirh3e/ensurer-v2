use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState},
};

use crate::app::state::{App, FILTER_STATUSES};

const BORDER_FG: Color = Color::Rgb(137, 180, 250);
const ACTIVE_FG: Color = Color::Rgb(166, 227, 161);
const DIM_FG: Color = Color::Rgb(88, 91, 112);
const BG: Color = Color::Rgb(24, 24, 37);

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let popup = centered_rect(36, FILTER_STATUSES.len() as u16 + 4, area);
    f.render_widget(Clear, popup);

    let current = app.filter.status.as_deref().unwrap_or("all");

    let items: Vec<ListItem> = FILTER_STATUSES
        .iter()
        .map(|&s| {
            let active = s == current;
            ListItem::new(Line::from(vec![
                Span::styled(
                    if active { "  ● " } else { "    " },
                    Style::default().fg(if active { ACTIVE_FG } else { DIM_FG }),
                ),
                Span::styled(
                    s,
                    Style::default().fg(if active {
                        ACTIVE_FG
                    } else {
                        Color::Rgb(205, 214, 244)
                    }),
                ),
            ]))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.filter.filter_cursor));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(BORDER_FG))
                .style(Style::default().bg(BG))
                .title(Span::styled(
                    " Filter ",
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                )),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Rgb(137, 180, 250))
                .add_modifier(Modifier::BOLD)
                .bg(Color::Rgb(49, 50, 68)),
        )
        .style(Style::default().bg(BG));

    f.render_stateful_widget(list, popup, &mut state);
}

/// Fixed-height popup centred horizontally.
fn centered_rect(width: u16, height: u16, r: Rect) -> Rect {
    let w = width.min(r.width);
    let h = height.min(r.height);
    Rect {
        x: r.x + (r.width.saturating_sub(w)) / 2,
        y: r.y + (r.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}
