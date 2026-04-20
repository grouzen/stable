use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::{CreateAgentState, CreateField};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn render_create_agent(f: &mut Frame, area: Rect, state: &CreateAgentState) {
    // Modal dimensions
    let modal_width = 52u16;
    // rows: top border + blank + name + blank + dir + hint + blank + agent + blank + error? + hint + bottom border
    let error_rows: u16 = if state.error.is_some() { 1 } else { 0 };
    let modal_height = 12 + error_rows;

    let modal_area = centered_rect(modal_width, modal_height, area);

    // Clear background behind modal
    f.render_widget(Clear, modal_area);

    let outer = Block::default()
        .title(" New Agent ")
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::White));
    f.render_widget(outer, modal_area);

    // Inner area (inside the border)
    let inner = Rect {
        x: modal_area.x + 1,
        y: modal_area.y + 1,
        width: modal_area.width.saturating_sub(2),
        height: modal_area.height.saturating_sub(2),
    };

    // Layout rows inside inner area
    let mut constraints = vec![
        Constraint::Length(1), // blank
        Constraint::Length(1), // Name label + input
        Constraint::Length(1), // blank
        Constraint::Length(1), // Directory label + input
        Constraint::Length(1), // Tab hint
        Constraint::Length(1), // blank
        Constraint::Length(1), // Agent label
        Constraint::Length(1), // blank
    ];
    if state.error.is_some() {
        constraints.push(Constraint::Length(1)); // error line
    }
    constraints.push(Constraint::Length(1)); // action hints
    constraints.push(Constraint::Min(0)); // padding

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    let mut row = 0usize;

    // blank
    row += 1;

    // Name row
    render_field_row(
        f,
        rows[row],
        "Name:      ",
        &state.name,
        state.focus == CreateField::Name,
    );
    row += 1;

    // blank
    row += 1;

    // Directory row
    render_field_row(
        f,
        rows[row],
        "Directory: ",
        &state.directory,
        state.focus == CreateField::Directory,
    );
    row += 1;

    // Tab hint
    let hint = Paragraph::new("             Tab: path autocomplete")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, rows[row]);
    row += 1;

    // blank
    row += 1;

    // Agent label
    let agent_line = Line::from(vec![
        Span::raw("  Agent:     "),
        Span::styled("● opencode", Style::default().fg(Color::Green)),
    ]);
    f.render_widget(Paragraph::new(agent_line), rows[row]);
    row += 1;

    // blank
    row += 1;

    // Error line (optional)
    if let Some(err) = &state.error {
        let err_text = format!("  Error: {}", err);
        let err_para = Paragraph::new(err_text.as_str()).style(Style::default().fg(Color::Red));
        f.render_widget(err_para, rows[row]);
        row += 1;
    }

    // Action hints
    let actions = Paragraph::new("  [Enter] Launch        [Esc] Cancel")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(actions, rows[row]);
}

// ---------------------------------------------------------------------------
// Field row renderer
// ---------------------------------------------------------------------------

fn render_field_row(f: &mut Frame, area: Rect, label: &str, value: &str, focused: bool) {
    let input_width = area.width.saturating_sub(label.len() as u16 + 2 + 2); // 2 brackets, 2 spaces
    let displayed = truncate_left(value, input_width as usize);

    let input_style = if focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let bracket_style = if focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let line = Line::from(vec![
        Span::raw(format!("  {}", label)),
        Span::styled("[", bracket_style),
        Span::styled(
            format!("{:<width$}", displayed, width = input_width as usize),
            input_style,
        ),
        Span::styled("]", bracket_style),
    ]);

    f.render_widget(Paragraph::new(line), area);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Truncate from the left so the end (cursor position) is always visible.
fn truncate_left(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let start = s.len() - max;
        s[start..].to_string()
    }
}

/// Returns a centered Rect of the given width and height within `area`.
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect {
        x,
        y,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}
