//! Issues: listing, details, comments, creating, updating, labels.

use super::*;

/// Which issues to list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[derive(Default)]
pub enum IssueFilter {
    #[default]
    Open,
    Closed,
    All,
}


impl IssueFilter {
    fn state(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::All => "all",
        }
    }

    /// The next filter, for a toolbar button that cycles through them.
    pub fn next(self) -> Self {
        match self {
            Self::Open => Self::Closed,
            Self::Closed => Self::All,
            Self::All => Self::Open,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::All => "all",
        }
    }
}

pub fn list_issues(
    repo: &RepoRef,
    filter: IssueFilter,
    auth: AuthMode,
    page: Page,
) -> Result<Vec<Issue>, ApiError> {
    get(&format!("/repos/{}/issues?state={}&{}", repo.slug(), filter.state(), page.query()), auth)
}

pub fn get_issue(repo: &RepoRef, number: &str, auth: AuthMode) -> Result<Issue, ApiError> {
    get(&format!("/repos/{}/issues/{}", repo.slug(), enc(number)), auth)
}

pub fn issue_comments(repo: &RepoRef, number: &str, auth: AuthMode) -> Result<Vec<Comment>, ApiError> {
    get(&format!("/repos/{}/issues/{}/comments?per_page=100", repo.slug(), enc(number)), auth)
}

/// Create an issue. Gitee's create endpoint is owner-scoped (`repo` names the
/// repository), unlike the read endpoints.
pub fn create_issue(
    repo: &RepoRef,
    title: &str,
    body: &str,
    labels: &[String],
    assignee: Option<&str>,
    auth: AuthMode,
) -> Result<Issue, ApiError> {
    if title.trim().is_empty() {
        return Err(ApiError::Invalid("an issue needs a title".into()));
    }
    let form = Form::new()
        .push("repo", repo.repo.clone())
        .push("title", title.trim())
        .push_opt("body", (!body.trim().is_empty()).then(|| body.to_owned()))
        .push_opt("labels", (!labels.is_empty()).then(|| labels.join(",")))
        .push_opt("assignee", assignee.map(str::to_owned));
    send("POST", &format!("/repos/{}/issues", enc(&repo.owner)), form, auth)
}

/// Change an issue's state: `open`, `closed`, `progressing` or `rejected`.
pub fn set_issue_state(repo: &RepoRef, number: &str, state: &str, auth: AuthMode) -> Result<Issue, ApiError> {
    if !matches!(state, "open" | "closed" | "progressing" | "rejected") {
        return Err(ApiError::Invalid(format!("unknown issue state: {state}")));
    }
    let form = Form::new().push("state", state);
    send("PATCH", &format!("/repos/{}/issues/{}", enc(&repo.owner), enc(number)), form, auth)
}

pub fn comment_issue(repo: &RepoRef, number: &str, body: &str, auth: AuthMode) -> Result<Comment, ApiError> {
    if body.trim().is_empty() {
        return Err(ApiError::Invalid("an empty comment".into()));
    }
    let form = Form::new().push("body", body.to_owned());
    send("POST", &format!("/repos/{}/issues/{}/comments", repo.slug(), enc(number)), form, auth)
}

/// The repository's labels, for the issue forms.
pub fn list_labels(repo: &RepoRef, auth: AuthMode) -> Result<Vec<RepoLabel>, ApiError> {
    get(&format!("/repos/{}/labels?per_page=100", repo.slug()), auth)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::json;
    
    fn repo() -> RepoRef {
        RepoRef::new("acme", "widgets")
    }

    #[test]
    fn lists_issues_with_string_numbers() {
        let host = crate::api::test_host();
        host.on_fetch(|_| {
            json(
                200,
                r#"[{"number":"ICK440","title":"t","state":"open","labels":["bug",{"name":"p1"}],"comments":2}]"#,
            )
        });
        let issues = list_issues(&repo(), IssueFilter::Open, AuthMode::Token, Page::first(50)).unwrap();
        assert_eq!(issues[0].number, "ICK440");
        assert_eq!(issues[0].labels.len(), 2);
        assert_eq!(issues[0].labels[1].0, "p1");
    }

    #[test]
    fn creates_an_issue_on_the_owner_endpoint() {
        let host = crate::api::test_host();
        host.on_fetch(|req| {
            assert_eq!(req.method, "POST");
            assert!(req.url.ends_with("/repos/acme/issues"), "{}", req.url);
            let body = String::from_utf8(req.body.clone().unwrap()).unwrap();
            assert!(body.contains("repo=widgets"), "{body}");
            assert!(body.contains("title=broken"), "{body}");
            assert!(body.contains("labels=bug%2Cp1"), "{body}");
            json(200, r#"{"number":"42","title":"broken","state":"open"}"#)
        });
        let issue = create_issue(&repo(), "broken", "", &["bug".into(), "p1".into()], None, AuthMode::Token).unwrap();
        assert_eq!(issue.number, "42");
    }

    #[test]
    fn closes_an_issue() {
        let host = crate::api::test_host();
        host.on_fetch(|req| {
            assert_eq!(req.method, "PATCH");
            assert!(req.url.ends_with("/repos/acme/issues/ICK440"), "{}", req.url);
            assert_eq!(String::from_utf8(req.body.clone().unwrap()).unwrap(), "state=closed");
            json(200, r#"{"number":"ICK440","title":"t","state":"closed"}"#)
        });
        let issue = set_issue_state(&repo(), "ICK440", "closed", AuthMode::Token).unwrap();
        assert_eq!(issue.state, "closed");
    }

    #[test]
    fn rejects_unknown_states() {
        assert!(matches!(
            set_issue_state(&repo(), "1", "done", AuthMode::Token),
            Err(ApiError::Invalid(_))
        ));
    }
}
