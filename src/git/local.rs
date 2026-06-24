//! Local git data for the git tab — shells out to `git` and parses the output.
//! No new dependency (same spirit as `module/discovery.rs`). Every function
//! returns owned data or a short error string; the caller renders it.

use std::path::Path;
use std::process::Command;

use super::model::{BranchInfo, Commit, FileChange, RepoStatus};

/// Run `git <args>` in `cwd`, returning stdout (trimmed of a trailing newline).
fn run(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("git not found: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Whether `cwd` is inside a git work tree.
pub fn is_repo(cwd: &Path) -> bool {
    run(cwd, &["rev-parse", "--is-inside-work-tree"])
        .map(|s| s.trim() == "true")
        .unwrap_or(false)
}

/// Branch + ahead/behind + working-tree changes + stashes.
pub fn status(cwd: &Path) -> Result<RepoStatus, String> {
    let raw = run(cwd, &["status", "--porcelain=v1", "--branch"])?;
    let mut st = RepoStatus::default();
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            parse_branch_line(rest, &mut st);
        } else if let Some(path) = line.strip_prefix("?? ") {
            st.untracked.push(path.to_string());
        } else if line.len() > 3 {
            let bytes = line.as_bytes();
            let (x, y) = (bytes[0] as char, bytes[1] as char);
            let path = line[3..].to_string();
            if x != ' ' && x != '?' {
                st.staged.push(FileChange {
                    code: x,
                    path: path.clone(),
                });
            }
            if y != ' ' && y != '?' {
                st.unstaged.push(FileChange { code: y, path });
            }
        }
    }
    st.stashes = run(cwd, &["stash", "list"])
        .map(|s| s.lines().map(str::to_string).collect())
        .unwrap_or_default();
    Ok(st)
}

/// Parse a porcelain `## ` branch header into `st`.
fn parse_branch_line(rest: &str, st: &mut RepoStatus) {
    // `main...origin/main [ahead 2, behind 1]`  |  `main`  |  `HEAD (no branch)`
    let (head, track) = match rest.split_once(" [") {
        Some((h, t)) => (h, Some(t.trim_end_matches(']'))),
        None => (rest, None),
    };
    let (branch, upstream) = match head.split_once("...") {
        Some((b, u)) => (b, Some(u.to_string())),
        None => (head, None),
    };
    st.branch = branch.trim().to_string();
    st.upstream = upstream;
    if let Some(t) = track {
        for part in t.split(',') {
            let part = part.trim();
            if let Some(n) = part.strip_prefix("ahead ") {
                st.ahead = n.trim().parse().unwrap_or(0);
            } else if let Some(n) = part.strip_prefix("behind ") {
                st.behind = n.trim().parse().unwrap_or(0);
            }
        }
    }
}

const FIELD: &str = "\u{1f}"; // unit separator — safe field delimiter

/// Local branches with upstream tracking and last-commit info.
pub fn branches(cwd: &Path) -> Result<Vec<BranchInfo>, String> {
    let fmt = format!(
        "%(HEAD){F}%(refname:short){F}%(upstream:track){F}%(contents:subject){F}%(authorname){F}%(committerdate:relative)",
        F = FIELD
    );
    let raw = run(
        cwd,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            &format!("--format={fmt}"),
            "refs/heads",
        ],
    )?;
    Ok(raw
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(FIELD).collect();
            if f.len() < 6 {
                return None;
            }
            let (ahead, behind) = parse_track(f[2]);
            Some(BranchInfo {
                is_head: f[0] == "*",
                name: f[1].to_string(),
                ahead,
                behind,
                subject: f[3].to_string(),
                author: f[4].to_string(),
                when: f[5].to_string(),
            })
        })
        .collect())
}

/// Parse a `%(upstream:track)` value like `[ahead 2, behind 1]`.
fn parse_track(s: &str) -> (u32, u32) {
    let inner = s.trim_start_matches('[').trim_end_matches(']');
    let (mut a, mut b) = (0, 0);
    for part in inner.split(',') {
        let part = part.trim();
        if let Some(n) = part.strip_prefix("ahead ") {
            a = n.trim().parse().unwrap_or(0);
        } else if let Some(n) = part.strip_prefix("behind ") {
            b = n.trim().parse().unwrap_or(0);
        }
    }
    (a, b)
}

/// Recent commits (the flow view). `all` includes every ref's history.
pub fn commits(cwd: &Path, n: usize, all: bool) -> Result<Vec<Commit>, String> {
    let fmt = format!("%h{F}%s{F}%an{F}%ar{F}%d", F = FIELD);
    let count = format!("-n{n}");
    let pretty = format!("--pretty=format:{fmt}");
    let mut args: Vec<&str> = vec!["log", "--graph", &count, &pretty];
    if all {
        args.push("--all");
    }
    let raw = run(cwd, &args)?;
    Ok(raw
        .lines()
        .filter_map(|line| {
            // `--graph` prefixes each line with rail glyphs before the format.
            match line.split_once(FIELD) {
                Some((head, rest)) => {
                    // head = "<graph><short-sha>"; split the sha off the graph.
                    let trimmed = head.trim_end();
                    let sha_start = trimmed.rfind(' ').map(|i| i + 1).unwrap_or(0);
                    let graph = head[..sha_start].to_string();
                    let sha = trimmed[sha_start..].to_string();
                    let f: Vec<&str> = rest.split(FIELD).collect();
                    Some(Commit {
                        sha,
                        graph,
                        subject: f.first().copied().unwrap_or("").to_string(),
                        author: f.get(1).copied().unwrap_or("").to_string(),
                        when: f.get(2).copied().unwrap_or("").to_string(),
                        refs: f.get(3).copied().unwrap_or("").trim().to_string(),
                    })
                }
                // Graph-only connector lines (e.g. `|/`) carry no commit.
                None => None,
            }
        })
        .collect())
}

/// Checkout a branch (mutating). Used by the Branches view's `enter`.
pub fn checkout(cwd: &Path, branch: &str) -> Result<(), String> {
    run(cwd, &["switch", branch]).map(|_| ())
}
