use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::models::{AgentEntry, AgentStatus};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn render_dashboard(f: &mut Frame, area: Rect, agents: &[AgentEntry], selected: usize) {
    // Split into main area and keybindings bar at bottom
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    let main_area = chunks[0];
    let bar_area = chunks[1];

    render_keybindings_bar(f, bar_area);

    if agents.is_empty() {
        render_empty_state(f, main_area);
    } else {
        render_grid(f, main_area, agents, selected);
    }
}

// ---------------------------------------------------------------------------
// Empty state
// ---------------------------------------------------------------------------

fn render_empty_state(f: &mut Frame, area: Rect) {
    let msg = Paragraph::new("No agents. Press [n] to create one.")
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::DarkGray));

    // Vertically center by adding top margin
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(45),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(area);

    f.render_widget(msg, vert[1]);
}

// ---------------------------------------------------------------------------
// Grid
// ---------------------------------------------------------------------------

fn grid_dim(n: usize) -> usize {
    if n <= 4 {
        2
    } else if n <= 9 {
        3
    } else {
        4
    }
}

fn render_grid(f: &mut Frame, area: Rect, agents: &[AgentEntry], selected: usize) {
    let dim = grid_dim(agents.len());
    let total_slots = dim * dim;

    // Build equal row/column constraints
    let col_constraints: Vec<Constraint> =
        (0..dim).map(|_| Constraint::Ratio(1, dim as u32)).collect();
    let row_constraints: Vec<Constraint> =
        (0..dim).map(|_| Constraint::Ratio(1, dim as u32)).collect();

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(row_constraints)
        .split(area);

    for row in 0..dim {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(col_constraints.clone())
            .split(rows[row]);

        for col in 0..dim {
            let slot = row * dim + col;
            let cell_area = cols[col];

            if slot < agents.len() {
                render_card(f, cell_area, &agents[slot], slot == selected);
            } else if slot < total_slots {
                // Empty slot — plain bordered block
                let block = Block::default().borders(Borders::ALL);
                f.render_widget(block, cell_area);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Card
// ---------------------------------------------------------------------------

fn status_symbol(status: &AgentStatus) -> &'static str {
    match status {
        AgentStatus::Running => "●",
        AgentStatus::WaitingForInput => "⏸",
        AgentStatus::Stopped => "■",
        AgentStatus::Unknown => "?",
    }
}

fn status_label(status: &AgentStatus) -> &'static str {
    match status {
        AgentStatus::Running => "Running",
        AgentStatus::WaitingForInput => "Waiting",
        AgentStatus::Stopped => "Stopped",
        AgentStatus::Unknown => "Unknown",
    }
}

fn status_color(status: &AgentStatus) -> Color {
    match status {
        AgentStatus::Running => Color::Green,
        AgentStatus::WaitingForInput => Color::Yellow,
        AgentStatus::Stopped => Color::Red,
        AgentStatus::Unknown => Color::DarkGray,
    }
}

fn format_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.0}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn truncate(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        let truncated: String = chars[..max.saturating_sub(1)].iter().collect();
        format!("{}…", truncated)
    }
}

fn render_card(f: &mut Frame, area: Rect, entry: &AgentEntry, is_selected: bool) {
    let border_style = if is_selected {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let block = Block::default()
        .title(format!(" {} ", entry.config.name))
        .borders(Borders::ALL)
        .border_type(if is_selected {
            BorderType::Double
        } else {
            BorderType::Plain
        })
        .border_style(border_style);

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Build card body lines
    let sym = status_symbol(&entry.meta.status);
    let lbl = status_label(&entry.meta.status);
    let col = status_color(&entry.meta.status);

    let status_line = Line::from(vec![
        Span::styled(sym, Style::default().fg(col)),
        Span::raw(" "),
        Span::styled(lbl, Style::default().fg(col)),
    ]);

    let ctx_line = if let Some(ctx) = &entry.meta.context {
        Line::from(format!(
            "ctx: {}/{}",
            format_tokens(ctx.used),
            format_tokens(ctx.total)
        ))
    } else {
        Line::from("ctx: —")
    };

    let avail_width = inner.width as usize;
    let label_width = 8; // "first: " or "last: "
    let text_width = avail_width.saturating_sub(label_width);

    let first_line = if let Some(fp) = &entry.meta.first_prompt {
        Line::from(format!("first: \"{}\"", truncate(fp, text_width)))
    } else {
        Line::from("first: —")
    };

    let last_line = if let Some(lp) = &entry.meta.last_prompt {
        Line::from(format!("last:  \"{}\"", truncate(lp, text_width)))
    } else {
        Line::from("last:  —")
    };

    let pane_line = Line::from(format!("pane: {}", entry.config.pane));

    let text = Text::from(vec![
        status_line,
        ctx_line,
        first_line,
        last_line,
        pane_line,
    ]);
    let para = Paragraph::new(text);
    f.render_widget(para, inner);
}

// ---------------------------------------------------------------------------
// Keybindings bar
// ---------------------------------------------------------------------------

fn render_keybindings_bar(f: &mut Frame, area: Rect) {
    let bar = Paragraph::new("[n] New  [d] Delete  [Enter] Open  [q] Quit")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(bar, area);
}
