//! Repositories: listing, fetching, creating, branches.

use super::*;

/// Repositories the token can see, most recently pushed first.
pub fn list_repositories(auth: AuthMode, page: Page) -> Result<Vec<Repository>, ApiError> {
    get(&format!("/user/repos?sort=pushed&{}", page.query()), auth)
}

pub fn get_repository_at(repo: &RepoRef, auth: AuthMode) -> Result<Repository, ApiError> {
    get(&format!("/repos/{}", repo.slug()), auth)
}

/// Create a repository under the user (`org` when given) and initialise it.
pub fn create_repository(
    name: &str,
    description: &str,
    private: bool,
    org: Option<&str>,
    auth: AuthMode,
) -> Result<Repository, ApiError> {
    let name = name.trim();
    if name.is_empty() || name.len() > 191 || name.contains(['/', '\\', ' ', '\0']) {
        return Err(ApiError::Invalid(format!("invalid repository name: {name:?}")));
    }
    let form = Form::new()
        .push("name", name)
        .push("description", description)
        .push("private", private.to_string())
        .push("auto_init", "true");
    match org.filter(|o| !o.trim().is_empty()) {
        Some(org) => send("POST", &format!("/orgs/{}/repos", enc(org.trim())), form, auth),
        None => send("POST", "/user/repos", form, auth),
    }
}

/// The repository's branches, newest first.
pub fn list_branches(repo: &RepoRef, auth: AuthMode, page: Page) -> Result<Vec<Branch>, ApiError> {
    get(&format!("/repos/{}/branches?{}", repo.slug(), page.query()), auth)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::json;
    
    #[test]
    fn lists_the_users_repositories_with_paging() {
        let host = crate::api::test_host();
        host.on_fetch(|req| {
            assert!(req.url.contains("/user/repos?sort=pushed&page=1&per_page=50"), "{}", req.url);
            json(200, r#"[{"full_name":"acme/widgets","clone_url":"https://gitee.com/acme/widgets.git"}]"#)
        });
        let repos = list_repositories(AuthMode::Token, Page::first(50)).unwrap();
        assert_eq!(repos[0].full_name, "acme/widgets");
    }

    #[test]
    fn rejects_an_invalid_repository_name_before_sending() {
        let host = crate::api::test_host();
        let err = create_repository("bad name", "", false, None, AuthMode::Token).unwrap_err();
        assert!(matches!(err, ApiError::Invalid(_)));
        assert!(host.requests().is_empty());
    }

    #[test]
    fn creates_a_repository_in_an_organization() {
        let host = crate::api::test_host();
        host.on_fetch(|req| {
            assert_eq!(req.method, "POST");
            assert!(req.url.ends_with("/orgs/acme/repos"), "{}", req.url);
            let body = String::from_utf8(req.body.clone().unwrap_or_default()).unwrap();
            assert!(body.contains("name=widgets"), "{body}");
            assert!(body.contains("private=false"), "{body}");
            json(200, r#"{"full_name":"acme/widgets","html_url":"https://gitee.com/acme/widgets"}"#)
        });
        let repo = create_repository("widgets", "demo", false, Some("acme"), AuthMode::Token).unwrap();
        assert_eq!(repo.full_name, "acme/widgets");
    }
}
