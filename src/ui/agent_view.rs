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

    // Compute visible line slice
    let lines = &state.lines;
    let total = lines.len();

    let visible_text = if total == 0 {
        String::new()
    } else if state.scroll_offset == 0 {
        // Live: show last viewport_height lines
        let start = total.saturating_sub(viewport_height);
        lines[start..].join("\n")
    } else {
        // Scrolled: offset is lines-from-bottom
        let end = total.saturating_sub(state.scroll_offset);
        let start = end.saturating_sub(viewport_height);
        lines[start..end].join("\n")
    };

    // Parse ANSI escape sequences into styled ratatui Text
    let text = visible_text
        .as_bytes()
        .into_text()
        .unwrap_or_else(|_| ratatui::text::Text::raw(visible_text.clone()));

    let para = Paragraph::new(text);
    f.render_widget(para, content_area);

    // Forward the pane cursor so the user sees it (live mode only; hide when scrolled).
    if state.scroll_offset == 0 && !state.show_stopped_overlay {
        if let Some((cx, cy)) = state.cursor {
            let screen_x = content_area.x.saturating_add(cx);
            let screen_y = content_area.y.saturating_add(cy);
            // Only set if within the content area bounds.
            if screen_x < content_area.x + content_area.width
                && screen_y < content_area.y + content_area.height
            {
                f.set_cursor_position((screen_x, screen_y));
            }
        }
    }

    // Status bar
    let refresh_str = if let Some(instant) = state.last_refresh {
        // Format elapsed as HH:MM:SS using system time
        let elapsed = instant.elapsed();
        let secs = elapsed.as_secs();
        // Use wall-clock time via SystemTime
        if let Ok(sys_now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            let wall_secs = sys_now.as_secs().saturating_sub(secs);
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

    let scrolled_indicator = if state.scroll_offset > 0 {
        "  [scrolled]"
    } else {
        ""
    };

    let status_text = format!(
        "pane: {} | {} | last refresh {}{}",
        agent_entry.config.pane, agent_entry.config.agent_type, refresh_str, scrolled_indicator
    );

    let status_bar = Paragraph::new(status_text).style(Style::default().fg(Color::DarkGray));
    f.render_widget(status_bar, status_area);

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
