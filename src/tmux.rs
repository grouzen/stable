use anyhow::{Context, Result};
use crossterm::terminal;
use regex::Regex;
use std::process::Command;
use tmux_interface::{NewWindow, SendKeys, Tmux};

const SESSION: &str = "stable";

/// Replace non-`[a-zA-Z0-9_-]` chars with `-`, collapse consecutive dashes, trim edges.
pub fn sanitize_name(s: &str) -> String {
    let s = s.trim();
    let sanitized: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    Regex::new(r"-{2,}")
        .unwrap()
        .replace_all(&sanitized, "-")
        .trim_matches('-')
        .to_string()
}

/// Ensure the `stable` tmux session exists; create it detached if not.
/// Uses raw `Command` so that a tmux server is started automatically when
/// none is running (tmux_interface's `HasSession` errors out in that case).
pub fn ensure_session() -> Result<()> {
    let has = Command::new("tmux")
        .args(["has-session", "-t", SESSION])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !has {
        // Create the session detached with the current terminal size so panes
        // aren't stuck at the tmux default (80×24).
        let (cols, rows) = terminal::size().unwrap_or((220, 50));
        Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s",
                SESSION,
                "-x",
                &cols.to_string(),
                "-y",
                &rows.to_string(),
            ])
            .status()
            .context("failed to create tmux session")?;
    }

    Ok(())
}

/// Create a new window in the `stable` session with the given working directory and name.
/// Returns the window index (parsed from `#{window_index}` format output).
pub fn new_window(dir: &str, name: &str) -> Result<usize> {
    let output = Tmux::with_command(
        NewWindow::new()
            .detached()
            .target_window(SESSION)
            .start_directory(dir)
            .window_name(name)
            .print()
            .format("#{window_index}")
            .build(),
    )
    .output()
    .context("failed to create tmux window")?;

    let inner = output.into_inner();
    let stdout = String::from_utf8_lossy(&inner.stdout);
    let index: usize = stdout
        .trim()
        .parse()
        .with_context(|| format!("failed to parse window index from: {:?}", stdout.trim()))?;
    Ok(index)
}

/// Send keys to a tmux pane target (e.g. `stable:1.0`).
pub fn send_keys(target: &str, keys: &str) -> Result<()> {
    Tmux::with_command(SendKeys::new().target_pane(target).key(keys).build())
        .status()
        .with_context(|| format!("failed to send keys to {}", target))?;
    Ok(())
}

/// Capture the raw ANSI output of a pane (`-p -e -S -`).
pub fn capture_pane(target: &str) -> Result<String> {
    let output = Command::new("tmux")
        .args(["capture-pane", "-t", target, "-p", "-e", "-S", "-"])
        .output()
        .with_context(|| format!("failed to capture pane {}", target))?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Resize a tmux window to the given dimensions.
pub fn resize_window(target: &str, width: u16, height: u16) -> Result<()> {
    Command::new("tmux")
        .args([
            "resize-window",
            "-t",
            target,
            "-x",
            &width.to_string(),
            "-y",
            &height.to_string(),
        ])
        .status()
        .with_context(|| format!("failed to resize window {}", target))?;
    Ok(())
}

/// Return the cursor position within the pane's visible screen as (col, row).
pub fn cursor_position(target: &str) -> Option<(u16, u16)> {
    let output = Command::new("tmux")
        .args([
            "display-message",
            "-t",
            target,
            "-p",
            "#{cursor_x} #{cursor_y}",
        ])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&output.stdout);
    let mut parts = s.trim().split_whitespace();
    let x: u16 = parts.next()?.parse().ok()?;
    let y: u16 = parts.next()?.parse().ok()?;
    Some((x, y))
}

/// Check whether a pane is alive by querying its pid.
pub fn is_alive(target: &str) -> bool {
    Command::new("tmux")
        .args(["display-message", "-t", target, "-p", "#{pane_pid}"])
        .output()
        .map(|o| !o.stdout.trim_ascii().is_empty())
        .unwrap_or(false)
}
