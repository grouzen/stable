use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::models::{AgentEntry, AgentStatus};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn render_dashboard(
    f: &mut Frame,
    area: Rect,
    agents: &[AgentEntry],
    selected: usize,
    card_scroll: &[u16],
    card_response_heights: &mut Vec<u16>,
    card_response_widths: &mut Vec<u16>,
) {
    // Split into main area and keybindings bar at bottom
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    let main_area = chunks[0];
    let bar_area = chunks[1];

    render_keybindings_bar(f, bar_area);
    render_grid(
        f,
        main_area,
        agents,
        selected,
        card_scroll,
        card_response_heights,
        card_response_widths,
    );
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

fn render_grid(
    f: &mut Frame,
    area: Rect,
    agents: &[AgentEntry],
    selected: usize,
    card_scroll: &[u16],
    card_response_heights: &mut Vec<u16>,
    card_response_widths: &mut Vec<u16>,
) {
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
            .alignment(Alignment::Center);
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

    // Ensure vecs have room for all slots
    if card_response_heights.len() < agents.len() {
        card_response_heights.resize(agents.len(), 0);
    }
    if card_response_widths.len() < agents.len() {
        card_response_widths.resize(agents.len(), 0);
    }

    for row in 0..rows {
        let col_areas = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(col_constraints.clone())
            .split(row_areas[row]);

        for col in 0..cols {
            let slot = row * cols + col;
            let cell_area = col_areas[col];

            if slot < agents.len() {
                let scroll = card_scroll.get(slot).copied().unwrap_or(0);
                let (resp_h, resp_w) =
                    render_card(f, cell_area, &agents[slot], slot == selected, scroll);
                card_response_heights[slot] = resp_h;
                card_response_widths[slot] = resp_w;
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

fn render_card(
    f: &mut Frame,
    area: Rect,
    entry: &AgentEntry,
    is_selected: bool,
    response_scroll: u16,
) -> (u16, u16) {
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

    if inner.height == 0 || inner.width == 0 {
        return (0, 0);
    }

    // -----------------------------------------------------------------------
    // Layout: top header (3 lines) + response block (remaining)
    // -----------------------------------------------------------------------
    //   line 0 — ctx info (left) + status (right)
    //   line 1 — first prompt
    //   line 2 — last prompt
    //   rest   — last model response (markdown, scrollable when selected)
    // -----------------------------------------------------------------------

    let header_lines: u16 = 3;
    let (header_area, response_area) = if inner.height > header_lines {
        let splits = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(header_lines), Constraint::Min(0)])
            .split(inner);
        (splits[0], Some(splits[1]))
    } else {
        (inner, None)
    };

    // -----------------------------------------------------------------------
    // Header — 3 rows
    // -----------------------------------------------------------------------

    // Row 0: ctx (left) + status (right)
    let sym = status_symbol(&entry.meta.status);
    let lbl = status_label(&entry.meta.status);
    let col = status_color(&entry.meta.status);

    let ctx_text = if let Some(ctx) = &entry.meta.context {
        let used = format_tokens(ctx.used);
        if let Some(total) = ctx.total {
            format!("ctx: {}/{}", used, format_tokens(total))
        } else {
            format!("ctx: {}", used)
        }
    } else {
        "ctx: —".to_string()
    };

    let status_str = format!("{} {}", sym, lbl);
    let avail = inner.width as usize;
    // Pad ctx_text so status is right-aligned on the same line
    let padding = avail.saturating_sub(ctx_text.len() + status_str.len());
    let row0_text = format!("{}{}{}", ctx_text, " ".repeat(padding), status_str);
    let row0 = Line::from(vec![
        Span::raw(&row0_text[..ctx_text.len() + padding]),
        Span::styled(status_str.clone(), Style::default().fg(col)),
    ]);

    // Row 1: first prompt
    let label_w = 8usize; // "first: " width
    let text_w = avail.saturating_sub(label_w + 2); // +2 for quotes
    let row1 = if let Some(fp) = &entry.meta.first_prompt {
        Line::from(vec![
            Span::styled("first: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("\"{}\"", truncate(fp, text_w))),
        ])
    } else {
        Line::from(vec![
            Span::styled("first: ", Style::default().fg(Color::DarkGray)),
            Span::styled("—", Style::default().fg(Color::DarkGray)),
        ])
    };

    // Row 2: last prompt
    let row2 = if let Some(lp) = &entry.meta.last_prompt {
        Line::from(vec![
            Span::styled("last:  ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("\"{}\"", truncate(lp, text_w))),
        ])
    } else {
        Line::from(vec![
            Span::styled("last:  ", Style::default().fg(Color::DarkGray)),
            Span::styled("—", Style::default().fg(Color::DarkGray)),
        ])
    };

    // Render the three header rows
    let header_splits = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(header_area);

    f.render_widget(Paragraph::new(row0), header_splits[0]);
    f.render_widget(Paragraph::new(row1), header_splits[1]);
    f.render_widget(Paragraph::new(row2), header_splits[2]);

    // -----------------------------------------------------------------------
    // Response block
    // -----------------------------------------------------------------------

    let Some(resp_area) = response_area else {
        return (0, 0);
    };

    let divider_area = Rect {
        x: resp_area.x,
        y: resp_area.y,
        width: resp_area.width,
        height: 1,
    };
    let content_area = if resp_area.height > 1 {
        Rect {
            x: resp_area.x,
            y: resp_area.y + 1,
            width: resp_area.width,
            height: resp_area.height - 1,
        }
    } else {
        resp_area
    };

    // Draw divider
    let divider_style = if is_selected {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let divider: String = std::iter::repeat('─')
        .take(resp_area.width as usize)
        .collect();
    f.render_widget(Paragraph::new(divider).style(divider_style), divider_area);

    // Render response content
    match &entry.meta.last_model_response {
        Some(response) if !response.is_empty() => {
            let md_text = tui_markdown::from_str(response);
            let scroll_offset = if is_selected { response_scroll } else { 0 };
            let para = Paragraph::new(md_text)
                .wrap(ratatui::widgets::Wrap { trim: false })
                .scroll((scroll_offset, 0));
            f.render_widget(para, content_area);

            // Scroll hint on selected card
            if is_selected && scroll_offset > 0 {
                let hint = Paragraph::new("▲ PgUp")
                    .style(Style::default().fg(Color::DarkGray))
                    .alignment(Alignment::Right);
                f.render_widget(hint, divider_area);
            }
        }
        _ => {
            let placeholder =
                Paragraph::new("no response yet").style(Style::default().fg(Color::DarkGray));
            f.render_widget(placeholder, content_area);
        }
    }

    (content_area.height, content_area.width)
}

// ---------------------------------------------------------------------------
// Keybindings bar
// ---------------------------------------------------------------------------

fn render_keybindings_bar(f: &mut Frame, area: Rect) {
    let bar = Paragraph::new(
        "[n] New  [d] Delete  [Enter] Open  [←↓↑→/hjkl] Navigate  [PgUp/PgDn] Scroll response  [q] Quit",
    )
    .style(Style::default().fg(Color::DarkGray));
    f.render_widget(bar, area);
}
