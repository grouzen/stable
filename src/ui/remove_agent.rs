use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

pub fn render_remove_agent(f: &mut Frame, area: Rect, agent_name: &str) {
    // Dialog dimensions: wide enough for the prompt text, 3 rows tall (border + line + border)
    let dialog_width = 60u16.min(area.width.saturating_sub(4));
    let dialog_height = 3u16;

    let dialog_x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
    let dialog_y = area.y + (area.height.saturating_sub(dialog_height)) / 2;

    let dialog_area = Rect {
        x: dialog_x,
        y: dialog_y,
        width: dialog_width,
        height: dialog_height,
    };

    // Clear behind the dialog
    f.render_widget(Clear, dialog_area);

    let text = Line::from(vec![
        Span::raw("Remove \""),
        Span::styled(agent_name, Style::default().fg(Color::Yellow)),
        Span::raw("\"? "),
        Span::styled("[y/Enter]", Style::default().fg(Color::Green)),
        Span::raw(" confirm  "),
        Span::styled("[n/Esc]", Style::default().fg(Color::Red)),
        Span::raw(" cancel"),
    ]);

    let paragraph = Paragraph::new(text)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));

    f.render_widget(paragraph, dialog_area);
}
