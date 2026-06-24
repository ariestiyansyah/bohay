//! GitHub data for the git tab via the **`gh` CLI** (docs/17, GIT-2). No HTTP/
//! auth dependency — we shell out and parse JSON, and degrade gracefully when
//! `gh` is missing or unauthenticated.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

use super::model::{Checks, Issue, PullRequest};
use super::Scope;

/// Availability of the `gh` CLI for this session.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GhState {
    Ready,
    NotAuthed,
    Missing,
}

impl GhState {
    pub fn note(self) -> Option<&'static str> {
        match self {
            GhState::Ready => None,
            GhState::NotAuthed => Some("run `gh auth login` for PRs & issues"),
            GhState::Missing => Some("install GitHub CLI (`gh`) for PRs & issues"),
        }
    }
}

/// Probe `gh` once (installed? authenticated?).
pub fn detect() -> GhState {
    match Command::new("gh").arg("--version").output() {
        Ok(o) if o.status.success() => match Command::new("gh").args(["auth", "status"]).output() {
            Ok(a) if a.status.success() => GhState::Ready,
            Ok(_) => GhState::NotAuthed,
            Err(_) => GhState::Missing,
        },
        _ => GhState::Missing,
    }
}

fn run_gh(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("gh")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("gh: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err.lines().next().unwrap_or("gh failed").trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

const PR_FIELDS: &str = "number,title,author,isDraft,state,reviewDecision,reviewRequests,headRefName,additions,deletions,statusCheckRollup";
// `gh search` exposes fewer fields than `gh pr list` (no checks/+-/review).
const SEARCH_PR_FIELDS: &str = "number,title,author,isDraft,state,repository";
const SEARCH_ISSUE_FIELDS: &str = "number,title,author,labels,repository";

/// Open pull requests for `scope` (this repo, or everything you're involved in).
pub fn pull_requests(cwd: &Path, scope: Scope) -> Result<Vec<PullRequest>, String> {
    let raw = match scope {
        Scope::ThisRepo => run_gh(cwd, &["pr", "list", "--json", PR_FIELDS, "--limit", "50"])?,
        Scope::MyWork => run_gh(
            cwd,
            &[
                "search",
                "prs",
                "--involves=@me",
                "--state=open",
                "--json",
                SEARCH_PR_FIELDS,
                "--limit",
                "50",
            ],
        )?,
    };
    let v: Value = serde_json::from_str(&raw).map_err(|e| format!("parse gh: {e}"))?;
    Ok(v.as_array()
        .map(|a| a.iter().map(parse_pr).collect())
        .unwrap_or_default())
}

const ISSUE_FIELDS: &str = "number,title,author,labels,assignees";

/// Open issues for `scope`.
pub fn issues(cwd: &Path, scope: Scope) -> Result<Vec<Issue>, String> {
    let raw = match scope {
        Scope::ThisRepo => run_gh(
            cwd,
            &["issue", "list", "--json", ISSUE_FIELDS, "--limit", "50"],
        )?,
        Scope::MyWork => run_gh(
            cwd,
            &[
                "search",
                "issues",
                "--involves=@me",
                "--state=open",
                "--json",
                SEARCH_ISSUE_FIELDS,
                "--limit",
                "50",
            ],
        )?,
    };
    let v: Value = serde_json::from_str(&raw).map_err(|e| format!("parse gh: {e}"))?;
    Ok(v.as_array()
        .map(|a| a.iter().map(parse_issue).collect())
        .unwrap_or_default())
}

/// `{ "nameWithOwner": "o/r" }` (search results) → "o/r".
fn repo_of(v: &Value) -> String {
    v.get("repository")
        .and_then(|r| r.get("nameWithOwner").or_else(|| r.get("name")))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn parse_pr(v: &Value) -> PullRequest {
    PullRequest {
        number: v.get("number").and_then(Value::as_u64).unwrap_or(0),
        title: str_at(v, "title"),
        author: login(v.get("author")),
        state: str_at(v, "state"),
        is_draft: v.get("isDraft").and_then(Value::as_bool).unwrap_or(false),
        review_decision: str_at(v, "reviewDecision"),
        reviewers: logins(v.get("reviewRequests")),
        head: str_at(v, "headRefName"),
        additions: v.get("additions").and_then(Value::as_u64).unwrap_or(0),
        deletions: v.get("deletions").and_then(Value::as_u64).unwrap_or(0),
        checks: rollup(v.get("statusCheckRollup")),
        repo: repo_of(v),
    }
}

fn parse_issue(v: &Value) -> Issue {
    Issue {
        number: v.get("number").and_then(Value::as_u64).unwrap_or(0),
        title: str_at(v, "title"),
        author: login(v.get("author")),
        labels: names(v.get("labels"), "name"),
        assignees: logins(v.get("assignees")),
        repo: repo_of(v),
    }
}

fn str_at(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

/// `{ "login": "x" }` → "x".
fn login(v: Option<&Value>) -> String {
    v.and_then(|o| o.get("login"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// `[ { "login": "a" }, … ]` → ["a", …] (users) / `name` (teams).
fn logins(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|o| {
                    o.get("login")
                        .or_else(|| o.get("name"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn names(v: Option<&Value>, key: &str) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|o| o.get(key).and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Collapse `statusCheckRollup` into one state. A failure wins, then pending.
fn rollup(v: Option<&Value>) -> Checks {
    let Some(arr) = v.and_then(Value::as_array) else {
        return Checks::None;
    };
    if arr.is_empty() {
        return Checks::None;
    }
    let mut pending = false;
    for c in arr {
        // status contexts use `state`; check runs use `status` + `conclusion`.
        let state = c.get("state").and_then(Value::as_str).unwrap_or("");
        let status = c.get("status").and_then(Value::as_str).unwrap_or("");
        let concl = c.get("conclusion").and_then(Value::as_str).unwrap_or("");
        let s = if !state.is_empty() {
            state
        } else if !concl.is_empty() {
            concl
        } else {
            status
        };
        match s {
            "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED"
            | "STARTUP_FAILURE" => return Checks::Failing,
            "PENDING" | "IN_PROGRESS" | "QUEUED" | "EXPECTED" | "REQUESTED" | "WAITING" => {
                pending = true
            }
            _ => {}
        }
    }
    if pending {
        Checks::Pending
    } else {
        Checks::Passing
    }
}

/// Open a PR (or issue) in the browser.
pub fn view_web(cwd: &Path, kind: &str, number: u64) -> Result<(), String> {
    run_gh(cwd, &[kind, "view", &number.to_string(), "--web"]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_pr() {
        let v: Value = serde_json::from_str(
            r#"{"number":78,"title":"Add dark mode","author":{"login":"sam"},
                "isDraft":false,"state":"OPEN","reviewDecision":"REVIEW_REQUIRED",
                "reviewRequests":[{"login":"taylor"}],"headRefName":"feature/dark-mode",
                "additions":567,"deletions":234,"url":"http://x",
                "statusCheckRollup":[{"state":"SUCCESS"}]}"#,
        )
        .unwrap();
        let pr = parse_pr(&v);
        assert_eq!(pr.number, 78);
        assert_eq!(pr.author, "sam");
        assert_eq!(pr.reviewers, vec!["taylor"]);
        assert_eq!(pr.additions, 567);
        assert!(matches!(pr.checks, Checks::Passing));
    }

    #[test]
    fn rollup_failure_wins() {
        let v: Value =
            serde_json::from_str(r#"[{"conclusion":"SUCCESS"},{"state":"FAILURE"}]"#).unwrap();
        assert!(matches!(rollup(Some(&v)), Checks::Failing));
        let v: Value = serde_json::from_str(r#"[{"status":"IN_PROGRESS"}]"#).unwrap();
        assert!(matches!(rollup(Some(&v)), Checks::Pending));
        let v: Value = serde_json::from_str("[]").unwrap();
        assert!(matches!(rollup(Some(&v)), Checks::None));
    }
}
