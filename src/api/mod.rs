//! Gitee (码云) OpenAPI v5 client, built on `host::fetch`.
//!
//! Authentication supports two modes, selected by what the user configured:
//! - **Personal access token**: `Authorization: token <token>` (recommended).
//! - **OAuth2 bearer token**: `Authorization: Bearer <access_token>`.
//!
//! The token lives in the IDE's secret store, never in plugin storage or logs;
//! it is injected into the header by the IDE via the `{{secret:name}}`
//! placeholder, so it never passes through plugin code.

mod issues;
mod notify;
mod pulls;
mod repos;
mod types;

pub use issues::*;
pub use notify::*;
pub use pulls::*;
pub use repos::*;
pub use types::*;

use openidea_plugin::{Error, HttpRequest, host};
use serde::de::DeserializeOwned;
use std::fmt;

use crate::i18n::t;

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

/// A Gitee repository reference: `owner/repo`.
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

impl fmt::Display for RepoRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

/// Why a request failed, as far as the UI needs to tell them apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApiError {
    /// 401: no token, or the token is invalid/expired.
    Unauthorized,
    /// 403: the token lacks the scope, or the repository forbids it.
    Forbidden(String),
    /// 404: the repository, pull request or issue does not exist (or is private).
    NotFound(String),
    /// 409: the request conflicts with the current state (already merged, …).
    Conflict(String),
    /// 429: Gitee's rate limit.
    RateLimited,
    /// 5xx.
    Server(u16, String),
    /// The request never reached Gitee.
    Network(String),
    /// Gitee answered with something that doesn't match the API.
    BadResponse(String),
    /// A value the plugin rejected before sending anything.
    Invalid(String),
}

impl ApiError {
    /// Whether the fix is to configure a token.
    pub fn needs_token(&self) -> bool {
        matches!(self, Self::Unauthorized)
    }

    /// A short, actionable sentence for a notification, on top of the message.
    pub fn hint(&self) -> Option<String> {
        match self {
            Self::Unauthorized => Some(t(
                "在「设置 › 插件 › Gitee › Secrets」填入私人访问令牌。",
                "Add a personal access token in Settings › Plugins › Gitee › Secrets.",
            )),
            Self::Forbidden(_) => Some(t(
                "检查令牌的权限范围（projects / pull_requests / issues）。",
                "Check the token's scopes (projects / pull_requests / issues).",
            )),
            Self::RateLimited => Some(t(
                "Gitee 接口限流，请稍后再试。",
                "Gitee is rate limiting the API; try again shortly.",
            )),
            _ => None,
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized => f.write_str(&t("未授权：令牌缺失或已失效", "unauthorized: no token, or it expired")),
            Self::Forbidden(m) => write!(f, "{}: {m}", t("无权限", "forbidden")),
            Self::NotFound(m) => write!(f, "{}: {m}", t("不存在", "not found")),
            Self::Conflict(m) => write!(f, "{}: {m}", t("状态冲突", "conflict")),
            Self::RateLimited => f.write_str(&t("触发 Gitee 限流", "Gitee rate limit reached")),
            Self::Server(status, m) => write!(f, "{} {status}: {m}", t("Gitee 服务错误", "Gitee server error")),
            Self::Network(m) => write!(f, "{}: {m}", t("网络错误", "network error")),
            Self::BadResponse(m) => write!(f, "{}: {m}", t("响应无法解析", "unexpected response")),
            Self::Invalid(m) => write!(f, "{}: {m}", t("参数错误", "invalid")),
        }
    }
}

impl From<ApiError> for Error {
    /// Keeps the actionable hint with the message, so a failing command tells
    /// the user what to do instead of only what broke.
    fn from(e: ApiError) -> Self {
        let mut text = e.to_string();
        if let Some(hint) = e.hint() {
            text.push('\n');
            text.push_str(&hint);
        }
        match e {
            ApiError::Invalid(m) => Error::Invalid(m),
            ApiError::NotFound(m) => Error::NotFound(m),
            _ => Error::Failed(text),
        }
    }
}

/// Which slice of a list to ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Page {
    pub page: u32,
    pub per_page: u32,
}

impl Page {
    pub const MAX_PER_PAGE: u32 = 100;

    pub fn first(per_page: u32) -> Self {
        Self { page: 1, per_page: per_page.clamp(1, Self::MAX_PER_PAGE) }
    }

    fn query(&self) -> String {
        format!("page={}&per_page={}", self.page.max(1), self.per_page.clamp(1, Self::MAX_PER_PAGE))
    }
}

impl Default for Page {
    fn default() -> Self {
        Self::first(50)
    }
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

/// Percent-encode a query value (`owner/repo`, a branch with `/`, a search).
pub(crate) fn enc(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            b' ' => out.push_str("%20"),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// Perform an HTTP call and return status + body text.
fn request(
    method: &str,
    url: &str,
    auth: AuthMode,
    content_type: Option<&str>,
    body: Option<Vec<u8>>,
) -> Result<(u16, String), ApiError> {
    let mut req = HttpRequest {
        method: method.to_owned(),
        url: url.to_owned(),
        body,
        timeout_ms: Some(15_000),
        ..Default::default()
    };
    req = req.header("Authorization", &auth_header(auth));
    req = req.header("Accept", "application/json");
    if let Some(kind) = content_type {
        req = req.header("Content-Type", kind);
    }
    let resp = host::fetch(&req).map_err(|e| ApiError::Network(e.to_string()))?;
    Ok((resp.status, resp.text()))
}

/// The message Gitee puts in an error body, if any.
fn api_message(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body)
        && let Some(msg) = v.get("message").and_then(|m| m.as_str())
    {
        return msg.to_owned();
    }
    truncate(body, 200).to_owned()
}

fn truncate(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// Map a non-2xx answer to the error the UI acts on.
fn failure(status: u16, body: &str) -> ApiError {
    let message = api_message(body);
    match status {
        401 => ApiError::Unauthorized,
        403 => ApiError::Forbidden(message),
        404 => ApiError::NotFound(message),
        409 => ApiError::Conflict(message),
        422 => ApiError::Invalid(message),
        429 => ApiError::RateLimited,
        500..=599 => ApiError::Server(status, message),
        other => ApiError::BadResponse(format!("HTTP {other}: {message}")),
    }
}

fn absolute(path: &str) -> String {
    if path.starts_with("https://") || path.starts_with("http://") {
        path.to_owned()
    } else {
        format!("{API_BASE}{path}")
    }
}

/// `GET` a JSON document.
pub(crate) fn get<T: DeserializeOwned>(path: &str, auth: AuthMode) -> Result<T, ApiError> {
    let (status, body) = request("GET", &absolute(path), auth, None, None)?;
    if !(200..300).contains(&status) {
        return Err(failure(status, &body));
    }
    serde_json::from_str(&body).map_err(|e| ApiError::BadResponse(format!("{e}: {}", truncate(&body, 160))))
}

/// A form body, as Gitee's write endpoints document their fields (`formData`).
#[derive(Debug, Default)]
pub(crate) struct Form(Vec<(String, String)>);

impl Form {
    pub(crate) fn new() -> Self {
        Self(Vec::new())
    }

    pub(crate) fn push(mut self, key: &str, value: impl Into<String>) -> Self {
        self.0.push((key.to_owned(), value.into()));
        self
    }

    pub(crate) fn push_opt(self, key: &str, value: Option<impl Into<String>>) -> Self {
        match value {
            Some(v) => self.push(key, v),
            None => self,
        }
    }

    fn encode(&self) -> Vec<u8> {
        self.0
            .iter()
            .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
            .collect::<Vec<_>>()
            .join("&")
            .into_bytes()
    }
}

/// Send a write request with a form body and parse the JSON answer.
pub(crate) fn send<T: DeserializeOwned>(method: &str, path: &str, form: Form, auth: AuthMode) -> Result<T, ApiError> {
    let (status, body) = request(
        method,
        &absolute(path),
        auth,
        Some("application/x-www-form-urlencoded"),
        Some(form.encode()),
    )?;
    if !(200..300).contains(&status) {
        return Err(failure(status, &body));
    }
    serde_json::from_str(&body).map_err(|e| ApiError::BadResponse(format!("{e}: {}", truncate(&body, 160))))
}

/// Send a write request whose answer carries nothing the plugin needs.
pub(crate) fn send_empty(method: &str, path: &str, form: Form, auth: AuthMode) -> Result<(), ApiError> {
    let (status, body) = request(
        method,
        &absolute(path),
        auth,
        Some("application/x-www-form-urlencoded"),
        Some(form.encode()),
    )?;
    if !(200..300).contains(&status) {
        return Err(failure(status, &body));
    }
    Ok(())
}

/// A mock host granting what these endpoints need (`secrets`, `net:gitee.com`).
#[cfg(test)]
pub(crate) fn test_host() -> openidea_plugin::mock::MockHost {
    let host = openidea_plugin::mock::MockHost::install()
        .with_capabilities(&["secrets", "net:gitee.com", "project.read", "vcs.read", "vcs.write", "events"]);
    // The `{{secret:…}}` placeholders the client sends must resolve.
    host.set_secret("gitee-token", "test-token");
    host.set_secret("gitee-oauth-token", "test-oauth");
    host
}

#[cfg(test)]
pub(crate) fn json(status: u16, body: &str) -> Result<openidea_plugin::HttpResponse, Error> {
    Ok(openidea_plugin::HttpResponse { status, headers: Vec::new(), body: body.as_bytes().to_vec() })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_remote_forms() {
        assert_eq!(parse_remote("https://gitee.com/foo/bar.git"), Some(RepoRef::new("foo", "bar")));
        assert_eq!(parse_remote("http://gitee.com/foo/bar"), Some(RepoRef::new("foo", "bar")));
        assert_eq!(parse_remote("git@gitee.com:foo/bar.git"), Some(RepoRef::new("foo", "bar")));
        assert_eq!(parse_remote("foo/bar"), Some(RepoRef::new("foo", "bar")));
        assert_eq!(parse_remote("github.com/foo/bar"), None);
        assert_eq!(parse_remote("foo/bar/baz"), None);
        assert_eq!(parse_remote(""), None);
    }

    #[test]
    fn rejects_injection_characters() {
        assert_eq!(parse_remote("foo/../bar"), None);
        assert_eq!(parse_remote("foo/bar; rm -rf /"), None);
        assert_eq!(parse_remote("foo/bar space"), None);
        assert_eq!(parse_remote("foo/bar@evil.com"), None);
        assert_eq!(parse_remote("foo/bar//etc/passwd"), None);
        assert_eq!(parse_remote("foo-bar/baz_qux.1"), Some(RepoRef::new("foo-bar", "baz_qux.1")));
    }

    #[test]
    fn builds_auth_headers() {
        assert_eq!(auth_header(AuthMode::Token), "token {{secret:gitee-token}}");
        assert_eq!(auth_header(AuthMode::OAuth), "Bearer {{secret:gitee-oauth-token}}");
    }

    #[test]
    fn encodes_query_values() {
        assert_eq!(enc("openharmony/docs"), "openharmony%2Fdocs");
        assert_eq!(enc("feature/a b"), "feature%2Fa%20b");
        assert_eq!(enc("a.b_c-d~"), "a.b_c-d~");
    }

    #[test]
    fn forms_encode_and_skip_missing_values() {
        let form = Form::new().push("title", "hi & bye").push_opt("body", None::<String>);
        assert_eq!(String::from_utf8(form.encode()).unwrap(), "title=hi%20%26%20bye");
    }

    #[test]
    fn truncates_long_errors() {
        assert_eq!(truncate("hello", 3), "hel");
        assert_eq!(truncate("hi", 10), "hi");
    }

    #[test]
    fn classifies_failures() {
        assert_eq!(failure(401, "{}"), ApiError::Unauthorized);
        assert_eq!(failure(404, r#"{"message":"Not Found"}"#), ApiError::NotFound("Not Found".into()));
        assert_eq!(failure(429, ""), ApiError::RateLimited);
        assert!(matches!(failure(500, ""), ApiError::Server(500, _)));
        assert!(matches!(failure(422, r#"{"message":"bad"}"#), ApiError::Invalid(_)));
    }

    #[test]
    fn pages_clamp_per_page() {
        assert_eq!(Page::first(500).per_page, Page::MAX_PER_PAGE);
        assert_eq!(Page::first(0).per_page, 1);
        assert_eq!(Page::first(50).query(), "page=1&per_page=50");
    }
}
