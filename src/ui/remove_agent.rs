use ratatui::{
    layout::Alignment,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use ratatui::layout::Rect;

use crate::ui::theme::*;

pub fn render_remove_agent(f: &mut Frame, area: Rect, agent_name: &str) {
    // Dialog dimensions: wide enough for the prompt text, 5 rows tall for breathing room
    let dialog_width = 62u16.min(area.width.saturating_sub(4));
    let dialog_height = 5u16;

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

    let block = Block::default()
        .title(Span::styled(
            " Remove Agent ",
            Style::default().fg(RED).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(RED));

    let inner = block.inner(dialog_area);
    f.render_widget(block, dialog_area);

    // Content: blank line then the confirmation prompt
    let text = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("Remove ", Style::default().fg(GRAY)),
            Span::styled(
                agent_name,
                Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
            ),
            Span::styled("?  ", Style::default().fg(GRAY)),
            Span::styled("[", Style::default().fg(BG2)),
            Span::styled("y / Enter", Style::default().fg(GREEN)),
            Span::styled("]", Style::default().fg(BG2)),
            Span::styled(" confirm  ", Style::default().fg(GRAY)),
            Span::styled("[", Style::default().fg(BG2)),
            Span::styled("n / Esc", Style::default().fg(RED)),
            Span::styled("]", Style::default().fg(BG2)),
            Span::styled(" cancel", Style::default().fg(GRAY)),
        ]),
    ];

    let paragraph = Paragraph::new(text).alignment(Alignment::Center);
    f.render_widget(paragraph, inner);
}
