//! Gitee integration for OpenIDEA.
//!
//! A plugin that:
//! - shows the current project's Gitee remote and open PR count in the status bar,
//! - opens a **Gitee tool window**: the project's pull requests and issues, and
//!   every repository the token can see (double-click one to clone it),
//! - lists pull requests and issues,
//! - creates a remote Gitee repository,
//! - clones a Gitee repository and pushes the current branch (via `git`).
//!
//! Authentication: personal access token (default) or OAuth2, configured via
//! the settings page; the token is kept in the IDE secret store.

mod api;

use openidea_plugin::{
    Error, Event, Invocation, NotificationLevel, PickItem, Plugin, StatusItem, host, register_plugin,
    views::{Body, Button, TreeItem, View, ViewEvent, Viewport, Views},
};
use api::{AuthMode, RepoRef, parse_remote};

/// Status bar item id.
const STATUS_ITEM: &str = "gitee";

/// Tool window id, declared as `[[contributes.views]]` in `plugin.toml`.
const VIEW_ID: &str = "repos";

#[derive(Default)]
pub struct GiteePlugin {
    /// Repositories of the token, as of the last fetch.
    repos: Vec<api::Repository>,
    /// Open pull requests of the current project's Gitee repository.
    prs: Vec<api::PullRequest>,
    /// Open issues of the current project's Gitee repository.
    issues: Vec<api::Issue>,
    /// Whether the cache holds a fetch from this session.
    loaded: bool,
    /// The last fetch error, shown in the view's status line.
    error: Option<String>,
    /// The tree item the user selected (`repo:<owner/name>`).
    selected: Option<String>,
}

/// The IDE's UI language: manifest text is localized by `locales/<tag>.json`,
/// strings built at run time pick a branch here.
fn chinese() -> bool {
    host::locale().to_ascii_lowercase().starts_with("zh")
}

/// `zh` or `en`, by the IDE's UI language.
fn t(zh: &str, en: &str) -> String {
    if chinese() { zh.to_owned() } else { en.to_owned() }
}

/// Read the user's chosen auth mode from settings (defaults to token).
fn auth_mode() -> AuthMode {
    match host::setting_string("authMode").as_deref() {
        Some("oauth") => AuthMode::OAuth,
        _ => AuthMode::Token,
    }
}

/// Whether the secret the chosen mode needs is stored.
fn has_token() -> bool {
    match auth_mode() {
        AuthMode::Token => host::secret_is_set("gitee-token"),
        AuthMode::OAuth => host::secret_is_set("gitee-oauth-token"),
    }
}

/// Resolve the Gitee `owner/repo` of the current project, if any.
fn current_repo(ctx: &Invocation) -> Option<RepoRef> {
    let project = ctx.project()?;
    // Remotes are `(name, url)` pairs; find the first Gitee one.
    let remotes = project.vcs_remotes().ok()?;
    for (_name, url) in remotes {
        if let Some(r) = parse_remote(&url) {
            return Some(r);
        }
    }
    None
}

impl GiteePlugin {
    fn show_status(&self, ctx: &Invocation) {
        let Some(repo) = current_repo(ctx) else {
            host::remove_status_item(STATUS_ITEM);
            return;
        };
        let text = match api::list_prs(&repo, auth_mode()) {
            Ok(prs) => format!("Gitee: {} · {} open PR", repo, prs.len()),
            Err(Error::Denied(_)) => format!("Gitee: {} · no token", repo),
            Err(_) => format!("Gitee: {}", repo),
        };
        host::set_status_item(
            STATUS_ITEM,
            &StatusItem {
                text,
                tooltip: Some(format!("Gitee repository {}\nClick to open the Gitee panel.", repo)),
                command: Some("show-panel".into()),
            },
        );
    }

    fn cmd_prs(&self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let repo = current_repo(ctx).ok_or("no Gitee repository in this project")?;
        let prs = api::list_prs(&repo, auth_mode())?;
        if prs.is_empty() {
            return Ok(Some(format!("{}: no open pull requests.", repo)));
        }
        let mut out = format!("{} — open pull requests:\n", repo);
        for pr in &prs {
            let head = pr.head.as_ref().map(|h| h.label.as_str()).unwrap_or("?");
            let base = pr.base.as_ref().map(|b| b.label.as_str()).unwrap_or("?");
            out.push_str(&format!(
                "  #{:>4} [{}] {} — {}\n    {} ← {}\n    {}\n",
                pr.number,
                pr.state,
                pr.title,
                pr.user.as_ref().map(|u| u.login.as_str()).unwrap_or("?"),
                base,
                head,
                pr.html_url
            ));
        }
        Ok(Some(out))
    }

    fn cmd_issues(&self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let repo = current_repo(ctx).ok_or("no Gitee repository in this project")?;
        let issues = api::list_issues(&repo, auth_mode())?;
        if issues.is_empty() {
            return Ok(Some(format!("{}: no open issues.", repo)));
        }
        let mut out = format!("{} — open issues:\n", repo);
        for it in &issues {
            out.push_str(&format!(
                "  #{:>4} [{}] {} — {}\n    {}\n",
                it.number,
                it.state,
                it.title,
                it.user.as_ref().map(|u| u.login.as_str()).unwrap_or("?"),
                it.html_url
            ));
        }
        Ok(Some(out))
    }

    fn cmd_create_repo(
        &self,
        _ctx: &Invocation,
        arg: Option<&str>,
    ) -> Result<Option<String>, Error> {
        let name = arg.ok_or("usage: create-repo <name>")?;
        let repo = api::create_repository(name, "", false, auth_mode())?;
        Ok(Some(format!(
            "Created {} — {}\nclone: {} (ssh: {})",
            repo.full_name, repo.html_url, repo.clone_url, repo.ssh_url
        )))
    }

    /// Clone a Gitee repository interactively, like the welcome screen's
    /// clone flow: pick a repository (or enter a URL / owner/repo), choose a
    /// target directory, then clone and open the project.
    fn cmd_clone(
        &self,
        _ctx: &Invocation,
        arg: Option<&str>,
    ) -> Result<Option<String>, Error> {
        // Resolve the remote: an explicit argument wins; otherwise ask the
        // user — entering `owner/repo` or a URL, or picking from their repos.
        let remote: String = match arg {
            Some(a) if !a.trim().is_empty() => a.to_owned(),
            _ => match self.pick_repository()? {
                Some(r) => r.full_name.clone(),
                None => return Ok(None), // user cancelled
            },
        };
        Ok(self.clone_remote(&remote)?.map(|dest| format!("Cloned {remote} into {dest}.")))
    }

    /// Verify `remote`, ask for a directory, `git clone` into it and open the
    /// result as a project. `Ok(None)` when the user cancels the directory.
    fn clone_remote(&self, remote: &str) -> Result<Option<String>, Error> {
        let repo = parse_remote(remote)
            .ok_or_else(|| Error::Invalid(format!("not a Gitee remote: {remote}")))?;

        // Verify the repository exists and is reachable before cloning.
        let info = api::get_repository(remote, auth_mode())?;
        let url = if info.clone_url.is_empty() {
            format!("https://gitee.com/{}.git", repo.slug())
        } else {
            info.clone_url
        };

        // Choose where to put it.
        let target = host::pick_directory("Clone into…", None)?;
        let Some(target) = target else {
            return Ok(None); // user cancelled
        };
        let dest = format!("{}/{}", target.trim_end_matches(['/', '\\']), repo.repo);

        // Clone into the chosen directory (absolute path as git's argument,
        // so the process working directory does not matter).
        let out = host::run_process("git", &["clone".to_owned(), url, dest.clone()], None)?;
        if out.status.map_or(true, |s| s != 0) {
            let msg = if out.stderr.is_empty() { out.stdout } else { out.stderr };
            return Err(Error::Failed(msg));
        }

        // Open the freshly cloned project in the IDE.
        host::open_project(&dest, false)?;
        host::notify(NotificationLevel::Info, "Gitee", &format!("Cloned {} into {}", repo, dest));
        Ok(Some(dest))
    }

    /// Ask the user to pick one of their Gitee repositories, falling back to
    /// manual `owner/repo` input when the list is empty or unavailable.
    /// Returns `Ok(None)` when the user cancels.
    fn pick_repository(&self) -> Result<Option<api::Repository>, Error> {
        match api::list_repositories(auth_mode()) {
            Ok(repos) if !repos.is_empty() => {
                let items: Vec<PickItem> = repos
                    .iter()
                    .map(|r| {
                        PickItem::new(&r.full_name, &r.full_name)
                            .description(if r.clone_url.is_empty() {
                                "repository"
                            } else {
                                &r.clone_url
                            })
                    })
                    .collect();
                if let Some(key) = host::pick("Clone a Gitee repository", &items)? {
                    return Ok(repos.iter().find(|r| r.full_name == key).cloned());
                }
                // User cancelled the picker.
                Ok(None)
            }
            _ => {
                let input = host::input("Gitee repository (owner/repo or URL)", None)?;
                let Some(input) = input else {
                    return Ok(None); // user cancelled
                };
                api::get_repository(&input, auth_mode()).map(Some)
            }
        }
    }

    /// Push the current branch to its upstream via `git` (requires `process:git`).
    fn cmd_push(&self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let project = ctx.project().ok_or("no project open")?;
        let out = project.run_process("git", &["push".to_owned()], None)?;
        if out.status.map_or(true, |s| s != 0) {
            return Err(Error::Failed(if out.stderr.is_empty() {
                out.stdout
            } else {
                out.stderr
            }));
        }
        Ok(Some("Pushed to remote.".to_owned()))
    }

    /// Refetch the repositories and, when the project has a Gitee remote, its
    /// pull requests and issues. Failures land in `error` for the view's status.
    fn reload(&mut self, ctx: &Invocation) {
        self.error = None;
        self.loaded = true;
        self.repos.clear();
        self.prs.clear();
        self.issues.clear();
        if !has_token() {
            return;
        }
        match api::list_repositories(auth_mode()) {
            Ok(repos) => self.repos = repos,
            Err(e) => self.error = Some(e.to_string()),
        }
        if let Some(repo) = current_repo(ctx) {
            match api::list_prs(&repo, auth_mode()) {
                Ok(prs) => self.prs = prs,
                Err(e) => {
                    self.error.get_or_insert(e.to_string());
                }
            }
            match api::list_issues(&repo, auth_mode()) {
                Ok(issues) => self.issues = issues,
                Err(e) => {
                    self.error.get_or_insert(e.to_string());
                }
            }
        }
    }

    /// Clone from the panel: failures become a notification and the status line
    /// instead of failing the view event.
    fn clone_from_view(&mut self, remote: &str) {
        match self.clone_remote(remote) {
            Ok(_) => self.error = None,
            Err(e) => {
                let msg = e.to_string();
                host::notify(NotificationLevel::Error, "Gitee", &msg);
                self.error = Some(msg);
            }
        }
    }

    /// The panel's content: the project's PRs and issues, then every repository.
    fn panel(&mut self, ctx: &Invocation) -> View {
        if !has_token() {
            return View::message(&t(
                "尚未配置 Gitee 令牌。\n打开「设置 › 插件 › Gitee › Secrets」填入私人访问令牌，然后刷新。",
                "No Gitee token yet.\nOpen Settings › Plugins › Gitee › Secrets, add a personal access token, then refresh.",
            ))
            .button(Button::new("refresh", &t("刷新", "Refresh")).icon("refresh"));
        }
        if !self.loaded {
            self.reload(ctx);
        }

        let mut items: Vec<TreeItem> = Vec::new();

        // The current project's Gitee repository, with its open PRs and issues.
        if let Some(repo) = current_repo(ctx) {
            items.push(TreeItem::new("project", &repo.to_string()).icon("branch").expandable(true));
            items.push(
                TreeItem::new("prs", &format!("Pull Requests ({})", self.prs.len()))
                    .parent("project")
                    .icon("git-pull"),
            );
            for pr in &self.prs {
                let label = format!("#{} {}", pr.number, pr.title);
                let mut item = TreeItem::new(&format!("pr:{}", pr.number), &label).parent("prs");
                if let Some(user) = &pr.user {
                    item = item.detail(&user.login);
                }
                items.push(item);
            }
            items.push(
                TreeItem::new("issues", &format!("Issues ({})", self.issues.len()))
                    .parent("project")
                    .icon("branch"),
            );
            for issue in &self.issues {
                let label = format!("#{} {}", issue.number, issue.title);
                let mut item = TreeItem::new(&format!("issue:{}", issue.number), &label).parent("issues");
                if let Some(user) = &issue.user {
                    item = item.detail(&user.login);
                }
                items.push(item);
            }
        }

        // Every repository the token can see; double-click one to clone it.
        items.push(
            TreeItem::new("mine", &format!("{} ({})", t("我的仓库", "My repositories"), self.repos.len()))
                .icon("branch")
                .expandable(true),
        );
        for repo in &self.repos {
            let mut item = TreeItem::new(&format!("repo:{}", repo.full_name), &repo.full_name).parent("mine");
            if !repo.clone_url.is_empty() {
                item = item.detail(&repo.clone_url);
            }
            items.push(item);
        }

        let clone_enabled = self.selected.as_deref().is_some_and(|key| key.starts_with("repo:"));
        let mut view = View::new(Body::Tree(items))
            .button(Button::new("refresh", &t("刷新", "Refresh")).icon("refresh"))
            .button(Button::new("clone", &t("克隆选中项", "Clone Selected")).icon("git-pull").enabled(clone_enabled))
            .button(Button::new("create", &t("新建仓库", "New Repository")).icon("add"));

        view = match &self.error {
            Some(error) => view.status(&format!("{}: {error}", t("错误", "Error"))),
            None => view.status(&format!(
                "{} {} · {} PR · {} Issue",
                self.repos.len(),
                t("个仓库", "repositories"),
                self.prs.len(),
                self.issues.len()
            )),
        };
        view
    }
}

impl Views for GiteePlugin {
    fn render(&mut self, view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        if view != VIEW_ID {
            return Err(Error::NotFound(format!("view {view}")));
        }
        Ok(self.panel(ctx))
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        if view != VIEW_ID {
            return Err(Error::NotFound(format!("view {view}")));
        }
        match event {
            ViewEvent::Button(id) => match id.as_str() {
                "refresh" => self.reload(ctx),
                "clone" => {
                    if let Some(remote) = self.selected.as_deref().and_then(|key| key.strip_prefix("repo:")) {
                        let remote = remote.to_owned();
                        self.clone_from_view(&remote);
                    }
                }
                "create" => {
                    if let Some(name) = host::input(&t("新建 Gitee 仓库名", "New Gitee repository name"), None)? {
                        match api::create_repository(&name, "", false, auth_mode()) {
                            Ok(repo) => {
                                host::notify(
                                    NotificationLevel::Info,
                                    "Gitee",
                                    &format!("{} {}", t("已创建", "Created"), repo.full_name),
                                );
                                self.reload(ctx);
                            }
                            Err(e) => self.error = Some(e.to_string()),
                        }
                    }
                }
                _ => {}
            },
            ViewEvent::Select(key) => self.selected = Some(key),
            ViewEvent::Activate(key) => {
                if let Some(remote) = key.strip_prefix("repo:") {
                    let remote = remote.to_owned();
                    self.clone_from_view(&remote);
                }
            }
            _ => {}
        }
        // The IDE renders the view again: the cache and status line changed.
        host::refresh_view(VIEW_ID);
        Ok(())
    }
}

impl Plugin for GiteePlugin {
    fn run_command(
        &mut self,
        id: &str,
        ctx: &Invocation,
        arg: Option<&str>,
    ) -> Result<Option<String>, Error> {
        match id {
            "prs" => self.cmd_prs(ctx),
            "issues" => self.cmd_issues(ctx),
            "create-repo" => self.cmd_create_repo(ctx, arg),
            "clone" => self.cmd_clone(ctx, arg),
            "push" => self.cmd_push(ctx),
            "refresh" => {
                self.loaded = false;
                self.show_status(ctx);
                Ok(None)
            }
            "show-panel" => {
                host::reveal_view(VIEW_ID)?;
                Ok(None)
            }
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }

    fn handle_events(&mut self, events: &[Event], ctx: &Invocation) {
        // Refresh the status bar on schedule and on VCS changes; the panel
        // refetches the next time it is shown.
        if events
            .iter()
            .any(|e| matches!(e, Event::Scheduled | Event::VcsChanged(_)))
        {
            self.loaded = false;
            self.show_status(ctx);
        }
    }
}

register_plugin!(GiteePlugin, views);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    #[test]
    fn auth_mode_defaults_to_token() {
        // No settings in a fresh MockHost: falls back to token.
        let host = MockHost::install();
        let _ = &host;
        assert_eq!(auth_mode(), AuthMode::Token);
    }

    #[test]
    fn parses_gitee_remote() {
        assert_eq!(
            parse_remote("git@gitee.com:acme/widgets.git"),
            Some(RepoRef::new("acme", "widgets"))
        );
        assert_eq!(parse_remote("https://github.com/a/b"), None);
    }

    #[test]
    fn panel_asks_for_a_token_before_listing_anything() {
        let host = MockHost::install();
        let ctx = host.invocation();
        let mut plugin = GiteePlugin::default();
        let view = plugin.render(VIEW_ID, None, &ctx).unwrap();
        match view.body {
            Body::Message(text) => assert!(text.contains("令牌") || text.contains("token")),
            other => panic!("expected a message body, got {other:?}"),
        }
        // Nothing was fetched without a token.
        assert!(host.requests().is_empty());
    }

    #[test]
    fn panel_lists_the_tokens_repositories() {
        // The panel reads the token (`secrets`) and lists it over the network.
        let host = MockHost::install().with_capabilities(&["secrets", "net:gitee.com", "project.read", "vcs.read"]);
        host.set_secret("gitee-token", "secret");
        host.on_fetch(|req| {
            let body = if req.url.contains("/user/repos") {
                r#"[{"full_name":"acme/widgets","html_url":"https://gitee.com/acme/widgets","ssh_url":"git@gitee.com:acme/widgets.git","clone_url":"https://gitee.com/acme/widgets.git"}]"#
            } else {
                "[]"
            };
            Ok(openidea_plugin::HttpResponse { status: 200, headers: Vec::new(), body: body.as_bytes().to_vec() })
        });
        let ctx = host.invocation();
        let mut plugin = GiteePlugin::default();
        let view = plugin.render(VIEW_ID, None, &ctx).unwrap();
        match view.body {
            Body::Tree(items) => {
                assert!(
                    items.iter().any(|i| i.key == "repo:acme/widgets"),
                    "the repository should be listed: {items:?}"
                );
            }
            other => panic!("expected a tree body, got {other:?}"),
        }
        // Selecting the repository enables the clone button.
        plugin.handle(VIEW_ID, ViewEvent::Select("repo:acme/widgets".into()), &ctx).unwrap();
        let view = plugin.render(VIEW_ID, None, &ctx).unwrap();
        assert!(view.toolbar.iter().any(|b| b.id == "clone" && b.enabled));
    }

    #[test]
    fn unknown_views_are_rejected() {
        let host = MockHost::install();
        let ctx = host.invocation();
        let mut plugin = GiteePlugin::default();
        assert!(matches!(plugin.render("nope", None, &ctx), Err(Error::NotFound(_))));
    }
}
