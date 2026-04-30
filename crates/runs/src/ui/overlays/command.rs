use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

pub fn render(f: &mut Frame, area: Rect, buf: &str) {
    let height = 3u16;
    let cmd_area = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(height + 1),
        width: area.width,
        height,
    };
    f.render_widget(Clear, cmd_area);
    let prompt = if buf.starts_with('/') { buf.to_string() } else { format!(":{buf}") };
    let para = Paragraph::new(Line::from(Span::raw(prompt)))
        .block(Block::default().borders(Borders::ALL).title(" Command "));
    f.render_widget(para, cmd_area);
}
