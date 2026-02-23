# RTK Jujutsu (jj) VCS Support Specification

**Status**: Draft v1.0  
**Author**: Claude  
**Date**: 2026-02-22  

## Executive Summary

This spec proposes adding Jujutsu (jj) version control support to RTK. jj is a modern Git-compatible VCS gaining adoption among developers using Claude Code and other AI coding assistants. Since jj outputs are often verbose (especially `jj log`, `jj status`, `jj diff`, `jj op log`), RTK filtering can achieve 60-90% token savings similar to existing git support.

## Background

### What is Jujutsu (jj)?

Jujutsu is a Git-compatible version control system with several key differences from Git:

1. **Working copy is a commit**: Changes are automatically committed as you work
2. **No staging area**: No index/staging concept - everything is commits
3. **Conflicts are first-class**: Conflicts are recorded in commits, not blocking
4. **Automatic rebase**: Descendants auto-rebase when you modify a commit
5. **Operation log**: Every operation is recorded (enables powerful undo)
6. **Bookmarks not branches**: Labels pointing to commits (not auto-advancing)
7. **Change IDs**: Stable identifiers that persist across rewrites

### Why jj Support for RTK?

1. **Growing adoption**: 26k+ GitHub stars, actively maintained by Google
2. **AI-assisted development**: jj's simpler mental model suits AI workflows
3. **Verbose output**: `jj log`, `jj status`, `jj diff` produce substantial output
4. **User demand**: Developers using jj with Claude Code want RTK optimization
5. **Git backend**: jj uses Git storage, so patterns similar to git.rs apply

## Proposed Commands

### High Priority (Phase 1)

| Command | jj Equivalent | Token Strategy | Expected Savings |
|---------|---------------|----------------|------------------|
| `rtk jj status` | `jj status` | Show working copy state compactly | 70-85% |
| `rtk jj log` | `jj log` | Truncate lines, limit entries, simplify format | 75-90% |
| `rtk jj diff` | `jj diff` | Stat summary + compact hunks | 70-85% |
| `rtk jj show` | `jj show` | One-line summary + compact diff | 75-85% |

### Medium Priority (Phase 2)

| Command | jj Equivalent | Token Strategy | Expected Savings |
|---------|---------------|----------------|------------------|
| `rtk jj describe` | `jj describe` | "ok ✓" confirmation | 90%+ |
| `rtk jj new` | `jj new` | "ok ✓ {change_id}" | 85-90% |
| `rtk jj squash` | `jj squash` | "ok ✓ squashed" | 85-90% |
| `rtk jj absorb` | `jj absorb` | Summary of absorbed changes | 80-90% |
| `rtk jj rebase` | `jj rebase` | "ok ✓ rebased N commits" | 85-90% |
| `rtk jj op log` | `jj op log` | Limit entries, simplify format | 75-85% |

### Lower Priority (Phase 3)

| Command | jj Equivalent | Token Strategy | Expected Savings |
|---------|---------------|----------------|------------------|
| `rtk jj bookmark` | `jj bookmark` | Compact list, "ok ✓" for mutations | 70-80% |
| `rtk jj git push` | `jj git push` | "ok ✓ pushed {bookmark}" | 90%+ |
| `rtk jj git fetch` | `jj git fetch` | "ok fetched (N new)" | 85-90% |
| `rtk jj split` | `jj split` | "ok ✓ split into N commits" | 85%+ |
| `rtk jj edit` | `jj edit` | "ok ✓ now editing {change_id}" | 90%+ |
| `rtk jj resolve` | `jj resolve` | Summary of resolved conflicts | 80-85% |

## Architecture

### Module Structure

```
src/
  jj.rs              # Main jj module (similar to git.rs)
  jj_cmd.rs          # Alternative: standalone command module
```

**Recommendation**: Use `jj.rs` to mirror `git.rs` pattern. jj is a complex VCS with many subcommands that benefit from being in a single cohesive module.

### Command Enum

```rust
#[derive(Debug, Clone)]
pub enum JjCommand {
    // Core workflow
    Status,
    Log,
    Diff,
    Show,
    Describe { message: Option<String> },
    New { revision: Option<String> },
    
    // Commit manipulation
    Squash { interactive: bool },
    Absorb,
    Rebase { source: Option<String>, destination: Option<String> },
    Split,
    Edit { revision: String },
    
    // Bookmarks (branches)
    Bookmark { subcommand: Option<String> },
    
    // Git interop
    Git { subcommand: String },
    
    // Operation log
    OpLog,
    Undo,
    
    // Conflict resolution
    Resolve,
}
```

### Clap Integration (main.rs)

```rust
/// Jujutsu (jj) commands with compact output
Jj {
    #[command(subcommand)]
    command: JjCommands,
},

#[derive(Subcommand)]
enum JjCommands {
    /// Show working copy status
    Status {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Show commit log
    Log {
        /// Revset expression
        #[arg(short, long)]
        revisions: Option<String>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Show diff
    Diff {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Show commit details
    Show {
        /// Revision to show
        revision: Option<String>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Describe (set commit message)
    Describe {
        /// Message
        #[arg(short, long)]
        message: Option<String>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Create new commit
    New {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Squash changes into parent
    Squash {
        /// Interactive mode
        #[arg(short, long)]
        interactive: bool,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Auto-distribute changes to ancestors
    Absorb {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Rebase commits
    Rebase {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Bookmark operations (jj's equivalent of branches)
    Bookmark {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Git operations (push, fetch, clone)
    Git {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Operation log
    #[command(name = "op")]
    Op {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Undo last operation
    Undo {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}
```

## Output Filtering Strategies

### jj status

**Raw output example:**
```
The working copy has no changes.
Working copy  (@) : kntqzsqt d7439b06 (empty) (no description set)
Parent commit (@-): orrkosyo 7fd1a60b master | (empty) Merge pull request #6
```

**RTK filtered output:**
```
@ kntqzsqt d7439b06 (empty)
@- orrkosyo 7fd1a60b master
```

**When working copy has changes:**
```
Working copy changes:
M src/main.rs
A new_file.rs
Working copy  (@) : abc12345 def67890 Add new feature
Parent commit (@-): xyz98765 uvw54321 main | Previous commit
```

**RTK filtered:**
```
@ abc12345 Add new feature
  M src/main.rs
  A new_file.rs
@- xyz98765 main
```

### jj log

**Raw output example:**
```
@  mpqrykyp martinvonz@google.com 2023-02-12 15:00:22 aef4df99
│  (empty) (no description set)
○  kntqzsqt martinvonz@google.com 2023-02-12 14:56:59 5d39e19d
│  Say goodbye
◆  orrkosyo octocat@nowhere.com 2012-03-06 15:06:50 master 7fd1a60b
│  (empty) Merge pull request #6 from Spaceghost/patch-1
~
```

**RTK filtered (default -5 limit, compact format):**
```
@ mpqrykyp aef4df99 (empty)
○ kntqzsqt 5d39e19d Say goodbye
◆ orrkosyo 7fd1a60b master | Merge pull request #6...
```

**Strategy:**
- Limit to 5-10 entries by default
- Strip email addresses (keep author name if needed)
- Strip full timestamps (keep relative like "2d ago" if verbose)
- Truncate long messages at 60 chars
- Preserve graph characters (@ ○ ◆ │ ~)

### jj diff

Reuse existing `compact_diff()` from `git.rs`:
- Show `--stat` summary first
- Compact hunks with truncation
- Max 10 lines per hunk
- Max 100 total lines

### jj show

**Strategy:**
- One-line commit summary (change_id commit_id message)
- Stats summary
- Compact diff (reuse git.rs compact_diff)

### Write Operations (describe, new, squash, etc.)

**Pattern:** Show minimal confirmation with key identifiers

```rust
// jj new
fn run_new(args: &[String], verbose: u8) -> Result<()> {
    let output = Command::new("jj").arg("new").args(args).output()?;
    
    if output.status.success() {
        // Extract change ID from output
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(change_id) = extract_change_id(&stdout) {
            println!("ok ✓ {}", change_id);
        } else {
            println!("ok ✓");
        }
    } else {
        // Show error
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("FAILED: jj new");
        eprintln!("{}", stderr);
        std::process::exit(output.status.code().unwrap_or(1));
    }
    Ok(())
}
```

### jj op log

**Raw output:**
```
@  d3b77addea49 user@host 3 minutes ago, lasted 3 milliseconds
│  squash commits into f7fb5943a6b9460eb106dba2fac5cac1625c6f7a
│  args: jj squash
○  6fc1873c1180 user@host 3 minutes ago, lasted 1 milliseconds
│  snapshot working copy
│  args: jj st
○  ed91f7bcc1fb user@host 6 minutes ago, lasted 1 milliseconds
│  new empty commit
│  args: jj new puqltutt
```

**RTK filtered (limit 5, compact):**
```
@ d3b77ad 3m ago squash → f7fb594
○ 6fc1873 3m ago snapshot
○ ed91f7b 6m ago jj new puqltutt
```

**Strategy:**
- Limit to 5 entries by default
- Shorten op IDs to 7 chars
- Simplify timestamps ("3m ago" vs "3 minutes ago, lasted 3 milliseconds")
- Show operation summary, not full args unless verbose

### jj git push/fetch

**Pattern:** Ultra-compact confirmation

```
# jj git push -c @
ok ✓ pushed push-kntqzsqt → origin

# jj git fetch
ok fetched (3 new bookmarks)
```

### jj bookmark

**List mode:**
```
# Raw
main: orrkosyo 7fd1a60b (empty) Merge pull request #6
feature: abc12345 def67890 Add feature
  @origin: abc12345 def67890

# RTK filtered
main: orrkosyo 7fd1a60b
feature: abc12345 (tracked @origin)
```

**Mutation mode:** "ok ✓"

## Conflict Handling

jj handles conflicts differently - they're stored in commits. RTK should:

1. **Detect conflict state** from `jj status` output
2. **Show conflict count** prominently
3. **List conflicted files** compactly

```
@ abc12345 (conflict) Feature work
  Conflicts: 2 files
    src/main.rs (2-sided)
    src/lib.rs (2-sided)
@- xyz98765 main
```

## Passthrough Strategy

Like git.rs, implement `run_passthrough()` for unsupported subcommands:

```rust
pub fn run_passthrough(args: &[OsString], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();
    
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
```

## Implementation Plan

### Phase 1: Core Commands (Week 1)

1. Create `src/jj.rs` module structure
2. Implement `jj status` with filtering
3. Implement `jj log` with line limits and compaction
4. Implement `jj diff` (reuse compact_diff from git.rs)
5. Implement `jj show`
6. Add passthrough for unsupported commands
7. Add to main.rs Commands enum
8. Write tests with fixtures

### Phase 2: Write Operations (Week 2)

1. Implement `jj describe` with "ok ✓" pattern
2. Implement `jj new` with change ID extraction
3. Implement `jj squash` with summary
4. Implement `jj absorb` with change distribution summary
5. Implement `jj rebase` with commit count
6. Implement `jj op log` with compact format
7. Implement `jj undo`

### Phase 3: Git Interop & Bookmarks (Week 3)

1. Implement `jj git push` with bookmark tracking
2. Implement `jj git fetch` with new ref count
3. Implement `jj bookmark list/set/delete`
4. Implement `jj split` and `jj edit`
5. Add conflict state handling to status/log

### Phase 4: Polish & Documentation

1. Benchmark all commands (<10ms target)
2. Add to README.md
3. Add to CLAUDE.md Module Responsibilities table
4. Update CHANGELOG.md
5. Add smoke tests to test-all.sh

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_jj_log() {
        let raw = r#"@  mpqrykyp user@email.com 2023-02-12 15:00:22 aef4df99
│  (empty) (no description set)
○  kntqzsqt user@email.com 2023-02-12 14:56:59 5d39e19d
│  Say goodbye
"#;
        let result = filter_jj_log(raw, 5);
        assert!(result.contains("@ mpqrykyp aef4df99"));
        assert!(result.contains("○ kntqzsqt 5d39e19d Say goodbye"));
        assert!(!result.contains("user@email.com")); // Email stripped
    }

    #[test]
    fn test_filter_jj_status_clean() {
        let raw = r#"The working copy has no changes.
Working copy  (@) : kntqzsqt d7439b06 (empty) (no description set)
Parent commit (@-): orrkosyo 7fd1a60b master | (empty) Merge pull request #6
"#;
        let result = filter_jj_status(raw);
        assert!(result.contains("@ kntqzsqt d7439b06 (empty)"));
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
        assert!(result.contains("squash"));
    }
}
```

### Fixtures

Create test fixtures:
- `tests/fixtures/jj_log_raw.txt`
- `tests/fixtures/jj_status_raw.txt`
- `tests/fixtures/jj_diff_raw.txt`
- `tests/fixtures/jj_op_log_raw.txt`

### Smoke Tests

Add to `scripts/test-all.sh`:
```bash
# jj commands (only if jj is installed)
if command -v jj &> /dev/null; then
    echo "Testing jj commands..."
    rtk jj status 2>/dev/null || echo "jj status: not in jj repo (OK)"
    rtk jj log -r @ 2>/dev/null || echo "jj log: not in jj repo (OK)"
fi
```

## Performance Requirements

| Metric | Target |
|--------|--------|
| Startup time | <10ms |
| jj status | <15ms total |
| jj log -5 | <20ms total |
| Memory | <5MB resident |

## Risks & Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| jj output format changes | Medium | High | Use flexible regex, test with multiple jj versions |
| Low user demand | Low | Medium | jj is growing; demand will increase |
| Complex revset handling | Medium | Low | Pass through to jj, don't parse revsets |
| Interactive commands (split, diffedit) | High | Medium | Always passthrough interactive commands |

## Success Metrics

1. **Token savings**: 60-90% on common jj commands
2. **Performance**: <10ms startup overhead
3. **Coverage**: Support top 15 most-used jj commands
4. **Reliability**: No filter failures on valid jj output

## Open Questions

1. **Revset support**: Should RTK understand revsets or always pass through?
   - **Recommendation**: Pass through. Revsets are complex; let jj handle them.

2. **Template customization**: jj supports custom templates. Handle?
   - **Recommendation**: Detect custom templates and pass through.

3. **Colocated repos**: jj can colocate with git. Detect and use jj or git?
   - **Recommendation**: User explicitly chooses `rtk jj` vs `rtk git`.

4. **Operation IDs in undo**: Show full or truncated?
   - **Recommendation**: Truncated (7 chars) by default, full with `-v`.

## References

- [Jujutsu GitHub Repository](https://github.com/jj-vcs/jj)
- [Jujutsu Documentation](https://docs.jj-vcs.dev/latest/)
- [Git Comparison](https://docs.jj-vcs.dev/latest/git-comparison/)
- [Working with GitHub](https://docs.jj-vcs.dev/latest/github/)
- RTK git.rs implementation (existing pattern)
- RTK gh_cmd.rs implementation (JSON filtering pattern)

## Appendix: Common jj Workflows

### Basic Development Workflow
```bash
jj new                  # Create new commit for work
# ... edit files ...
jj describe -m "feat: add feature"
jj git push -c @        # Push with auto-generated bookmark
```

### Stacked PRs Workflow
```bash
jj new main -m "Feature A"
# ... work on A ...
jj new -m "Feature B"   # B depends on A
# ... work on B ...
jj git push -c @-       # Push A
jj git push -c @        # Push B
```

### Fixing Earlier Commits
```bash
jj edit <change_id>     # Switch to earlier commit
# ... make fixes ...
jj new                  # Return to working on top
# Descendants auto-rebased!
```

### Absorb Changes
```bash
# After reviewing, realize fixes belong in ancestor commits
jj absorb               # Auto-distribute changes to ancestors
jj git push --all       # Push all updated bookmarks
```

These workflows generate verbose output that RTK can compress significantly.
