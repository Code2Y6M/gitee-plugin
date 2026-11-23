//! Pull requests: listing, details, files, commits, comments, creating,
//! merging and approving.

use super::*;

/// Which pull requests to list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[derive(Default)]
pub enum PrFilter {
    #[default]
    Open,
    Merged,
    Closed,
    All,
}


impl PrFilter {
    fn state(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Merged => "merged",
            Self::Closed => "closed",
            Self::All => "all",
        }
    }

    /// The next filter, for a toolbar button that cycles through them.
    pub fn next(self) -> Self {
        match self {
            Self::Open => Self::Merged,
            Self::Merged => Self::Closed,
            Self::Closed => Self::All,
            Self::All => Self::Open,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Merged => "merged",
            Self::Closed => "closed",
            Self::All => "all",
        }
    }
}

/// How `PUT /pulls/{n}/merge` combines the commits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}

impl MergeMethod {
    fn as_str(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Squash => "squash",
            Self::Rebase => "rebase",
        }
    }

    pub fn all() -> [Self; 3] {
        [Self::Merge, Self::Squash, Self::Rebase]
    }
}

/// The pull requests of a repository.
pub fn list_prs(
    repo: &RepoRef,
    filter: PrFilter,
    auth: AuthMode,
    page: Page,
) -> Result<Vec<PullRequest>, ApiError> {
    get(&format!("/repos/{}/pulls?state={}&{}", repo.slug(), filter.state(), page.query()), auth)
}

pub fn get_pr(repo: &RepoRef, number: u64, auth: AuthMode) -> Result<PullRequest, ApiError> {
    get(&format!("/repos/{}/pulls/{number}", repo.slug()), auth)
}

/// The files a pull request changes (Gitee returns at most 300 diffs; large
/// files come without a patch).
pub fn pr_files(repo: &RepoRef, number: u64, auth: AuthMode) -> Result<Vec<ChangedFile>, ApiError> {
    get(&format!("/repos/{}/pulls/{number}/files?per_page=100", repo.slug()), auth)
}

/// The commits of a pull request (at most 250).
pub fn pr_commits(repo: &RepoRef, number: u64, auth: AuthMode) -> Result<Vec<PrCommit>, ApiError> {
    get(&format!("/repos/{}/pulls/{number}/commits?per_page=100", repo.slug()), auth)
}

/// The conversation comments of a pull request.
pub fn pr_comments(repo: &RepoRef, number: u64, auth: AuthMode) -> Result<Vec<Comment>, ApiError> {
    get(&format!("/repos/{}/pulls/{number}/comments?per_page=100", repo.slug()), auth)
}

/// Open a pull request; `issue` (an issue number) fills what the issue says.
pub fn create_pr(
    repo: &RepoRef,
    title: &str,
    head: &str,
    base: &str,
    body: &str,
    issue: Option<&str>,
    auth: AuthMode,
) -> Result<PullRequest, ApiError> {
    if title.trim().is_empty() {
        return Err(ApiError::Invalid("a pull request needs a title".into()));
    }
    if head.trim().is_empty() || base.trim().is_empty() {
        return Err(ApiError::Invalid("a pull request needs a source and a target branch".into()));
    }
    let form = Form::new()
        .push("title", title.trim())
        .push("head", head.trim())
        .push("base", base.trim())
        .push_opt("body", (!body.trim().is_empty()).then(|| body.to_owned()))
        .push_opt("issue", issue.map(str::to_owned));
    send("POST", &format!("/repos/{}/pulls", repo.slug()), form, auth)
}

/// Merge a pull request.
pub fn merge_pr(
    repo: &RepoRef,
    number: u64,
    method: MergeMethod,
    prune_source_branch: bool,
    close_related_issue: bool,
    auth: AuthMode,
) -> Result<(), ApiError> {
    let form = Form::new()
        .push("merge_method", method.as_str())
        .push("prune_source_branch", prune_source_branch.to_string())
        .push("close_related_issue", close_related_issue.to_string());
    send_empty("PUT", &format!("/repos/{}/pulls/{number}/merge", repo.slug()), form, auth)
}

/// Add a comment to the pull request's conversation.
pub fn comment_pr(repo: &RepoRef, number: u64, body: &str, auth: AuthMode) -> Result<Comment, ApiError> {
    if body.trim().is_empty() {
        return Err(ApiError::Invalid("an empty comment".into()));
    }
    let form = Form::new().push("body", body.to_owned());
    send("POST", &format!("/repos/{}/pulls/{number}/comments", repo.slug()), form, auth)
}

/// Approve the pull request (`POST /pulls/{n}/review`).
pub fn approve_pr(repo: &RepoRef, number: u64, auth: AuthMode) -> Result<(), ApiError> {
    send_empty("POST", &format!("/repos/{}/pulls/{number}/review", repo.slug()), Form::new(), auth)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::json;
    
    fn repo() -> RepoRef {
        RepoRef::new("acme", "widgets")
    }

    #[test]
    fn lists_pull_requests_by_state() {
        let host = crate::api::test_host();
        host.on_fetch(|req| {
            assert!(req.url.contains("/repos/acme/widgets/pulls?state=merged&page=1&per_page=50"), "{}", req.url);
            json(200, r#"[{"number":7,"title":"t","state":"closed","merged_at":"2025-01-01T00:00:00Z"}]"#)
        });
        let prs = list_prs(&repo(), PrFilter::Merged, AuthMode::Token, Page::first(50)).unwrap();
        assert_eq!(prs[0].number, 7);
        assert_eq!(prs[0].status(), "merged");
    }

    #[test]
    fn reads_a_changed_file_with_object_patch_and_string_counts() {
        let host = crate::api::test_host();
        host.on_fetch(|_| {
            json(
                200,
                r#"[{"filename":"a.md","status":null,"additions":"2","deletions":"1","patch":{"diff":"@@ -1 +1 @@"}}]"#,
            )
        });
        let files = pr_files(&repo(), 7, AuthMode::Token).unwrap();
        assert_eq!(files[0].filename, "a.md");
        assert_eq!(files[0].stats(), "+2 −1");
        assert_eq!(files[0].patch.as_deref(), Some("@@ -1 +1 @@"));
    }

    #[test]
    fn creates_a_pull_request_with_a_form_body() {
        let host = crate::api::test_host();
        host.on_fetch(|req| {
            assert_eq!(req.method, "POST");
            assert!(req.url.ends_with("/repos/acme/widgets/pulls"));
            let body = String::from_utf8(req.body.clone().unwrap()).unwrap();
            assert!(body.contains("title=fix%20the%20thing"), "{body}");
            assert!(body.contains("head=feature%2Fx"), "{body}");
            assert!(body.contains("base=master"), "{body}");
            json(200, r#"{"number":9,"title":"fix the thing","state":"open"}"#)
        });
        let pr = create_pr(&repo(), "fix the thing", "feature/x", "master", "", None, AuthMode::Token).unwrap();
        assert_eq!(pr.number, 9);
    }

    #[test]
    fn merges_with_the_chosen_method() {
        let host = crate::api::test_host();
        host.on_fetch(|req| {
            assert_eq!(req.method, "PUT");
            assert!(req.url.ends_with("/pulls/9/merge"));
            let body = String::from_utf8(req.body.clone().unwrap()).unwrap();
            assert!(body.contains("merge_method=squash"), "{body}");
            assert!(body.contains("prune_source_branch=true"), "{body}");
            json(200, "{}")
        });
        merge_pr(&repo(), 9, MergeMethod::Squash, true, false, AuthMode::Token).unwrap();
    }

    #[test]
    fn refuses_empty_comments_and_titles() {
        assert!(matches!(comment_pr(&repo(), 1, "  ", AuthMode::Token), Err(ApiError::Invalid(_))));
        assert!(matches!(
            create_pr(&repo(), " ", "a", "b", "", None, AuthMode::Token),
            Err(ApiError::Invalid(_))
        ));
    }
}
