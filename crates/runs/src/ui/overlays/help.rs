use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::app::keybindings::help_sections;

// LazyVim which-key palette
const KEY_FG: Color = Color::Rgb(245, 194, 231);   // pink
const DESC_FG: Color = Color::Rgb(198, 208, 245);  // lavender
const HEADER_FG: Color = Color::Rgb(166, 227, 161); // green
const BORDER_FG: Color = Color::Rgb(137, 180, 250); // blue
const DIM_FG: Color = Color::Rgb(88, 91, 112);     // surface2
const PANEL_BG: Color = Color::Rgb(24, 24, 37);    // base (Catppuccin Mocha)

pub fn render(f: &mut Frame, area: Rect) {
    let sections = help_sections();

    // Panel takes up the bottom portion, height = longest section + chrome
    let max_entries = sections.iter().map(|s| s.entries.len()).max().unwrap_or(0);
    let panel_height = (max_entries as u16 + 5).min(area.height / 2 + 4);

    let panel_area = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(panel_height),
        width: area.width,
        height: panel_height,
    };

    f.render_widget(Clear, panel_area);

    let outer_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER_FG))
        .style(Style::default().bg(PANEL_BG))
        .title(Line::from(vec![
            Span::styled(" ", Style::default().bg(PANEL_BG)),
            Span::styled("which-key", Style::default().fg(BORDER_FG).add_modifier(Modifier::BOLD).bg(PANEL_BG)),
            Span::styled(" ", Style::default().bg(PANEL_BG)),
        ]));

    let inner = outer_block.inner(panel_area);
    f.render_widget(outer_block, panel_area);

    // Divide inner area into equal columns, one per section.
    let n = sections.len() as u16;
    let constraints: Vec<Constraint> = (0..n).map(|_| Constraint::Ratio(1, n.into())).collect();
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(inner);

    for (i, section) in sections.iter().enumerate() {
        let col = columns[i];
        let mut lines: Vec<Line> = Vec::new();

        // Section header
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {}", section.title),
                Style::default().fg(HEADER_FG).add_modifier(Modifier::BOLD).bg(PANEL_BG),
            ),
        ]));

        // Thin rule under header
        lines.push(Line::from(Span::styled(
            format!(" {}", "─".repeat(col.width.saturating_sub(2) as usize)),
            Style::default().fg(DIM_FG).bg(PANEL_BG),
        )));

        for (key, desc) in section.entries {
            let key_width = 10usize;
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    format!("{:<width$}", key, width = key_width),
                    Style::default().fg(KEY_FG).add_modifier(Modifier::BOLD).bg(PANEL_BG),
                ),
                Span::styled(
                    *desc,
                    Style::default().fg(DESC_FG).bg(PANEL_BG),
                ),
            ]));
        }

        // Footer on last column
        if i == sections.len() - 1 {
            while lines.len() < (panel_height as usize).saturating_sub(3) {
                lines.push(Line::raw(""));
            }
            lines.push(Line::from(vec![
                Span::styled(" Press ", Style::default().fg(DIM_FG).bg(PANEL_BG)),
                Span::styled("?", Style::default().fg(KEY_FG).add_modifier(Modifier::BOLD).bg(PANEL_BG)),
                Span::styled(" or ", Style::default().fg(DIM_FG).bg(PANEL_BG)),
                Span::styled("Esc", Style::default().fg(KEY_FG).add_modifier(Modifier::BOLD).bg(PANEL_BG)),
                Span::styled(" to close", Style::default().fg(DIM_FG).bg(PANEL_BG)),
            ]));
        }

        f.render_widget(
            Paragraph::new(lines).style(Style::default().bg(PANEL_BG)),
            col,
        );
    }
}
