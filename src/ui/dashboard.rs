use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
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

    render_grid(f, main_area, agents, selected);
}

// ---------------------------------------------------------------------------
// Grid
// ---------------------------------------------------------------------------

/// Returns (cols, rows) for the grid based on the number of agents.
///
/// Layout progression:
///   0–2  agents → 2×1  (2 cols, 1 row)
///   3    agents → 3×1  (3 cols, 1 row)
///   4–6  agents → 3×2  (3 cols, 2 rows)
///   7–8  agents → 4×2  (4 cols, 2 rows)
///   9–12 agents → 4×3  (4 cols, 3 rows)
///  13–16 agents → 4×4  (4 cols, 4 rows)
pub fn grid_layout(n: usize) -> (usize, usize) {
    if n <= 2 {
        (2, 1)
    } else if n <= 3 {
        (3, 1)
    } else if n <= 6 {
        (3, 2)
    } else if n <= 8 {
        (4, 2)
    } else if n <= 12 {
        (4, 3)
    } else {
        (4, 4)
    }
}

fn render_grid(f: &mut Frame, area: Rect, agents: &[AgentEntry], selected: usize) {
    if agents.is_empty() {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Ratio(1, 2),
                Constraint::Length(1),
                Constraint::Ratio(1, 2),
            ])
            .split(area);
        let msg = Paragraph::new("No agents. Press [n] to create one.")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(msg, chunks[1]);
        return;
    }

    let (cols, rows) = grid_layout(agents.len());

    let col_constraints: Vec<Constraint> = (0..cols)
        .map(|_| Constraint::Ratio(1, cols as u32))
        .collect();
    let row_constraints: Vec<Constraint> = (0..rows)
        .map(|_| Constraint::Ratio(1, rows as u32))
        .collect();

    let row_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(row_constraints)
        .split(area);

    for row in 0..rows {
        let col_areas = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(col_constraints.clone())
            .split(row_areas[row]);

        for col in 0..cols {
            let slot = row * cols + col;
            let cell_area = col_areas[col];

            if slot < agents.len() {
                render_card(f, cell_area, &agents[slot], slot == selected);
            }
            // Empty slots render as blank (no border)
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
        let used = format_tokens(ctx.used);
        let text = if let Some(total) = ctx.total {
            format!("ctx: {}/{}", used, format_tokens(total))
        } else {
            format!("ctx: {}", used)
        };
        Line::from(text)
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
    let bar = Paragraph::new("[n] New  [d] Delete  [Enter] Open  [←↓↑→/hjkl] Navigate  [q] Quit")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(bar, area);
}
