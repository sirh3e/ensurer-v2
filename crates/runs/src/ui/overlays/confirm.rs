use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Borders, Clear, Paragraph},
};

pub fn render(f: &mut Frame, area: Rect, message: &str) {
    let w = (message.len() as u16 + 4).min(area.width);
    let h = 3u16;
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, popup);
    let para = Paragraph::new(message)
        .block(Block::default().borders(Borders::ALL).title(" Confirm "));
    f.render_widget(para, popup);
}
