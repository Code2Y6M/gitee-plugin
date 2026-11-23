//! Gitee (码云) OpenAPI v5 client, built on `host::fetch`.
//!
//! Authentication supports two modes, selected by what the user configured:
//! - **Personal access token**: `Authorization: token <token>` (recommended).
//! - **OAuth2 bearer token**: `Authorization: Bearer <access_token>`.
//!
//! The token lives in the IDE's secret store, never in plugin storage or logs;
//! it is injected into the header by the IDE via the `{{secret:name}}`
//! placeholder, so it never passes through plugin code.

use openidea_plugin::{host, Error, HttpRequest};
use serde::Deserialize;

/// Gitee OpenAPI v5 base URL.
pub const API_BASE: &str = "https://gitee.com/api/v5";

/// Which auth mode to use for a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthMode {
    /// `Authorization: token <token>` — personal access token.
    Token,
    /// `Authorization: Bearer <access_token>` — OAuth2.
    OAuth,
}

/// A parsed Gitee repository reference: `owner/repo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoRef {
    pub owner: String,
    pub repo: String,
}

impl RepoRef {
    pub fn new(owner: impl Into<String>, repo: impl Into<String>) -> Self {
        Self { owner: owner.into(), repo: repo.into() }
    }

    /// The `owner/repo` path segment used in API URLs.
    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }
}

impl std::fmt::Display for RepoRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.owner, self.repo)
    }
}

/// Parse a Gitee remote URL or `owner/repo` string into a [`RepoRef`].
///
/// Accepted forms:
/// - `https://gitee.com/owner/repo.git`
/// - `http://gitee.com/owner/repo`
/// - `git@gitee.com:owner/repo.git`
/// - `owner/repo`
pub fn parse_remote(s: &str) -> Option<RepoRef> {
    let s = s.trim();
    // ssh form: git@gitee.com:owner/repo.git
    if let Some(rest) = s.strip_prefix("git@gitee.com:") {
        return split_path(rest);
    }
    // https/http form
    for scheme in ["https://", "http://"] {
        if let Some(rest) = s.strip_prefix(scheme) {
            let rest = rest.strip_prefix("gitee.com/")?;
            return split_path(rest);
        }
    }
    // plain owner/repo
    if !s.contains("://") && !s.contains('@') {
        return split_path(s);
    }
    None
}

fn split_path(rest: &str) -> Option<RepoRef> {
    let path = rest.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut parts = path.split('/');
    let owner = parts.next()?.trim();
    let repo = parts.next()?.trim();
    if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
        return None;
    }
    // Reject any character outside Gitee's legal owner/repo names, so a remote
    // can never inject path traversal or shell metacharacters into a URL or a
    // `git clone` argument.
    if !valid_name(owner) || !valid_name(repo) {
        return None;
    }
    Some(RepoRef::new(owner, repo))
}

/// Gitee owner/repo names allow `[a-zA-Z0-9._-]` (plus CJK for owners, which
/// we conservatively reject here to keep URLs safe).
fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Build the Authorization header value for the given mode.
///
/// The `{{secret:name}}` placeholder is substituted by the IDE (requires the
/// `secrets` capability); the raw token never reaches plugin code or logs.
fn auth_header(auth: AuthMode) -> String {
    match auth {
        AuthMode::Token => "token {{secret:gitee-token}}".to_owned(),
        AuthMode::OAuth => "Bearer {{secret:gitee-oauth-token}}".to_owned(),
    }
}

/// Perform an HTTP call and return status + body text.
fn request(
    method: &str,
    url: &str,
    auth: AuthMode,
    body: Option<String>,
) -> Result<(u16, String), Error> {
    let mut req = HttpRequest {
        method: method.to_owned(),
        url: url.to_owned(),
        body: body.map(String::into_bytes),
        ..Default::default()
    };
    req = req.header("Authorization", &auth_header(auth));
    if req.body.is_some() {
        req = req.header("Content-Type", "application/json");
    }

    let resp = host::fetch(&req)?;
    Ok((resp.status, resp.text()))
}

fn ok_status(status: u16) -> bool {
    (200..300).contains(&status)
}

fn api_error(status: u16, body: &str) -> Error {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(msg) = v.get("message").and_then(|m| m.as_str()) {
            return Error::Failed(format!("Gitee API {status}: {msg}"));
        }
    }
    Error::Failed(format!("Gitee API {status}: {}", truncate(body, 200)))
}

fn truncate(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

fn get_json<T: for<'de> Deserialize<'de>>(url: &str, auth: AuthMode) -> Result<T, Error> {
    let (status, body) = request("GET", url, auth, None)?;
    if !ok_status(status) {
        return Err(api_error(status, &body));
    }
    serde_json::from_str(&body).map_err(|e| Error::Failed(format!("bad API response: {e}")))
}

/// A Gitee pull request (subset of the API's fields).
#[derive(Clone, Debug, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub html_url: String,
    #[serde(default)]
    pub user: Option<User>,
    #[serde(default)]
    pub head: Option<BranchRef>,
    #[serde(default)]
    pub base: Option<BranchRef>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct User {
    pub login: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BranchRef {
    pub label: String,
}

/// A Gitee issue (subset).
#[derive(Clone, Debug, Deserialize)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub html_url: String,
    #[serde(default)]
    pub user: Option<User>,
}

/// The repository info returned when creating / fetching a repo.
#[derive(Clone, Debug, Deserialize)]
pub struct Repository {
    pub full_name: String,
    pub html_url: String,
    pub ssh_url: String,
    #[serde(default)]
    pub clone_url: String,
}

/// List the pull requests of a repository.
pub fn list_prs(repo: &RepoRef, auth: AuthMode) -> Result<Vec<PullRequest>, Error> {
    let url = format!("{API_BASE}/repos/{}/pulls?state=open&per_page=50", repo.slug());
    get_json(&url, auth)
}

/// List the issues of a repository.
pub fn list_issues(repo: &RepoRef, auth: AuthMode) -> Result<Vec<Issue>, Error> {
    let url = format!("{API_BASE}/repos/{}/issues?state=open&per_page=50", repo.slug());
    get_json(&url, auth)
}

/// Create a repository under the authenticated user's account.
pub fn create_repository(
    name: &str,
    description: &str,
    private: bool,
    auth: AuthMode,
) -> Result<Repository, Error> {
    let body = serde_json::json!({
        "name": name,
        "description": description,
        "private": private,
        "auto_init": true,
    })
    .to_string();
    let (status, body_text) =
        request("POST", &format!("{API_BASE}/user/repos"), auth, Some(body))?;
    if !ok_status(status) {
        return Err(api_error(status, &body_text));
    }
    serde_json::from_str(&body_text)
        .map_err(|e| Error::Failed(format!("bad API response: {e}")))
}

/// Resolve a Gitee remote URL via the API (verifies access).
pub fn get_repository(remote: &str, auth: AuthMode) -> Result<Repository, Error> {
    let repo = parse_remote(remote)
        .ok_or_else(|| Error::Invalid(format!("not a Gitee remote: {remote}")))?;
    get_json(&format!("{API_BASE}/repos/{}", repo.slug()), auth)
}

/// List the repositories the authenticated user owns or can access.
///
/// Uses the `/user/repos` endpoint (returns the user's own repos, including
/// private ones when the token has the right scope), sorted by most recently
/// pushed first. Fall back to `/repos`-style listing is not needed here.
pub fn list_repositories(auth: AuthMode) -> Result<Vec<Repository>, Error> {
    let url = format!("{API_BASE}/user/repos?per_page=100&sort=pushed");
    get_json(&url, auth)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_remote_forms() {
        assert_eq!(
            parse_remote("https://gitee.com/foo/bar.git"),
            Some(RepoRef::new("foo", "bar"))
        );
        assert_eq!(
            parse_remote("http://gitee.com/foo/bar"),
            Some(RepoRef::new("foo", "bar"))
        );
        assert_eq!(
            parse_remote("git@gitee.com:foo/bar.git"),
            Some(RepoRef::new("foo", "bar"))
        );
        assert_eq!(parse_remote("foo/bar"), Some(RepoRef::new("foo", "bar")));
        assert_eq!(parse_remote("github.com/foo/bar"), None);
        assert_eq!(parse_remote("foo/bar/baz"), None);
        assert_eq!(parse_remote(""), None);
    }

    #[test]
    fn rejects_injection_characters() {
        // Path traversal and shell metacharacters must be rejected.
        assert_eq!(parse_remote("foo/../bar"), None);
        assert_eq!(parse_remote("foo/bar; rm -rf /"), None);
        assert_eq!(parse_remote("foo/bar space"), None);
        assert_eq!(parse_remote("foo/bar@evil.com"), None);
        assert_eq!(parse_remote("foo/bar//etc/passwd"), None);
        // Legal characters still parse.
        assert_eq!(
            parse_remote("foo-bar/baz_qux.1"),
            Some(RepoRef::new("foo-bar", "baz_qux.1"))
        );
    }

    #[test]
    fn builds_auth_headers() {
        assert_eq!(auth_header(AuthMode::Token), "token {{secret:gitee-token}}");
        assert_eq!(auth_header(AuthMode::OAuth), "Bearer {{secret:gitee-oauth-token}}");
    }

    #[test]
    fn parses_pr_json() {
        let p: PullRequest = serde_json::from_str(
            r#"{"number":42,"title":"t","state":"open","html_url":"https://gitee.com/f/b/pulls/42","user":{"login":"a"},"head":{"label":"a:dev"},"base":{"label":"a:main"}}"#,
        )
        .unwrap();
        assert_eq!(p.number, 42);
        assert_eq!(p.head.unwrap().label, "a:dev");
    }

    #[test]
    fn truncates_long_errors() {
        assert_eq!(truncate("hello", 3), "hel");
        assert_eq!(truncate("hi", 10), "hi");
    }
}
