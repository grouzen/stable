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

/// Truncates a string to `max` chars, appending `…` if needed.
fn truncate(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        let t: String = chars[..max.saturating_sub(1)].iter().collect();
        format!("{}…", t)
    }
}

/// Returns the first newline-delimited line of a prompt string.
fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("")
}

/// Replaces the home directory prefix with `~` for compact display.
fn shellify_dir(dir: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if let Some(rest) = dir.strip_prefix(&home) {
            return format!("~{}", rest);
        }
    }
    dir.to_string()
}

/// Formats a millisecond duration into a human-readable string
/// (e.g. "3h 12m", "45m", "< 1m").
fn format_uptime(ms: u64) -> String {
    let secs = ms / 1000;
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    if hours > 0 {
        format!("{}h {}m", hours, mins)
    } else if mins > 0 {
        format!("{}m", mins)
    } else if secs > 0 {
        format!("{}s", secs)
    } else {
        "< 1s".to_string()
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

    // Apply 1-cell left/right inner padding
    let raw_inner = block.inner(area);
    f.render_widget(block, area);

    if raw_inner.height == 0 || raw_inner.width < 2 {
        return (0, 0);
    }
    let inner = Rect {
        x: raw_inner.x + 1,
        y: raw_inner.y,
        width: raw_inner.width.saturating_sub(2),
        height: raw_inner.height,
    };

    // -----------------------------------------------------------------------
    // Compute header content
    // -----------------------------------------------------------------------

    // Row 0: ctx + work time (left) + status (right) — always height 3
    let sym = status_symbol(&entry.meta.status);
    let lbl = status_label(&entry.meta.status);
    let col = status_color(&entry.meta.status);

    let ctx_text = if let Some(ctx) = &entry.meta.context {
        let used = format_tokens(ctx.used);
        if let Some(total) = ctx.total {
            format!("{}/{}", used, format_tokens(total))
        } else {
            used
        }
    } else {
        "—".to_string()
    };

    let work_text = if entry.meta.total_work_ms > 0 {
        format!(" ⏱ {}", format_uptime(entry.meta.total_work_ms))
    } else {
        String::new()
    };
    let left_text = format!("{}{}", ctx_text, work_text);

    let status_str = format!("{} {}", sym, lbl);
    let avail = inner.width as usize;
    let padding = avail.saturating_sub(left_text.chars().count() + status_str.len());
    let row0 = Line::from(vec![
        Span::raw(format!("{}{}", left_text, " ".repeat(padding))),
        Span::styled(status_str, Style::default().fg(col)),
    ]);

    // Rows 1 & 2: prompts — single line, truncated, ">" prefix
    let fp_raw = entry.meta.first_prompt.as_deref().unwrap_or("");
    let lp_raw = entry.meta.last_prompt.as_deref().unwrap_or("");
    let fp_text = first_line(fp_raw);
    let lp_text = first_line(lp_raw);

    // Hide last prompt if it equals the first (e.g. single-message agents)
    let show_last = !lp_text.is_empty() && lp_text != fp_text;

    let prompt_h: u16 = 2; // 1 text + 1 bottom margin
    let row1_h = prompt_h;
    let row2_h = if show_last { prompt_h } else { 0 };
    let row0_h: u16 = 2;

    // --- Info row A: directory ---
    let dir_str = shellify_dir(&entry.config.directory);
    let info_a = Line::from(vec![Span::styled(
        dir_str,
        Style::default().fg(Color::DarkGray),
    )]);

    // --- Info row B: agent_type · model_name ---
    let agent_type = &entry.config.agent_type;
    let model_str = entry.meta.model_name.as_deref().unwrap_or("—");
    let info_b = Line::from(vec![
        Span::styled(agent_type.as_str(), Style::default().fg(Color::DarkGray)),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::styled(model_str, Style::default().fg(Color::DarkGray)),
    ]);

    let info_row_h: u16 = 1;
    let info_row_b_h: u16 = 2; // 1 text + 1 empty margin line
    let header_lines = row0_h + info_row_h + info_row_b_h + row1_h + row2_h;

    // -----------------------------------------------------------------------
    // Layout: header + response block
    // -----------------------------------------------------------------------

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
    // Render header rows
    // -----------------------------------------------------------------------

    // Helper: render a line with bottom padding inside a slot
    let render_centered = |f: &mut Frame, slot: Rect, line: Line, h: u16| {
        if slot.height == 0 || h == 0 {
            return;
        }
        let sub = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(h.saturating_sub(1).max(1)),
                Constraint::Length(1),
            ])
            .split(slot);
        f.render_widget(Paragraph::new(line), sub[0]);
    };

    // Helper: render a single-line truncated prompt with a thick colored left
    // border, dimmed background, and a bottom margin line.
    let render_prompt = |f: &mut Frame, slot: Rect, text: &str, h: u16| {
        if slot.height == 0 || h == 0 {
            return;
        }
        // Split slot into text row + bottom margin
        let sub = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(h.saturating_sub(1)),
            ])
            .split(slot);
        let text_area = sub[0];

        // Block: thick left border (Cyan) + dim background fill
        let prompt_block = Block::default()
            .borders(Borders::LEFT)
            .border_type(BorderType::Thick)
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Rgb(20, 24, 30)));
        let text_inner = prompt_block.inner(text_area);
        f.render_widget(prompt_block, text_area);

        // Truncate to inner width (left border consumed 1 char)
        let usable = text_inner.width as usize;
        let content = if text.is_empty() {
            Paragraph::new(Span::styled("—", Style::default().fg(Color::DarkGray)))
                .style(Style::default().bg(Color::Rgb(20, 24, 30)))
        } else {
            Paragraph::new(Span::raw(truncate(text, usable)))
                .style(Style::default().bg(Color::Rgb(20, 24, 30)))
        };
        f.render_widget(content, text_inner);
    };

    // Build header row areas
    let mut constraints = vec![
        Constraint::Length(row0_h),
        Constraint::Length(info_row_h),
        Constraint::Length(info_row_b_h),
        Constraint::Length(row1_h),
    ];
    if show_last {
        constraints.push(Constraint::Length(row2_h));
    }
    let header_splits = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(header_area);

    render_centered(f, header_splits[0], row0, row0_h);
    f.render_widget(Paragraph::new(info_a), header_splits[1]);
    f.render_widget(Paragraph::new(info_b), header_splits[2]);
    render_prompt(f, header_splits[3], fp_text, row1_h);
    if show_last {
        render_prompt(f, header_splits[4], lp_text, row2_h);
    }

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
