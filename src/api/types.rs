//! The Gitee objects the plugin reads and writes.
//!
//! Gitee's JSON is looser than its docs: counts arrive as strings or numbers,
//! issue numbers may be non-numeric (`"ICK440"`), labels as strings or objects,
//! and a changed file's patch as `{"diff": "…"}`. Everything here parses
//! leniently so one odd field cannot fail a whole page.

use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// The user who opened a PR or issue, or wrote a comment.
#[derive(Clone, Debug, Deserialize)]
pub struct User {
    pub login: String,
    #[serde(default)]
    pub name: Option<String>,
}

impl User {
    /// The name to show: the display name when there is one.
    pub fn label(&self) -> &str {
        self.name.as_deref().filter(|n| !n.is_empty()).unwrap_or(&self.login)
    }
}

/// A label, which Gitee sends as a string or as an object with `name`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label(pub String);

impl<'de> Deserialize<'de> for Label {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        Ok(Label(match v {
            Value::String(s) => s,
            Value::Object(map) => map.get("name").and_then(|n| n.as_str()).unwrap_or_default().to_owned(),
            _ => String::new(),
        }))
    }
}

/// Parse a number that Gitee may send as a number or a string.
fn de_u32_lenient<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u32>, D::Error> {
    let v = Option::<Value>::deserialize(d)?;
    Ok(v.and_then(|v| match v {
        Value::Number(n) => n.as_u64().map(|n| n as u32),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }))
}

/// A changed file's patch: `{"diff": "…"}` (or, in older answers, the diff itself).
fn de_patch<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let v = Option::<Value>::deserialize(d)?;
    Ok(v.and_then(|v| match v {
        Value::String(s) => Some(s),
        Value::Object(map) => map.get("diff").and_then(|d| d.as_str()).map(str::to_owned),
        _ => None,
    }))
}

/// A branch reference of a pull request.
#[derive(Clone, Debug, Deserialize)]
pub struct BranchRef {
    #[serde(default)]
    pub label: String,
    #[serde(rename = "ref", default)]
    pub git_ref: Option<String>,
}

impl BranchRef {
    /// The branch name: `ref` when present, else the label.
    pub fn name(&self) -> &str {
        self.git_ref.as_deref().filter(|s| !s.is_empty()).unwrap_or(&self.label)
    }
}

/// A Gitee repository (subset).
#[derive(Clone, Debug, Deserialize)]
pub struct Repository {
    pub full_name: String,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub ssh_url: String,
    #[serde(default)]
    pub clone_url: String,
    #[serde(default)]
    pub default_branch: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "private", default)]
    pub is_private: bool,
}

impl Repository {
    /// The branch a pull request targets by default.
    pub fn base_branch(&self) -> String {
        self.default_branch.clone().unwrap_or_else(|| "master".to_owned())
    }
}

/// A branch of a repository.
#[derive(Clone, Debug, Deserialize)]
pub struct Branch {
    pub name: String,
}

/// A Gitee pull request.
#[derive(Clone, Debug, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub user: Option<User>,
    #[serde(default)]
    pub head: Option<BranchRef>,
    #[serde(default)]
    pub base: Option<BranchRef>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub merged_at: Option<String>,
    #[serde(default)]
    pub mergeable: Option<bool>,
    #[serde(default)]
    pub draft: Option<bool>,
    #[serde(default)]
    pub labels: Vec<Label>,
}

impl PullRequest {
    /// Whether the pull request is open (Gitee's `state` is `open`/`closed`/`merged`).
    pub fn is_open(&self) -> bool {
        self.state == "open" && self.merged_at.is_none()
    }

    /// `merged`, `closed` or `open`, for a status column.
    pub fn status(&self) -> &'static str {
        if self.merged_at.is_some() || self.state == "merged" {
            "merged"
        } else if self.state == "closed" {
            "closed"
        } else {
            "open"
        }
    }
}

/// A file a pull request changes (`GET /pulls/{n}/files`).
#[derive(Clone, Debug, Deserialize)]
pub struct ChangedFile {
    pub filename: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default, deserialize_with = "de_u32_lenient")]
    pub additions: Option<u32>,
    #[serde(default, deserialize_with = "de_u32_lenient")]
    pub deletions: Option<u32>,
    #[serde(default, deserialize_with = "de_patch")]
    pub patch: Option<String>,
}

impl ChangedFile {
    /// `+3 −1`, the way a diff shows it.
    pub fn stats(&self) -> String {
        match (self.additions, self.deletions) {
            (Some(a), Some(d)) => format!("+{a} −{d}"),
            _ => String::new(),
        }
    }
}

/// One commit of a pull request.
#[derive(Clone, Debug, Deserialize)]
pub struct PrCommit {
    pub sha: String,
    #[serde(default)]
    pub commit: Option<CommitDetail>,
    #[serde(default)]
    pub author: Option<User>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CommitDetail {
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub author: Option<CommitPerson>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CommitPerson {
    #[serde(default)]
    pub name: String,
}

impl PrCommit {
    /// The commit's first line.
    pub fn summary(&self) -> String {
        self.commit
            .as_ref()
            .map(|c| c.message.lines().next().unwrap_or_default().to_owned())
            .unwrap_or_default()
    }

    pub fn author_name(&self) -> String {
        self.commit
            .as_ref()
            .and_then(|c| c.author.as_ref())
            .map(|a| a.name.clone())
            .or_else(|| self.author.as_ref().map(|u| u.login.clone()))
            .unwrap_or_default()
    }
}

/// A comment on a pull request or issue. Gitee sends the body as HTML.
#[derive(Clone, Debug, Deserialize)]
pub struct Comment {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub user: Option<User>,
    #[serde(default)]
    pub created_at: Option<String>,
}

/// A Gitee issue.
#[derive(Clone, Debug, Deserialize)]
pub struct Issue {
    /// Project-scoped, possibly non-numeric (`ICK440`).
    pub number: String,
    pub title: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub user: Option<User>,
    #[serde(default)]
    pub labels: Vec<Label>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub comments: Option<u32>,
}

/// A repository label (`GET /repos/{owner}/{repo}/labels`).
#[derive(Clone, Debug, Deserialize)]
pub struct RepoLabel {
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
}

/// One entry of Gitee's notification inbox.
///
/// The endpoint's shape is under-documented, so the fields are extracted by
/// name from the raw JSON (`parse_thread`) instead of deserializing strictly.
#[derive(Clone, Debug, Default)]
pub struct Notification {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub repo: String,
    pub url: Option<String>,
    pub unread: bool,
    pub updated_at: Option<String>,
}

/// How many unread notifications there are (`GET /notifications/count`).
#[derive(Clone, Copy, Debug, Default)]
pub struct NotificationCount {
    pub unread: u32,
    pub total: u32,
}
