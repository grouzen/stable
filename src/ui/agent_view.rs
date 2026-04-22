use ansi_to_tui::IntoText;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::AgentViewState;
use crate::models::AgentEntry;

pub fn render_agent_view(
    f: &mut Frame,
    area: Rect,
    state: &AgentViewState,
    agent_entry: &AgentEntry,
) {
    // Split into content area and status bar (last row)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    let content_area = chunks[0];
    let status_area = chunks[1];

    let viewport_height = content_area.height as usize;

    // Show the last viewport_height lines (live — scrolling is handled by tmux/opencode).
    let lines = &state.lines;
    let total = lines.len();
    let visible_text = if total == 0 {
        String::new()
    } else {
        let start = total.saturating_sub(viewport_height);
        lines[start..].join("\n")
    };

    // Parse ANSI escape sequences into styled ratatui Text
    let text = visible_text
        .as_bytes()
        .into_text()
        .unwrap_or_else(|_| ratatui::text::Text::raw(visible_text.clone()));

    // Use the first background colour found in the parsed ANSI content as the
    // base style for the paragraph.  Cells that carry no explicit background
    // code (e.g. default-colour spaces around a modal overlay) will then
    // inherit the pane's own background rather than stable's terminal default.
    let base_bg = text
        .lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .find_map(|s| s.style.bg);
    let base_style = match base_bg {
        Some(bg) => Style::default().bg(bg),
        None => Style::default(),
    };
    let para = Paragraph::new(text).style(base_style);
    f.render_widget(para, content_area);

    // Forward the pane cursor.
    if !state.show_stopped_overlay {
        if let Some((cx, cy)) = state.cursor {
            let screen_x = content_area.x.saturating_add(cx);
            let screen_y = content_area.y.saturating_add(cy);
            if screen_x < content_area.x + content_area.width
                && screen_y < content_area.y + content_area.height
            {
                f.set_cursor_position((screen_x, screen_y));
            }
        }
    }

    // Status bar
    let refresh_str = if let Some(sys_time) = state.last_refresh {
        if let Ok(dur) = sys_time.duration_since(std::time::UNIX_EPOCH) {
            let wall_secs = dur.as_secs();
            let h = (wall_secs / 3600) % 24;
            let m = (wall_secs / 60) % 60;
            let s = wall_secs % 60;
            format!("{:02}:{:02}:{:02}", h, m, s)
        } else {
            "??:??:??".to_string()
        }
    } else {
        "--:--:--".to_string()
    };

    let status_text = format!(
        " {} | {} | {} | last refresh {}",
        agent_entry.config.name,
        agent_entry.config.directory,
        agent_entry.config.agent_type,
        refresh_str
    );

    let hint = " Shift+drag to select";
    let hint_width = hint.len() as u16;
    let status_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(hint_width)])
        .split(status_area);

    let status_bar = Paragraph::new(status_text).style(Style::default().fg(Color::DarkGray));
    f.render_widget(status_bar, status_chunks[0]);

    let hint_bar = Paragraph::new(hint).style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint_bar, status_chunks[1]);

    // Stopped overlay
    if state.show_stopped_overlay {
        render_stopped_overlay(f, area);
    }
}

fn render_stopped_overlay(f: &mut Frame, area: Rect) {
    // Compute a centered box: 46 wide, 6 tall
    let overlay_width = 46u16.min(area.width);
    let overlay_height = 6u16.min(area.height);
    let x = area.x + area.width.saturating_sub(overlay_width) / 2;
    let y = area.y + area.height.saturating_sub(overlay_height) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    f.render_widget(Clear, overlay_area);

    let block = Block::default().borders(Borders::ALL);
    let inner = block.inner(overlay_area);
    f.render_widget(block, overlay_area);

    // Content lines inside the box
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Agent stopped.",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("[d]", Style::default().fg(Color::Yellow)),
            Span::raw(" Remove agent   "),
            Span::styled("[Ctrl-g]", Style::default().fg(Color::Yellow)),
            Span::raw(" Dashboard"),
        ]),
    ];

    let para = Paragraph::new(lines).alignment(Alignment::Center);
    f.render_widget(para, inner);
}
