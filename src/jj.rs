//! Jujutsu (jj) VCS commands with token-optimized output.
//!
//! This module provides compact output for jj commands, achieving 60-90% token savings
//! through smart filtering, grouping, and truncation.
//!
//! # Supported Commands
//!
//! ## Phase 1 (Core)
//! - `jj status`: Compact working copy state
//! - `jj log`: Truncated commit history
//! - `jj diff`: Stat summary + compact hunks
//! - `jj show`: One-line summary + compact diff
//!
//! ## Phase 2 (Write Operations)
//! - `jj describe`: "ok ✓" confirmation
//! - `jj new`: "ok ✓ {change_id}"
//! - `jj squash`: "ok ✓ squashed"
//! - `jj absorb`: Summary of absorbed changes
//! - `jj rebase`: "ok ✓ rebased N commits"
//! - `jj op log`: Compact operation history
//!
//! ## Phase 3 (Git Interop)
//! - `jj git push`: "ok ✓ pushed {bookmark}"
//! - `jj git fetch`: "ok fetched (N new)"
//! - `jj bookmark`: Compact list / "ok ✓" for mutations

use crate::git::compact_diff;
use crate::tracking;
use anyhow::{Context, Result};
use std::ffi::OsString;
use std::process::Command;

/// Jujutsu command types
#[derive(Debug, Clone)]
pub enum JjCommand {
    /// Show working copy status
    Status,
    /// Show commit log
    Log { limit: Option<usize> },
    /// Show diff
    Diff,
    /// Show commit details
    Show,
    /// Set commit message
    Describe,
    /// Create new commit
    New,
    /// Squash changes into parent
    Squash,
    /// Auto-distribute changes to ancestors
    Absorb,
    /// Rebase commits
    Rebase,
    /// Bookmark operations
    Bookmark,
    /// Git operations (push, fetch, clone)
    Git,
    /// Operation log
    OpLog,
    /// Undo last operation
    Undo,
}

/// Main entry point for jj commands
pub fn run(cmd: JjCommand, args: &[String], verbose: u8) -> Result<()> {
    match cmd {
        JjCommand::Status => run_status(args, verbose),
        JjCommand::Log { limit } => run_log(args, limit, verbose),
        JjCommand::Diff => run_diff(args, verbose),
        JjCommand::Show => run_show(args, verbose),
        JjCommand::Describe => run_describe(args, verbose),
        JjCommand::New => run_new(args, verbose),
        JjCommand::Squash => run_squash(args, verbose),
        JjCommand::Absorb => run_absorb(args, verbose),
        JjCommand::Rebase => run_rebase(args, verbose),
        JjCommand::Bookmark => run_bookmark(args, verbose),
        JjCommand::Git => run_git(args, verbose),
        JjCommand::OpLog => run_op_log(args, verbose),
        JjCommand::Undo => run_undo(args, verbose),
    }
}

/// Filter jj status output to compact format
///
/// # Input Format
/// ```text
/// The working copy has no changes.
/// Working copy  (@) : kntqzsqt d7439b06 (empty) (no description set)
/// Parent commit (@-): orrkosyo 7fd1a60b master | (empty) Merge pull request #6
/// ```
///
/// # Output Format
/// ```text
/// @ kntqzsqt d7439b06 (empty)
/// @- orrkosyo 7fd1a60b master
/// ```
pub fn filter_jj_status(output: &str) -> String {
    let mut result = Vec::new();
    let mut file_changes: Vec<String> = Vec::new();
    let mut working_copy_line = String::new();
    let mut parent_line = String::new();

    for line in output.lines() {
        let trimmed = line.trim();

        // Skip verbose messages
        if trimmed.starts_with("The working copy")
            || trimmed.starts_with("Rebased")
            || trimmed.is_empty()
        {
            continue;
        }

        // Capture file changes (M, A, D, R, C prefixes)
        if (trimmed.starts_with("M ")
            || trimmed.starts_with("A ")
            || trimmed.starts_with("D ")
            || trimmed.starts_with("R ")
            || trimmed.starts_with("C "))
            && trimmed.len() > 2
        {
            file_changes.push(format!("  {}", trimmed));
            continue;
        }

        // Parse working copy line
        if trimmed.starts_with("Working copy") {
            working_copy_line = parse_commit_line(trimmed, "@");
            continue;
        }

        // Parse parent commit line
        if trimmed.starts_with("Parent commit") {
            parent_line = parse_commit_line(trimmed, "@-");
            continue;
        }
    }

    // Build output: working copy first, then changes, then parent
    if !working_copy_line.is_empty() {
        result.push(working_copy_line);
    }

    for change in file_changes {
        result.push(change);
    }

    if !parent_line.is_empty() {
        result.push(parent_line);
    }

    if result.is_empty() {
        "Clean working copy".to_string()
    } else {
        result.join("\n")
    }
}

/// Parse a jj commit line into compact format
/// Input: "Working copy  (@) : kntqzsqt d7439b06 (empty) (no description set)"
/// Output: "@ kntqzsqt d7439b06 (empty)"
fn parse_commit_line(line: &str, prefix: &str) -> String {
    // Find the colon separator after (@) or (@-)
    if let Some(colon_pos) = line.find(": ") {
        let after_colon = &line[colon_pos + 2..];
        let parts: Vec<&str> = after_colon.split_whitespace().collect();

        if parts.len() >= 2 {
            let change_id = parts[0];
            let commit_id = parts[1];

            // Look for bookmark/branch name (after commit_id, before description)
            let mut bookmark = String::new();

            // Check for (empty) marker
            let has_empty = after_colon.contains("(empty)");

            // Extract bookmark if present (appears before |)
            if let Some(pipe_pos) = after_colon.find(" | ") {
                // Format: "change_id commit_id bookmark | description"
                let before_pipe = &after_colon[..pipe_pos];
                let pipe_parts: Vec<&str> = before_pipe.split_whitespace().collect();
                if pipe_parts.len() >= 3 {
                    bookmark = pipe_parts[2].to_string();
                }
            } else if parts.len() >= 3 && !parts[2].starts_with('(') {
                // No pipe, but might have bookmark directly
                bookmark = parts[2].to_string();
            }

            // Build compact output
            let mut output = format!("{} {} {}", prefix, change_id, commit_id);

            if !bookmark.is_empty() {
                output.push_str(&format!(" {}", bookmark));
            }

            if has_empty {
                output.push_str(" (empty)");
            }

            return output;
        }
    }

    // Fallback: return prefix with raw content
    format!("{} {}", prefix, line)
}

/// Filter jj log output to compact format
///
/// # Strategy
/// - Limit entries (default 5)
/// - Strip email addresses
/// - Strip full timestamps
/// - Truncate long messages at 60 chars
/// - Preserve graph characters (@ ○ ◆ │ ~)
pub fn filter_jj_log(output: &str, limit: usize) -> String {
    let mut result = Vec::new();
    let mut entry_count = 0;
    let mut current_entry: Option<String> = None;

    for line in output.lines() {
        // Check for new entry (starts with graph char or @)
        let trimmed = line.trim_start();
        let is_new_entry = trimmed.starts_with('@')
            || trimmed.starts_with('\u{25cb}') // ○
            || trimmed.starts_with('\u{25c6}') // ◆
            || trimmed.starts_with('\u{25cf}') // ●
            || trimmed.starts_with('|')
            || trimmed.starts_with('\u{2502}'); // │

        if is_new_entry && trimmed.contains(' ') {
            // Save previous entry if exists
            if let Some(entry) = current_entry.take() {
                if entry_count < limit {
                    result.push(entry);
                    entry_count += 1;
                }
            }

            // Parse new entry
            current_entry = Some(parse_log_entry(line));
        } else if line.trim().starts_with('│') || line.trim().starts_with('|') {
            // Graph continuation line - skip or include based on content
            let content = line
                .trim()
                .trim_start_matches('│')
                .trim_start_matches('|')
                .trim();
            if !content.is_empty() && !content.contains('@') && current_entry.is_some() {
                // This might be the description line
                if let Some(ref mut entry) = current_entry {
                    // Append description if it's meaningful
                    if !content.starts_with("(empty)") && !content.contains("no description") {
                        let truncated = truncate_message(content, 50);
                        entry.push_str(&format!(" {}", truncated));
                    }
                }
            }
        }
    }

    // Don't forget the last entry
    if let Some(entry) = current_entry {
        if entry_count < limit {
            result.push(entry);
        }
    }

    if result.is_empty() {
        "No commits".to_string()
    } else {
        result.join("\n")
    }
}

/// Parse a single jj log entry line
fn parse_log_entry(line: &str) -> String {
    let trimmed = line.trim();

    // Extract graph character(s)
    let graph_char = if trimmed.starts_with('@') {
        "@"
    } else if trimmed.starts_with('\u{25cb}') {
        "\u{25cb}" // ○
    } else if trimmed.starts_with('\u{25c6}') {
        "\u{25c6}" // ◆
    } else if trimmed.starts_with('\u{25cf}') {
        "\u{25cf}" // ●
    } else {
        ""
    };

    // Split the rest
    let rest = trimmed
        .trim_start_matches('@')
        .trim_start_matches('\u{25cb}')
        .trim_start_matches('\u{25c6}')
        .trim_start_matches('\u{25cf}')
        .trim();

    let parts: Vec<&str> = rest.split_whitespace().collect();

    if parts.is_empty() {
        return graph_char.to_string();
    }

    // First part is change_id
    let change_id = parts[0];

    // Look for commit hash (8-char hex)
    let mut commit_id = "";
    let mut bookmark = "";

    for (i, part) in parts.iter().enumerate().skip(1) {
        // Skip email addresses
        if part.contains('@') && part.contains('.') {
            continue;
        }
        // Skip timestamps (YYYY-MM-DD)
        if part.len() == 10 && part.chars().nth(4) == Some('-') {
            continue;
        }
        // Skip time (HH:MM:SS)
        if part.len() == 8 && part.chars().nth(2) == Some(':') {
            continue;
        }

        // 8-char hex is likely commit id
        if part.len() == 8 && part.chars().all(|c| c.is_ascii_hexdigit()) {
            commit_id = part;
            // Check if next part is a bookmark
            if i + 1 < parts.len() {
                let next = parts[i + 1];
                if !next.starts_with('(') && !next.contains('@') && !next.contains('-') {
                    bookmark = next;
                }
            }
            break;
        }
    }

    let mut result = format!("{} {} {}", graph_char, change_id, commit_id);
    if !bookmark.is_empty() {
        result.push_str(&format!(" {}", bookmark));
    }

    result
}

/// Truncate a message to max chars, adding "..." if truncated
fn truncate_message(msg: &str, max: usize) -> String {
    if msg.chars().count() <= max {
        msg.to_string()
    } else {
        let truncated: String = msg.chars().take(max - 3).collect();
        format!("{}...", truncated)
    }
}

/// Filter jj op log output to compact format
///
/// # Input Format
/// ```text
/// @  d3b77addea49 user@host 3 minutes ago, lasted 3 milliseconds
/// │  squash commits into f7fb5943a6b9460eb106dba2fac5cac1625c6f7a
/// │  args: jj squash
/// ```
///
/// # Output Format
/// ```text
/// @ d3b77ad 3m ago squash
/// ```
pub fn filter_jj_op_log(output: &str, limit: usize) -> String {
    let mut result = Vec::new();
    let mut current_graph = String::new();
    let mut current_op_id = String::new();
    let mut current_time = String::new();
    let mut current_op = String::new();

    for line in output.lines() {
        let trimmed = line.trim();

        // New operation entry
        if trimmed.starts_with('@') || trimmed.starts_with('\u{25cb}') {
            // Save previous if exists
            if !current_op_id.is_empty() && result.len() < limit {
                result.push(format!(
                    "{} {} {} {}",
                    current_graph, current_op_id, current_time, current_op
                ));
            }

            // Parse new entry
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            current_graph = if trimmed.starts_with('@') {
                "@".to_string()
            } else {
                "\u{25cb}".to_string()
            };

            if parts.len() >= 2 {
                // Truncate op_id to 7 chars
                current_op_id = parts[1].chars().take(7).collect();
            }

            // Find time (X minutes ago, X hours ago, etc.)
            current_time = extract_relative_time(trimmed);
            current_op = String::new();
        } else if trimmed.starts_with('│') || trimmed.starts_with('|') {
            // Operation description line
            let content = trimmed
                .trim_start_matches('│')
                .trim_start_matches('|')
                .trim();

            if content.starts_with("args:") {
                // Extract command from args
                let cmd = content.trim_start_matches("args:").trim();
                current_op = cmd.to_string();
            } else if current_op.is_empty() && !content.is_empty() {
                // First description line
                current_op = truncate_message(content, 30);
            }
        }
    }

    // Don't forget last entry
    if !current_op_id.is_empty() && result.len() < limit {
        result.push(format!(
            "{} {} {} {}",
            current_graph, current_op_id, current_time, current_op
        ));
    }

    if result.is_empty() {
        "No operations".to_string()
    } else {
        result.join("\n")
    }
}

/// Extract relative time from jj output and shorten it
/// "3 minutes ago, lasted 3 milliseconds" -> "3m ago"
fn extract_relative_time(line: &str) -> String {
    // Look for patterns like "X minutes ago", "X hours ago", etc.
    let patterns = [
        ("seconds ago", "s ago"),
        ("second ago", "s ago"),
        ("minutes ago", "m ago"),
        ("minute ago", "m ago"),
        ("hours ago", "h ago"),
        ("hour ago", "h ago"),
        ("days ago", "d ago"),
        ("day ago", "d ago"),
        ("weeks ago", "w ago"),
        ("week ago", "w ago"),
    ];

    for (long, short) in patterns {
        if let Some(pos) = line.find(long) {
            // Find the number before this pattern
            let before = &line[..pos];
            let words: Vec<&str> = before.split_whitespace().collect();
            if let Some(num) = words.last() {
                if num.parse::<u64>().is_ok() {
                    return format!("{}{}", num, short);
                }
            }
        }
    }

    "now".to_string()
}

// ============================================================================
// Command Implementations
// ============================================================================

fn run_status(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let output = Command::new("jj")
        .arg("status")
        .args(args)
        .output()
        .context("Failed to run jj status")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("{}", stderr);
        std::process::exit(output.status.code().unwrap_or(1));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let filtered = filter_jj_status(&stdout);

    if verbose > 0 {
        eprintln!("jj status (filtered):");
    }

    println!("{}", filtered);

    timer.track(
        &format!("jj status {}", args.join(" ")),
        &format!("rtk jj status {}", args.join(" ")),
        &stdout,
        &filtered,
    );

    Ok(())
}

fn run_log(args: &[String], limit: Option<usize>, verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();
    let entry_limit = limit.unwrap_or(5);

    let output = Command::new("jj")
        .arg("log")
        .args(args)
        .output()
        .context("Failed to run jj log")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("{}", stderr);
        std::process::exit(output.status.code().unwrap_or(1));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let filtered = filter_jj_log(&stdout, entry_limit);

    if verbose > 0 {
        eprintln!("jj log (filtered, limit {}):", entry_limit);
    }

    println!("{}", filtered);

    timer.track(
        &format!("jj log {}", args.join(" ")),
        &format!("rtk jj log {}", args.join(" ")),
        &stdout,
        &filtered,
    );

    Ok(())
}

fn run_diff(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    // First get --stat output
    let stat_output = Command::new("jj")
        .arg("diff")
        .arg("--stat")
        .args(args)
        .output()
        .context("Failed to run jj diff --stat")?;

    let stat_stdout = String::from_utf8_lossy(&stat_output.stdout);

    // Then get full diff for compacting
    let diff_output = Command::new("jj")
        .arg("diff")
        .args(args)
        .output()
        .context("Failed to run jj diff")?;

    if !diff_output.status.success() {
        let stderr = String::from_utf8_lossy(&diff_output.stderr);
        eprintln!("{}", stderr);
        std::process::exit(diff_output.status.code().unwrap_or(1));
    }

    let diff_stdout = String::from_utf8_lossy(&diff_output.stdout);

    if verbose > 0 {
        eprintln!("jj diff (compact):");
    }

    // Print stat summary first
    if !stat_stdout.trim().is_empty() {
        println!("{}", stat_stdout.trim());
    }

    // Then compact diff
    let mut final_output = stat_stdout.to_string();
    if !diff_stdout.is_empty() {
        if !stat_stdout.trim().is_empty() {
            println!("\n--- Changes ---");
            final_output.push_str("\n--- Changes ---\n");
        }
        let compacted = compact_diff(&diff_stdout, 100);
        println!("{}", compacted);
        final_output.push_str(&compacted);
    }

    timer.track(
        &format!("jj diff {}", args.join(" ")),
        &format!("rtk jj diff {}", args.join(" ")),
        &diff_stdout,
        &final_output,
    );

    Ok(())
}

fn run_show(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    // Get commit info
    let output = Command::new("jj")
        .arg("show")
        .args(args)
        .output()
        .context("Failed to run jj show")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("{}", stderr);
        std::process::exit(output.status.code().unwrap_or(1));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    if verbose > 0 {
        eprintln!("jj show (compact):");
    }

    // jj show output is already somewhat compact, but we can still apply compact_diff
    // to the diff portion
    let mut lines = stdout.lines();
    let mut header_lines = Vec::new();
    let mut diff_content = String::new();
    let mut in_diff = false;

    for line in lines.by_ref() {
        if line.starts_with("diff --git") || line.starts_with("---") || line.starts_with("+++") {
            in_diff = true;
        }

        if in_diff {
            diff_content.push_str(line);
            diff_content.push('\n');
        } else {
            header_lines.push(line);
        }
    }

    // Print header (first few lines)
    for line in header_lines.iter().take(5) {
        println!("{}", line);
    }

    let header_output: String = header_lines
        .iter()
        .take(5)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");

    // Compact and print diff
    let mut final_output = header_output.clone();
    if !diff_content.is_empty() {
        let compacted = compact_diff(&diff_content, 100);
        println!("{}", compacted);
        final_output.push('\n');
        final_output.push_str(&compacted);
    }

    timer.track(
        &format!("jj show {}", args.join(" ")),
        &format!("rtk jj show {}", args.join(" ")),
        &stdout,
        &final_output,
    );

    Ok(())
}

fn run_describe(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let output = Command::new("jj")
        .arg("describe")
        .args(args)
        .output()
        .context("Failed to run jj describe")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);

    if output.status.success() {
        if verbose > 0 {
            eprintln!("jj describe succeeded");
        }
        println!("ok \u{2713}");

        timer.track(
            &format!("jj describe {}", args.join(" ")),
            &format!("rtk jj describe {}", args.join(" ")),
            &combined,
            "ok \u{2713}",
        );
    } else {
        eprintln!("FAILED: jj describe");
        eprintln!("{}", combined);
        std::process::exit(output.status.code().unwrap_or(1));
    }

    Ok(())
}

fn run_new(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let output = Command::new("jj")
        .arg("new")
        .args(args)
        .output()
        .context("Failed to run jj new")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);

    if output.status.success() {
        // Try to extract change_id from output
        let change_id = extract_change_id(&combined);

        let msg = if let Some(id) = change_id {
            format!("ok \u{2713} {}", id)
        } else {
            "ok \u{2713}".to_string()
        };

        if verbose > 0 {
            eprintln!("jj new succeeded");
        }
        println!("{}", msg);

        timer.track(
            &format!("jj new {}", args.join(" ")),
            &format!("rtk jj new {}", args.join(" ")),
            &combined,
            &msg,
        );
    } else {
        eprintln!("FAILED: jj new");
        eprintln!("{}", combined);
        std::process::exit(output.status.code().unwrap_or(1));
    }

    Ok(())
}

/// Extract change_id from jj output
fn extract_change_id(output: &str) -> Option<String> {
    // jj output often contains change_id in format like "Working copy now at: abc12345"
    // or "Created new commit abc12345"
    for line in output.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        for word in words {
            // Change IDs are typically 8 lowercase alphanumeric chars
            if word.len() == 8
                && word
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            {
                return Some(word.to_string());
            }
        }
    }
    None
}

fn run_squash(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let output = Command::new("jj")
        .arg("squash")
        .args(args)
        .output()
        .context("Failed to run jj squash")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);

    if output.status.success() {
        if verbose > 0 {
            eprintln!("jj squash succeeded");
        }
        println!("ok \u{2713} squashed");

        timer.track(
            &format!("jj squash {}", args.join(" ")),
            &format!("rtk jj squash {}", args.join(" ")),
            &combined,
            "ok \u{2713} squashed",
        );
    } else {
        eprintln!("FAILED: jj squash");
        eprintln!("{}", combined);
        std::process::exit(output.status.code().unwrap_or(1));
    }

    Ok(())
}

fn run_absorb(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let output = Command::new("jj")
        .arg("absorb")
        .args(args)
        .output()
        .context("Failed to run jj absorb")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);

    if output.status.success() {
        // Count absorbed changes from output
        let absorbed_count = count_absorbed_changes(&combined);

        let msg = if absorbed_count > 0 {
            format!("ok \u{2713} absorbed {} changes", absorbed_count)
        } else {
            "ok \u{2713} absorbed".to_string()
        };

        if verbose > 0 {
            eprintln!("jj absorb succeeded");
        }
        println!("{}", msg);

        timer.track(
            &format!("jj absorb {}", args.join(" ")),
            &format!("rtk jj absorb {}", args.join(" ")),
            &combined,
            &msg,
        );
    } else {
        eprintln!("FAILED: jj absorb");
        eprintln!("{}", combined);
        std::process::exit(output.status.code().unwrap_or(1));
    }

    Ok(())
}

fn count_absorbed_changes(output: &str) -> usize {
    // Count lines that indicate absorbed hunks
    output
        .lines()
        .filter(|line| line.contains("Absorbed") || line.contains("absorbed"))
        .count()
}

fn run_rebase(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let output = Command::new("jj")
        .arg("rebase")
        .args(args)
        .output()
        .context("Failed to run jj rebase")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);

    if output.status.success() {
        // Try to extract commit count
        let commit_count = extract_rebased_count(&combined);

        let msg = if commit_count > 0 {
            format!("ok \u{2713} rebased {} commits", commit_count)
        } else {
            "ok \u{2713} rebased".to_string()
        };

        if verbose > 0 {
            eprintln!("jj rebase succeeded");
        }
        println!("{}", msg);

        timer.track(
            &format!("jj rebase {}", args.join(" ")),
            &format!("rtk jj rebase {}", args.join(" ")),
            &combined,
            &msg,
        );
    } else {
        eprintln!("FAILED: jj rebase");
        eprintln!("{}", combined);
        std::process::exit(output.status.code().unwrap_or(1));
    }

    Ok(())
}

fn extract_rebased_count(output: &str) -> usize {
    // Look for patterns like "Rebased 3 commits"
    for line in output.lines() {
        if line.contains("Rebased") || line.contains("rebased") {
            for word in line.split_whitespace() {
                if let Ok(n) = word.parse::<usize>() {
                    return n;
                }
            }
        }
    }
    0
}

fn run_bookmark(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    // Detect if this is a list operation or mutation
    let is_mutation = args.iter().any(|a| {
        a == "set"
            || a == "delete"
            || a == "create"
            || a == "move"
            || a == "rename"
            || a == "-d"
            || a == "--delete"
    });

    let output = Command::new("jj")
        .arg("bookmark")
        .args(args)
        .output()
        .context("Failed to run jj bookmark")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);

    if !output.status.success() {
        eprintln!("FAILED: jj bookmark");
        eprintln!("{}", combined);
        std::process::exit(output.status.code().unwrap_or(1));
    }

    if is_mutation {
        if verbose > 0 {
            eprintln!("jj bookmark mutation succeeded");
        }
        println!("ok \u{2713}");

        timer.track(
            &format!("jj bookmark {}", args.join(" ")),
            &format!("rtk jj bookmark {}", args.join(" ")),
            &combined,
            "ok \u{2713}",
        );
    } else {
        // List mode: compact the output
        let filtered = filter_bookmark_list(&stdout);

        if verbose > 0 {
            eprintln!("jj bookmark list (filtered):");
        }
        println!("{}", filtered);

        timer.track(
            &format!("jj bookmark {}", args.join(" ")),
            &format!("rtk jj bookmark {}", args.join(" ")),
            &stdout,
            &filtered,
        );
    }

    Ok(())
}

fn filter_bookmark_list(output: &str) -> String {
    let mut result = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Format: "main: orrkosyo 7fd1a60b (empty) Merge pull request #6"
        // Compact to: "main: orrkosyo 7fd1a60b"
        if let Some(colon_pos) = trimmed.find(':') {
            let name = &trimmed[..colon_pos];
            let rest = &trimmed[colon_pos + 1..].trim();

            // Extract change_id and commit_id (first two words)
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 2 {
                let change_id = parts[0];
                let commit_id = parts[1];

                // Check if tracked
                let tracked = if line.contains("@origin") || line.contains("(tracked)") {
                    " (tracked)"
                } else {
                    ""
                };

                result.push(format!("{}: {} {}{}", name, change_id, commit_id, tracked));
            } else {
                result.push(trimmed.to_string());
            }
        } else {
            // Might be a continuation line for tracking info
            if trimmed.starts_with('@') {
                continue; // Skip tracking detail lines
            }
            result.push(trimmed.to_string());
        }
    }

    if result.is_empty() {
        "No bookmarks".to_string()
    } else {
        result.join("\n")
    }
}

fn run_git(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    if args.is_empty() {
        // Just run `jj git` with no subcommand
        let status = Command::new("jj")
            .arg("git")
            .status()
            .context("Failed to run jj git")?;

        if !status.success() {
            std::process::exit(status.code().unwrap_or(1));
        }
        return Ok(());
    }

    let subcommand = &args[0];
    let sub_args = &args[1..];

    match subcommand.as_str() {
        "push" => run_git_push(sub_args, verbose),
        "fetch" => run_git_fetch(sub_args, verbose),
        _ => {
            // Passthrough for other git subcommands
            let output = Command::new("jj")
                .arg("git")
                .args(args)
                .output()
                .context("Failed to run jj git")?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            print!("{}", stdout);
            eprint!("{}", stderr);

            let args_str = args.join(" ");
            timer.track_passthrough(
                &format!("jj git {}", args_str),
                &format!("rtk jj git {} (passthrough)", args_str),
            );

            if !output.status.success() {
                std::process::exit(output.status.code().unwrap_or(1));
            }

            Ok(())
        }
    }
}

fn run_git_push(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let output = Command::new("jj")
        .args(["git", "push"])
        .args(args)
        .output()
        .context("Failed to run jj git push")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);

    if output.status.success() {
        // Extract bookmark name from output
        let bookmark = extract_pushed_bookmark(&combined);

        let msg = if let Some(b) = bookmark {
            format!("ok \u{2713} pushed {}", b)
        } else {
            "ok \u{2713} pushed".to_string()
        };

        if verbose > 0 {
            eprintln!("jj git push succeeded");
        }
        println!("{}", msg);

        timer.track(
            &format!("jj git push {}", args.join(" ")),
            &format!("rtk jj git push {}", args.join(" ")),
            &combined,
            &msg,
        );
    } else {
        eprintln!("FAILED: jj git push");
        eprintln!("{}", combined);
        std::process::exit(output.status.code().unwrap_or(1));
    }

    Ok(())
}

fn extract_pushed_bookmark(output: &str) -> Option<String> {
    // Look for bookmark name in push output
    for line in output.lines() {
        if line.contains("->") {
            let parts: Vec<&str> = line.split("->").collect();
            if let Some(first) = parts.first() {
                let words: Vec<&str> = first.split_whitespace().collect();
                if let Some(bookmark) = words.last() {
                    return Some(bookmark.to_string());
                }
            }
        }
        // Also check for "Branch changes to push" format
        if line.contains("push-") {
            for word in line.split_whitespace() {
                if word.starts_with("push-") {
                    return Some(word.to_string());
                }
            }
        }
    }
    None
}

fn run_git_fetch(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let output = Command::new("jj")
        .args(["git", "fetch"])
        .args(args)
        .output()
        .context("Failed to run jj git fetch")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);

    if output.status.success() {
        // Count new refs
        let new_count = count_fetched_refs(&combined);

        let msg = if new_count > 0 {
            format!("ok fetched ({} new)", new_count)
        } else {
            "ok fetched".to_string()
        };

        if verbose > 0 {
            eprintln!("jj git fetch succeeded");
        }
        println!("{}", msg);

        timer.track(
            &format!("jj git fetch {}", args.join(" ")),
            &format!("rtk jj git fetch {}", args.join(" ")),
            &combined,
            &msg,
        );
    } else {
        eprintln!("FAILED: jj git fetch");
        eprintln!("{}", combined);
        std::process::exit(output.status.code().unwrap_or(1));
    }

    Ok(())
}

fn count_fetched_refs(output: &str) -> usize {
    output
        .lines()
        .filter(|line| line.contains("bookmark") || line.contains("->") || line.contains("new"))
        .count()
}

fn run_op_log(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let output = Command::new("jj")
        .args(["op", "log"])
        .args(args)
        .output()
        .context("Failed to run jj op log")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("{}", stderr);
        std::process::exit(output.status.code().unwrap_or(1));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let filtered = filter_jj_op_log(&stdout, 5);

    if verbose > 0 {
        eprintln!("jj op log (filtered):");
    }

    println!("{}", filtered);

    timer.track(
        &format!("jj op log {}", args.join(" ")),
        &format!("rtk jj op log {}", args.join(" ")),
        &stdout,
        &filtered,
    );

    Ok(())
}

fn run_undo(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let output = Command::new("jj")
        .arg("undo")
        .args(args)
        .output()
        .context("Failed to run jj undo")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);

    if output.status.success() {
        if verbose > 0 {
            eprintln!("jj undo succeeded");
        }
        println!("ok \u{2713} undone");

        timer.track(
            &format!("jj undo {}", args.join(" ")),
            &format!("rtk jj undo {}", args.join(" ")),
            &combined,
            "ok \u{2713} undone",
        );
    } else {
        eprintln!("FAILED: jj undo");
        eprintln!("{}", combined);
        std::process::exit(output.status.code().unwrap_or(1));
    }

    Ok(())
}

/// Runs an unsupported jj subcommand by passing it through directly
pub fn run_passthrough(args: &[OsString], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    if verbose > 0 {
        eprintln!("jj passthrough: {:?}", args);
    }

    let status = Command::new("jj")
        .args(args)
        .status()
        .context("Failed to run jj")?;

    let args_str = tracking::args_display(args);
    timer.track_passthrough(
        &format!("jj {}", args_str),
        &format!("rtk jj {} (passthrough)", args_str),
    );

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_jj_status_clean() {
        let raw = r#"The working copy has no changes.
Working copy  (@) : kntqzsqt d7439b06 (empty) (no description set)
Parent commit (@-): orrkosyo 7fd1a60b master | (empty) Merge pull request #6
"#;
        let result = filter_jj_status(raw);
        assert!(result.contains("@ kntqzsqt d7439b06"));
        assert!(result.contains("(empty)"));
        assert!(!result.contains("The working copy has no changes"));
    }

    #[test]
    fn test_filter_jj_status_with_changes() {
        let raw = r#"Working copy changes:
M src/main.rs
A new_file.rs
Working copy  (@) : abc12345 def67890 Add new feature
Parent commit (@-): xyz98765 uvw54321 main | Previous commit
"#;
        let result = filter_jj_status(raw);
        assert!(result.contains("@ abc12345"));
        assert!(result.contains("M src/main.rs"));
        assert!(result.contains("A new_file.rs"));
        assert!(result.contains("@-"));
    }

    #[test]
    fn test_filter_jj_log() {
        let raw = r#"@  mpqrykyp user@email.com 2023-02-12 15:00:22 aef4df99
│  (empty) (no description set)
○  kntqzsqt user@email.com 2023-02-12 14:56:59 5d39e19d
│  Say goodbye
"#;
        let result = filter_jj_log(raw, 5);
        assert!(result.contains("@ mpqrykyp aef4df99"));
        assert!(result.contains("\u{25cb} kntqzsqt 5d39e19d")); // ○
        assert!(!result.contains("user@email.com")); // Email stripped
    }

    #[test]
    fn test_filter_jj_log_limit() {
        let raw = r#"@  aaa11111 user@email.com 2023-02-12 15:00:22 hash1111
│  msg1
○  bbb22222 user@email.com 2023-02-12 14:00:00 hash2222
│  msg2
○  ccc33333 user@email.com 2023-02-12 13:00:00 hash3333
│  msg3
○  ddd44444 user@email.com 2023-02-12 12:00:00 hash4444
│  msg4
"#;
        let result = filter_jj_log(raw, 2);
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_filter_jj_op_log() {
        let raw = r#"@  d3b77addea49 user@host 3 minutes ago, lasted 3 milliseconds
│  squash commits into f7fb5943a6b9460eb106dba2fac5cac1625c6f7a
│  args: jj squash
○  6fc1873c1180 user@host 3 minutes ago, lasted 1 milliseconds
│  snapshot working copy
│  args: jj st
"#;
        let result = filter_jj_op_log(raw, 5);
        assert!(result.contains("@ d3b77ad"));
        assert!(result.contains("3m ago"));
        assert!(result.contains("jj squash") || result.contains("squash"));
    }

    #[test]
    fn test_filter_jj_op_log_limit() {
        let raw = r#"@  op1 user@host 1 minutes ago, lasted 1 milliseconds
│  args: jj new
○  op2 user@host 2 minutes ago, lasted 1 milliseconds
│  args: jj describe
○  op3 user@host 3 minutes ago, lasted 1 milliseconds
│  args: jj squash
○  op4 user@host 4 minutes ago, lasted 1 milliseconds
│  args: jj log
"#;
        let result = filter_jj_op_log(raw, 2);
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_truncate_message() {
        let short = "Hello";
        assert_eq!(truncate_message(short, 10), "Hello");

        let long = "This is a very long message that should be truncated";
        let result = truncate_message(long, 20);
        assert!(result.len() <= 20);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_extract_relative_time() {
        assert_eq!(
            extract_relative_time("user@host 3 minutes ago, lasted"),
            "3m ago"
        );
        assert_eq!(
            extract_relative_time("user@host 1 hour ago, lasted"),
            "1h ago"
        );
        assert_eq!(
            extract_relative_time("user@host 5 seconds ago, lasted"),
            "5s ago"
        );
        assert_eq!(
            extract_relative_time("user@host 2 days ago, lasted"),
            "2d ago"
        );
    }

    #[test]
    fn test_extract_change_id() {
        let output = "Working copy now at: abc12345 def67890";
        assert_eq!(extract_change_id(output), Some("abc12345".to_string()));

        let output2 = "No change id here";
        assert_eq!(extract_change_id(output2), None);
    }

    #[test]
    fn test_parse_commit_line_with_bookmark() {
        let line = "Working copy  (@) : kntqzsqt d7439b06 master | Some message";
        let result = parse_commit_line(line, "@");
        assert!(result.contains("@ kntqzsqt d7439b06"));
        assert!(result.contains("master"));
    }

    #[test]
    fn test_parse_commit_line_empty() {
        let line = "Working copy  (@) : kntqzsqt d7439b06 (empty) (no description set)";
        let result = parse_commit_line(line, "@");
        assert!(result.contains("@ kntqzsqt d7439b06"));
        assert!(result.contains("(empty)"));
    }

    #[test]
    fn test_filter_bookmark_list() {
        let raw = r#"main: orrkosyo 7fd1a60b (empty) Merge pull request #6
feature: abc12345 def67890 Add feature
  @origin: abc12345 def67890
"#;
        let result = filter_bookmark_list(raw);
        assert!(result.contains("main: orrkosyo 7fd1a60b"));
        assert!(result.contains("feature: abc12345 def67890"));
    }

    #[test]
    fn test_filter_bookmark_list_tracked() {
        let raw = "main: abc12345 def67890 (tracked) @origin\n";
        let result = filter_bookmark_list(raw);
        assert!(result.contains("(tracked)"));
    }
}
