//! Gitee integration for OpenIDEA.
//!
//! - a **Gitee tool window**: notifications, the project's pull requests and
//!   issues, and every repository of the token,
//! - read: pull request details, changed files, commits, conversations,
//! - write: create/merge/approve/comment pull requests, create/comment/close
//!   issues, create repositories, clone and push,
//! - the commit message toolbar and a commit check that asks for an issue,
//! - MCP tools and a Search Everywhere category for the AI agent and search.
//!
//! Authentication: personal access token (default) or OAuth2, configured on the
//! settings page; the token lives in the IDE secret store and never reaches
//! plugin code — the IDE fills `{{secret:name}}` into the request header.

mod api;
mod i18n;

use api::*;
use i18n::t;
use openidea_plugin::{
    CommitDraft, ContentTab, Error, Event, Invocation, McpProvider, NotificationLevel, PickItem, Plugin,
    StatusItem,
    StatusMenuEntry, host, register_plugin,
    commit::{CommitChecks, CommitProblem},
    search::{SearchItem, SearchPage, SearchProvider, SearchTarget},
    views::{Body, Button, TreeItem, View, ViewEvent, Viewport, Views},
};
use std::time::Instant;

/// Status bar item id.
const STATUS_ITEM: &str = "gitee";

/// Tool window id, declared as `[[contributes.views]]` in `plugin.toml`.
const VIEW_ID: &str = "repos";

/// The commit check declared as `[[contributes.commitChecks]]`.
const COMMIT_CHECK: &str = "issue-reference";

#[derive(Default)]
pub struct GiteePlugin {
    /// Repositories of the token, as of the last fetch.
    repos: Vec<Repository>,
    repos_at: Option<Instant>,
    /// Pull requests of the project's Gitee repository.
    prs: Vec<PullRequest>,
    prs_at: Option<Instant>,
    /// Issues of the project's Gitee repository.
    issues: Vec<Issue>,
    issues_at: Option<Instant>,
    /// The notification inbox.
    notifications: Vec<Notification>,
    notifications_at: Option<Instant>,
    /// Which pull requests and issues to ask for.
    pr_filter: PrFilter,
    issue_filter: IssueFilter,
    /// The tree item the user selected (a `kind:value` key).
    selected: Option<String>,
    /// The last failure, shown in the panel's status line.
    error: Option<String>,
}

// ---------------------------------------------------------------- settings

fn setting_string(key: &str) -> Option<String> {
    host::setting_string(key).filter(|v| !v.trim().is_empty())
}

fn setting_u32(key: &str, default: u32) -> u32 {
    setting_string(key).and_then(|v| v.trim().parse().ok()).unwrap_or(default)
}

fn setting_bool(key: &str, default: bool) -> bool {
    match setting_string(key).as_deref() {
        Some("true") | Some("1") | Some("yes") => true,
        Some("false") | Some("0") | Some("no") => false,
        _ => default,
    }
}

fn page_size() -> u32 {
    setting_u32("defaultPageSize", 50).clamp(10, Page::MAX_PER_PAGE)
}

fn cache_seconds() -> u64 {
    u64::from(setting_u32("refreshSeconds", 30).clamp(5, 600))
}

/// Read the user's chosen auth mode from settings (defaults to token).
fn auth_mode() -> AuthMode {
    match setting_string("authMode").as_deref() {
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

// ---------------------------------------------------------------- project

/// Resolve the Gitee `owner/repo` of the current project, if any.
///
/// A project may have several remotes (a fork, an upstream): the setting
/// `preferredRemote` names the one to use, otherwise the first Gitee remote wins.
fn current_repo(ctx: &Invocation) -> Option<RepoRef> {
    let project = ctx.project()?;
    let remotes = project.vcs_remotes().ok()?;
    if let Some(name) = setting_string("preferredRemote")
        && let Some((_, url)) = remotes.iter().find(|(n, _)| n == &name)
        && let Some(repo) = parse_remote(url)
    {
        return Some(repo);
    }
    remotes.iter().find_map(|(_, url)| parse_remote(url))
}

/// The project's branch, for "create pull request".
fn current_branch(ctx: &Invocation) -> Option<String> {
    ctx.project()?.vcs_status().ok().flatten()?.branch
}

/// A repository-relative path of the focused editor file, if it is inside the project.
fn editor_relative_path(ctx: &Invocation) -> Option<String> {
    let editor = ctx.editor()?;
    let root = ctx.project()?.root().ok()?;
    let path = editor.path().replace('\\', "/");
    let root = root.replace('\\', "/");
    let root = root.trim_end_matches('/');
    path.strip_prefix(root).map(|p| p.trim_start_matches('/').to_owned())
}

// ---------------------------------------------------------------- cache

/// A pull request filter as the panel shows it.
fn pr_filter_text(filter: PrFilter) -> String {
    match filter {
        PrFilter::Open => t("开启", "open"),
        PrFilter::Merged => t("已合并", "merged"),
        PrFilter::Closed => t("已关闭", "closed"),
        PrFilter::All => t("全部", "all"),
    }
}

/// An issue filter as the panel shows it.
fn issue_filter_text(filter: IssueFilter) -> String {
    match filter {
        IssueFilter::Open => t("开启", "open"),
        IssueFilter::Closed => t("已关闭", "closed"),
        IssueFilter::All => t("全部", "all"),
    }
}

fn fresh(at: Option<Instant>, seconds: u64) -> bool {
    at.is_some_and(|at| at.elapsed().as_secs() < seconds)
}

impl GiteePlugin {
    fn invalidate(&mut self) {
        self.repos_at = None;
        self.prs_at = None;
        self.issues_at = None;
        self.notifications_at = None;
    }

    fn ensure_repos(&mut self, force: bool) -> Result<(), ApiError> {
        if !force && fresh(self.repos_at, cache_seconds()) {
            return Ok(());
        }
        self.repos = api::list_repositories(auth_mode(), Page::first(page_size()))?;
        self.repos_at = Some(Instant::now());
        Ok(())
    }

    fn ensure_prs(&mut self, repo: &RepoRef, force: bool) -> Result<(), ApiError> {
        if !force && fresh(self.prs_at, cache_seconds()) {
            return Ok(());
        }
        self.prs = api::list_prs(repo, self.pr_filter, auth_mode(), Page::first(page_size()))?;
        self.prs_at = Some(Instant::now());
        Ok(())
    }

    fn ensure_issues(&mut self, repo: &RepoRef, force: bool) -> Result<(), ApiError> {
        if !force && fresh(self.issues_at, cache_seconds()) {
            return Ok(());
        }
        self.issues = api::list_issues(repo, self.issue_filter, auth_mode(), Page::first(page_size()))?;
        self.issues_at = Some(Instant::now());
        Ok(())
    }

    fn ensure_notifications(&mut self, force: bool) -> Result<(), ApiError> {
        if !force && fresh(self.notifications_at, cache_seconds()) {
            return Ok(());
        }
        self.notifications = api::list_notifications(auth_mode(), false, Page::first(30))?;
        self.notifications_at = Some(Instant::now());
        Ok(())
    }

    fn unread(&self) -> usize {
        self.notifications.iter().filter(|n| n.unread).count()
    }

    /// Fetch everything the panel shows, collecting the first failure.
    fn refresh_all(&mut self, ctx: &Invocation, force: bool) {
        self.error = None;
        if !has_token() {
            return;
        }
        if let Err(e) = self.ensure_repos(force) {
            self.error = Some(e.to_string());
        }
        if let Some(repo) = current_repo(ctx) {
            if let Err(e) = self.ensure_prs(&repo, force) {
                self.error.get_or_insert(e.to_string());
            }
            if let Err(e) = self.ensure_issues(&repo, force) {
                self.error.get_or_insert(e.to_string());
            }
        }
        if let Err(e) = self.ensure_notifications(force) {
            self.error.get_or_insert(e.to_string());
        }
    }

    /// Report a failure of something the user asked for from the panel.
    fn notify_error(&self, error: &Error) {
        host::notify(NotificationLevel::Error, "Gitee", &error.to_string());
    }

    /// Confirm a change on Gitee when the setting asks for it.
    fn confirm(&self, action: &str, detail: &str) -> Result<bool, Error> {
        if !setting_bool("confirmBeforeWrite", true) {
            return Ok(true);
        }
        host::confirm(action, detail)
    }
}

// ---------------------------------------------------------------- status bar

impl GiteePlugin {
    fn show_status(&self, ctx: &Invocation) {
        let Some(repo) = current_repo(ctx) else {
            host::remove_status_item(STATUS_ITEM);
            return;
        };
        let (unread, total) = if self.notifications_at.is_some() {
            (self.unread(), self.notifications.len())
        } else if has_token() {
            match api::notification_count(auth_mode()) {
                Ok(count) => (count.unread as usize, count.total as usize),
                Err(_) => (0, 0),
            }
        } else {
            (0, 0)
        };
        let text = match self.prs_at {
            Some(_) if has_token() => {
                let prs = self.prs.iter().filter(|pr| pr.is_open()).count();
                if unread > 0 {
                    format!("Gitee: {repo} · {prs} PR · {unread} {}", t("未读", "unread"))
                } else {
                    format!("Gitee: {repo} · {prs} PR")
                }
            }
            _ if has_token() => format!("Gitee: {repo}"),
            _ => format!("Gitee: {repo} · {}", t("未配置令牌", "no token")),
        };
        let item = StatusItem {
            text,
            tooltip: Some(format!(
                "Gitee {repo}\n{unread}/{total} {}\n{}",
                t("未读通知", "unread notifications"),
                t("点击查看更多操作。", "Click for actions.")
            )),
            command: Some("show-panel".into()),
        };
        let menu = [
            StatusMenuEntry::Header(repo.to_string()),
            StatusMenuEntry::action(t("打开 Gitee 面板", "Open Gitee Panel"), "show-panel"),
            StatusMenuEntry::Action {
                label: t("通知", "Notifications"),
                description: Some(if unread > 0 {
                    format!("{unread} {}", t("条未读", "unread"))
                } else {
                    t("无未读", "none unread")
                }),
                command: "notifications".into(),
                argument: None,
            },
            StatusMenuEntry::action(t("创建 Pull Request…", "Create Pull Request…"), "create-pr"),
            StatusMenuEntry::Separator,
            StatusMenuEntry::action(t("全部 Gitee 命令…", "All Gitee Commands…"), "commands"),
            StatusMenuEntry::action(t("刷新", "Refresh"), "refresh"),
        ];
        let _ = host::set_status_item_menu(STATUS_ITEM, &item, &menu);
    }
}

// ---------------------------------------------------------------- panel

impl GiteePlugin {
    /// The panel: notifications, the project's pull requests and issues, then
    /// every repository the token can see.
    fn panel(&mut self, ctx: &Invocation) -> View {
        if !has_token() {
            return View::message(&t(
                "尚未配置 Gitee 令牌。\n打开「设置 › 插件 › Gitee › Secrets」填入私人访问令牌，然后刷新。",
                "No Gitee token yet.\nOpen Settings › Plugins › Gitee › Secrets, add a personal access token, then refresh.",
            ))
            .button(Button::new("refresh", &t("刷新", "Refresh")).icon("refresh"))
            .button(Button::new("commands", &t("命令", "Commands")).icon("menu"));
        }

        if self.repos_at.is_none() && self.error.is_none() {
            self.refresh_all(ctx, false);
        }

        let mut items: Vec<TreeItem> = Vec::new();

        // The inbox, when there is anything in it.
        if self.notifications_at.is_some() {
            let unread = self.unread();
            let title = if unread > 0 {
                format!("{} ({unread})", t("通知", "Notifications"))
            } else {
                t("通知", "Notifications")
            };
            items.push(TreeItem::new("notifs", &title).icon("bell").expandable(unread > 0));
            for note in self.notifications.iter().take(20) {
                let label = if note.title.is_empty() { note.kind.clone() } else { note.title.clone() };
                let mut item = TreeItem::new(&format!("notif:{}", note.id), &label).parent("notifs");
                let detail = match (&note.updated_at, note.repo.is_empty()) {
                    (Some(date), false) => format!("{} · {date}", note.repo),
                    (Some(date), true) => date.clone(),
                    (None, false) => note.repo.clone(),
                    (None, true) => String::new(),
                };
                if !detail.is_empty() {
                    item = item.detail(&detail);
                }
                items.push(item);
            }
        }

        // The current project's Gitee repository.
        if let Some(repo) = current_repo(ctx) {
            let project_label = format!("{} · {repo}", t("当前项目", "Current project"));
            items.push(TreeItem::new("project", &project_label).icon("branch").expandable(true));

            let pr_label = format!("Pull Requests ({}) · {}", self.prs.len(), pr_filter_text(self.pr_filter));
            items.push(TreeItem::new("prs", &pr_label).parent("project").icon("git-pull"));
            for pr in &self.prs {
                let label = format!("#{} {}", pr.number, pr.title);
                let author = pr.user.as_ref().map(|u| u.label().to_owned()).unwrap_or_default();
                let detail = format!("{} · {author}", pr.status());
                let item = TreeItem::new(&format!("pr:{}", pr.number), &label)
                    .parent("prs")
                    .detail(detail.trim_end_matches(" · "));
                items.push(item);
            }

            let issue_label = format!("Issues ({}) · {}", self.issues.len(), issue_filter_text(self.issue_filter));
            items.push(TreeItem::new("issues", &issue_label).parent("project").icon("branch"));
            for issue in &self.issues {
                let label = format!("#{} {}", issue.number, issue.title);
                let mut item = TreeItem::new(&format!("issue:{}", issue.number), &label).parent("issues");
                if let Some(user) = &issue.user {
                    item = item.detail(&user.login);
                }
                items.push(item);
            }
        }

        // Every repository the token can see.
        items.push(
            TreeItem::new("mine", &format!("{} ({})", t("我的仓库", "My repositories"), self.repos.len()))
                .icon("branch")
                .expandable(true),
        );
        for repo in &self.repos {
            let mut item = TreeItem::new(&format!("repo:{}", repo.full_name), &repo.full_name).parent("mine");
            let source = if repo.clone_url.is_empty() { repo.description.clone() } else { Some(repo.clone_url.clone()) };
            let detail = match (repo.is_private, source) {
                (true, Some(text)) => Some(format!("{} · {text}", t("私有", "private"))),
                (true, None) => Some(t("私有", "private").to_owned()),
                (false, other) => other,
            };
            if let Some(detail) = detail.filter(|d| !d.is_empty()) {
                item = item.detail(&detail);
            }
            items.push(item);
        }

        let clone_enabled = self.selected.as_deref().is_some_and(|k| k.starts_with("repo:"));
        let project = current_repo(ctx).is_some();
        let mut view = View::new(Body::Tree(items))
            .button(Button::new("refresh", &t("刷新", "Refresh")).icon("refresh"))
            .button(Button::new("filter", &format!("PR: {}", pr_filter_text(self.pr_filter))).icon("filter"))
            .button(Button::new("filter-issues", &format!("Issue: {}", issue_filter_text(self.issue_filter))).icon("filter"))
            .button(Button::new("new-pr", &t("新建 PR", "New PR")).icon("git-pull").enabled(project))
            .button(Button::new("clone", &t("克隆", "Clone")).icon("branch").enabled(clone_enabled))
            .button(Button::new("commands", &t("命令", "Commands")).icon("menu"));

        view = match &self.error {
            Some(error) => view.status(&format!("{}: {error}", t("错误", "Error"))),
            None => view.status(&format!(
                "{} {} · {} PR · {} Issue · {} {}",
                self.repos.len(),
                t("个仓库", "repositories"),
                self.prs.len(),
                self.issues.len(),
                self.unread(),
                t("条未读", "unread")
            )),
        };
        view
    }

    /// The item a command acts on: its argument, else the selected item.
    fn target_key(&self, arg: Option<&str>) -> Option<String> {
        arg.map(str::to_owned).or_else(|| self.selected.clone())
    }

    /// The PR number of `key`, when it names one.
    fn pr_number(key: &str) -> Option<u64> {
        key.strip_prefix("pr:").and_then(|n| n.parse().ok())
    }

    /// The issue number of `key`, when it names one.
    fn issue_number(key: &str) -> Option<&str> {
        key.strip_prefix("issue:")
    }

    /// The pull request a command works on: its argument, the selection, or one
    /// picked from the project's list.
    fn resolve_pr(&mut self, ctx: &Invocation, arg: Option<&str>) -> Result<Option<(RepoRef, u64)>, Error> {
        let repo = current_repo(ctx).ok_or("no Gitee repository in this project")?;
        if let Some(number) = self.target_key(arg).as_deref().and_then(Self::pr_number) {
            return Ok(Some((repo, number)));
        }
        self.ensure_prs(&repo, false)?;
        if self.prs.is_empty() {
            return Err(Error::NotFound(t("这个仓库没有匹配的 Pull Request。", "No pull request matches.")));
        }
        let items: Vec<PickItem> = self
            .prs
            .iter()
            .map(|pr| {
                let label = format!("#{} {}", pr.number, pr.title);
                PickItem::new(&pr.number.to_string(), &label)
            })
            .collect();
        let Some(key) = host::pick(&t("选择 Pull Request", "Pick a pull request"), &items)? else {
            return Ok(None);
        };
        Ok(key.parse().ok().map(|number| (repo, number)))
    }

    /// The issue a command works on: its argument, the selection, or one picked
    /// from the project's list.
    fn resolve_issue(&mut self, ctx: &Invocation, arg: Option<&str>) -> Result<Option<(RepoRef, String)>, Error> {
        let repo = current_repo(ctx).ok_or("no Gitee repository in this project")?;
        if let Some(number) = self.target_key(arg).as_deref().and_then(|k| Self::issue_number(k).map(str::to_owned)) {
            return Ok(Some((repo, number)));
        }
        self.ensure_issues(&repo, false)?;
        if self.issues.is_empty() {
            return Err(Error::NotFound(t("这个仓库没有匹配的 Issue。", "No issue matches.")));
        }
        let items: Vec<PickItem> = self
            .issues
            .iter()
            .map(|issue| {
                let label = format!("#{} {}", issue.number, issue.title);
                PickItem::new(&issue.number, &label)
            })
            .collect();
        let Some(number) = host::pick(&t("选择 Issue", "Pick an issue"), &items)? else {
            return Ok(None);
        };
        Ok(Some((repo, number)))
    }

    /// The repository of `key`: a `repo:` item, a PR/issue of the project, or
    /// the project itself.
    fn repo_of(&self, key: Option<&str>, ctx: &Invocation) -> Option<RepoRef> {
        let key = key.unwrap_or_default();
        if let Some(full) = key.strip_prefix("repo:") {
            return parse_remote(full);
        }
        current_repo(ctx)
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
                "refresh" => self.refresh_all(ctx, true),
                "filter" => {
                    self.pr_filter = self.pr_filter.next();
                    self.prs_at = None;
                    if let Some(repo) = current_repo(ctx) {
                        let _ = self.ensure_prs(&repo, true);
                    }
                }
                "filter-issues" => {
                    self.issue_filter = self.issue_filter.next();
                    self.issues_at = None;
                    if let Some(repo) = current_repo(ctx) {
                        let _ = self.ensure_issues(&repo, true);
                    }
                }
                "new-pr" => {
                    if let Err(e) = self.cmd_create_pr(ctx) {
                        self.notify_error(&e);
                    }
                }
                "clone" => {
                    if let Some(full) = self.selected.as_deref().and_then(|k| k.strip_prefix("repo:")) {
                        let full = full.to_owned();
                        let _ = self.cmd_clone_item(ctx, Some(&full));
                    }
                }
                "commands" => {
                    let _ = self.cmd_commands(ctx);
                }
                _ => {}
            },
            ViewEvent::Select(key) => {
                self.selected = Some(key);
                // Selecting a notification marks it read, like an inbox.
                if let Some(id) = self.selected.as_deref().and_then(|k| k.strip_prefix("notif:")) {
                    let id = id.to_owned();
                    if let Some(note) = self.notifications.iter_mut().find(|n| n.id == id) {
                        note.unread = false;
                    }
                    let _ = api::mark_read(auth_mode(), &id);
                }
            }
            ViewEvent::Activate(key) => {
                self.selected = Some(key.clone());
                if let Some(full) = key.strip_prefix("repo:") {
                    let full = full.to_owned();
                    let _ = self.cmd_clone_item(ctx, Some(&full));
                } else if key.starts_with("pr:") || key.starts_with("issue:") || key.starts_with("notif:") {
                    let _ = self.cmd_detail(ctx, Some(&key));
                }
            }
            _ => {}
        }
        host::refresh_view(VIEW_ID);
        Ok(())
    }
}

// ---------------------------------------------------------------- read commands

impl GiteePlugin {
    fn cmd_prs(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let repo = current_repo(ctx).ok_or("no Gitee repository in this project")?;
        self.prs_at = None;
        self.ensure_prs(&repo, true)?;
        if self.prs.is_empty() {
            return Ok(Some(format!("{}: no {} pull requests.", repo, self.pr_filter.label())));
        }
        let mut out = format!("{} — {} pull requests:\n", repo, self.pr_filter.label());
        for pr in &self.prs {
            let head = pr.head.as_ref().map(|h| h.name().to_owned()).unwrap_or_else(|| "?".into());
            let base = pr.base.as_ref().map(|b| b.name().to_owned()).unwrap_or_else(|| "?".into());
            out.push_str(&format!(
                "  #{:>4} [{}] {} — {}\n    {} ← {}\n    {}\n",
                pr.number,
                pr.status(),
                pr.title,
                pr.user.as_ref().map(|u| u.login.as_str()).unwrap_or("?"),
                base,
                head,
                pr.html_url
            ));
        }
        Ok(Some(out))
    }

    fn cmd_issues(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let repo = current_repo(ctx).ok_or("no Gitee repository in this project")?;
        self.issues_at = None;
        self.ensure_issues(&repo, true)?;
        if self.issues.is_empty() {
            return Ok(Some(format!("{}: no {} issues.", repo, self.issue_filter.label())));
        }
        let mut out = format!("{} — {} issues:\n", repo, self.issue_filter.label());
        for it in &self.issues {
            let labels = it.labels.iter().map(|l| l.0.as_str()).collect::<Vec<_>>().join(", ");
            out.push_str(&format!(
                "  #{:>6} [{}] {} — {}{}\n    {}\n",
                it.number,
                it.state,
                it.title,
                it.user.as_ref().map(|u| u.login.as_str()).unwrap_or("?"),
                if labels.is_empty() { String::new() } else { format!(" ({labels})") },
                it.html_url
            ));
        }
        Ok(Some(out))
    }

    /// Open a pull request, issue or notification as a read-only tab.
    fn cmd_detail(&mut self, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        let key = self.target_key(arg).ok_or("select a pull request, issue or notification first")?;
        let repo = self.repo_of(Some(&key), ctx).ok_or("no Gitee repository in this project")?;

        if let Some(number) = Self::pr_number(&key) {
            let pr = api::get_pr(&repo, number, auth_mode())?;
            let files = api::pr_files(&repo, number, auth_mode()).unwrap_or_default();
            let commits = api::pr_commits(&repo, number, auth_mode()).unwrap_or_default();
            let comments = api::pr_comments(&repo, number, auth_mode()).unwrap_or_default();

            let mut text = format!("# {} #{}\n\n", pr.title, pr.number);
            text.push_str(&format!(
                "- {}: {}\n- {}: {}\n- {}: {}\n- {}: {}\n",
                t("状态", "State"),
                pr.status(),
                t("作者", "Author"),
                pr.user.as_ref().map(|u| u.login.as_str()).unwrap_or("?"),
                t("源分支", "Head"),
                pr.head.as_ref().map(|h| h.name().to_owned()).unwrap_or_else(|| "?".into()),
                t("目标分支", "Base"),
                pr.base.as_ref().map(|b| b.name().to_owned()).unwrap_or_else(|| "?".into()),
            ));
            text.push_str(&format!("- {}: {}\n", t("链接", "URL"), pr.html_url));
            if let Some(created) = &pr.created_at {
                text.push_str(&format!("- {}: {created}\n", t("创建", "Created")));
            }
            if let Some(updated) = &pr.updated_at {
                text.push_str(&format!("- {}: {updated}\n", t("更新", "Updated")));
            }
            if pr.draft == Some(true) {
                text.push_str(&format!("- {}\n", t("草稿", "Draft")));
            }
            if pr.mergeable == Some(false) {
                text.push_str(&format!("- {}\n", t("当前不可合并", "Not mergeable")));
            }
            if let Some(body) = pr.body.as_deref().filter(|b| !b.trim().is_empty()) {
                text.push_str(&format!("\n---\n\n{}\n", html_to_text(body)));
            }
            if !files.is_empty() {
                text.push_str(&format!("\n---\n\n## {} ({})\n\n", t("改动文件", "Changed files"), files.len()));
                for file in &files {
                    text.push_str(&format!("- {} {}\n", file.filename, file.stats()));
                }
            }
            if !commits.is_empty() {
                text.push_str(&format!("\n---\n\n## {} ({})\n\n", t("提交", "Commits"), commits.len()));
                for commit in &commits {
                    let short = &commit.sha[..8.min(commit.sha.len())];
                    let author = commit.author_name();
                    text.push_str(&format!("- {short} — {} ({author})\n", commit.summary()));
                }
            }
            if !comments.is_empty() {
                text.push_str(&format!("\n---\n\n## {} ({})\n\n", t("评论", "Comments"), comments.len()));
                for comment in &comments {
                    text.push_str(&format!(
                        "**{}** {} (#{})\n\n{}\n\n",
                        comment.user.as_ref().map(|u| u.label()).unwrap_or("?"),
                        comment.created_at.as_deref().unwrap_or(""),
                        comment.id,
                        html_to_text(&comment.body)
                    ));
                }
            }
            host::open_content_tab(&ContentTab {
                key: format!("gitee-pr-{number}"),
                title: format!("Gitee PR #{number}"),
                file_name: Some(format!("PR-{number}.md")),
                language: Some("markdown".into()),
                content: text,
                read_only: true,
                actions: Vec::new(),
            })?;
            return Ok(None);
        }

        if let Some(number) = Self::issue_number(&key) {
            let issue = api::get_issue(&repo, number, auth_mode())?;
            let comments = api::issue_comments(&repo, number, auth_mode()).unwrap_or_default();
            let mut text = format!("# {} #{}\n\n", issue.title, issue.number);
            text.push_str(&format!(
                "- {}: {}\n- {}: {}\n",
                t("状态", "State"),
                issue.state,
                t("作者", "Author"),
                issue.user.as_ref().map(|u| u.login.as_str()).unwrap_or("?")
            ));
            if !issue.labels.is_empty() {
                let labels = issue.labels.iter().map(|l| l.0.as_str()).collect::<Vec<_>>().join(", ");
                text.push_str(&format!("- {}: {labels}\n", t("标签", "Labels")));
            }
            text.push_str(&format!("- {}: {}\n", t("链接", "URL"), issue.html_url));
            if let Some(created) = &issue.created_at {
                text.push_str(&format!("- {}: {created}\n", t("创建", "Created")));
            }
            if let Some(updated) = &issue.updated_at {
                text.push_str(&format!("- {}: {updated}\n", t("更新", "Updated")));
            }
            if let Some(count) = issue.comments {
                text.push_str(&format!("- {}: {count}\n", t("评论数", "Comments")));
            }
            if let Some(body) = issue.body.as_deref().filter(|b| !b.trim().is_empty()) {
                text.push_str(&format!("\n---\n\n{}\n", html_to_text(body)));
            }
            if !comments.is_empty() {
                text.push_str(&format!("\n---\n\n## {} ({})\n\n", t("评论", "Comments"), comments.len()));
                for comment in &comments {
                    text.push_str(&format!(
                        "**{}** {} (#{})\n\n{}\n\n",
                        comment.user.as_ref().map(|u| u.label()).unwrap_or("?"),
                        comment.created_at.as_deref().unwrap_or(""),
                        comment.id,
                        html_to_text(&comment.body)
                    ));
                }
            }
            host::open_content_tab(&ContentTab {
                key: format!("gitee-issue-{number}"),
                title: format!("Gitee Issue #{number}"),
                file_name: Some(format!("Issue-{number}.md")),
                language: Some("markdown".into()),
                content: text,
                read_only: true,
                actions: Vec::new(),
            })?;
            return Ok(None);
        }

        if let Some(id) = key.strip_prefix("notif:") {
            let url = self.notifications.iter().find(|n| n.id == id).and_then(|n| n.url.clone());
            return match url {
                Some(url) => {
                    host::open_url(&url)?;
                    Ok(None)
                }
                None => Err(Error::NotFound(format!("notification {id} has no link"))),
            };
        }

        // A repository: show its page.
        if let Some(full) = key.strip_prefix("repo:") {
            let repo = parse_remote(full).ok_or_else(|| Error::Invalid(format!("not a repository: {full}")))?;
            let info = api::get_repository_at(&repo, auth_mode())?;
            let url = if info.html_url.is_empty() { format!("https://gitee.com/{}", repo.slug()) } else { info.html_url };
            host::open_url(&url)?;
            return Ok(None);
        }

        Err(Error::Invalid(format!("nothing to show for {key:?}")))
    }

    /// Show the changed files of a pull request; picking one opens its diff.
    fn cmd_files(&mut self, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        let Some((repo, number)) = self.resolve_pr(ctx, arg)? else {
            return Ok(None);
        };
        let files = api::pr_files(&repo, number, auth_mode())?;
        if files.is_empty() {
            return Ok(Some(t("这个 PR 没有改动文件。", "This pull request changes no files.")));
        }
        let items: Vec<PickItem> = files
            .iter()
            .map(|file| {
                let note = match (&file.patch, file.status.as_deref()) {
                    (Some(_), Some(status)) => format!("{} · {status}", file.stats()),
                    (Some(_), None) => file.stats(),
                    (None, _) => t("无 diff（大文件或二进制）", "no diff (large or binary)").to_owned(),
                };
                let label = if note.is_empty() { file.filename.clone() } else { format!("{}  {note}", file.filename) };
                PickItem::new(&file.filename, &label)
            })
            .collect();
        let Some(chosen) = host::pick(&format!("PR #{number} · {}", t("改动文件", "changed files")), &items)? else {
            return Ok(None);
        };
        let Some(file) = files.iter().find(|f| f.filename == chosen) else {
            return Ok(None);
        };
        match &file.patch {
            Some(patch) => {
                host::open_content_tab(&ContentTab {
                    key: format!("gitee-pr-{number}-{}", file.filename),
                    title: format!("#{number} {}", file.filename),
                    file_name: Some(format!("{}.diff", file.filename.replace('/', "_"))),
                    language: Some("diff".into()),
                    content: patch.clone(),
                    read_only: true,
                    actions: Vec::new(),
                })?;
                Ok(None)
            }
            None => {
                host::notify(
                    NotificationLevel::Warning,
                    "Gitee",
                    &t(
                        "Gitee 没有返回这个文件的 diff（文件过大或为二进制），请在浏览器查看。",
                        "Gitee returned no diff for this file (too large or binary); open it in the browser.",
                    ),
                );
                Ok(None)
            }
        }
    }
}

/// Gitee sends comment and description bodies as HTML; the tabs show text.
fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut chars = html.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '<' => {
                let mut tag = String::new();
                for c in chars.by_ref() {
                    if c == '>' {
                        break;
                    }
                    tag.push(c);
                }
                let name = tag
                    .trim_start_matches('/')
                    .split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .trim_end_matches('/')
                    .to_lowercase();
                if matches!(name.as_str(), "p" | "div" | "br" | "li" | "tr" | "h1" | "h2" | "h3" | "pre") {
                    out.push('\n');
                }
            }
            '&' => {
                let mut entity = String::new();
                for c in chars.by_ref() {
                    if c == ';' {
                        break;
                    }
                    entity.push(c);
                }
                out.push(match entity.as_str() {
                    "amp" => '&',
                    "lt" => '<',
                    "gt" => '>',
                    "quot" => '"',
                    "nbsp" => ' ',
                    _ => ' ',
                });
            }
            other => out.push(other),
        }
    }
    let mut text = String::new();
    let mut blank = 0;
    for line in out.lines() {
        if line.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        text.push_str(line.trim_end());
        text.push('\n');
    }
    text.trim().to_owned()
}

// ---------------------------------------------------------------- write commands

impl GiteePlugin {
    /// Open a pull request from the checked-out branch into a chosen base.
    fn cmd_create_pr(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let repo = current_repo(ctx).ok_or("no Gitee repository in this project")?;
        let info = api::get_repository_at(&repo, auth_mode())?;
        let head = current_branch(ctx).ok_or("the project has no checked-out branch")?;
        let default_base = info.base_branch();

        // The base branch: the repository's branches, defaulting to its own.
        let branches = api::list_branches(&repo, auth_mode(), Page::first(100)).unwrap_or_default();
        let base = if branches.is_empty() {
            host::input(&t("目标分支", "Target branch"), Some(&default_base))?
        } else {
            let items: Vec<PickItem> = branches.iter().map(|b| PickItem::new(&b.name, &b.name)).collect();
            host::pick(&t("目标分支", "Target branch"), &items)?
        };
        let Some(base) = base else { return Ok(None) };
        if base == head {
            return Err(Error::Invalid(t(
                "源分支与目标分支相同，请先切换到一个特性分支。",
                "The source and target branches are the same; switch to a feature branch first.",
            )));
        }

        let Some(title) = host::input(&t("PR 标题", "Pull request title"), None)? else {
            return Ok(None);
        };
        if title.trim().is_empty() {
            return Ok(None);
        }
        let body = host::input_multiline(&t("PR 描述（可留空）", "Pull request description (optional)"), None)?
            .unwrap_or_default();

        if !self.confirm(&t("创建 Pull Request？", "Create the pull request?"), &format!("{repo}\n{head} → {base}"))? {
            return Ok(None);
        }
        let pr = api::create_pr(&repo, &title, &head, &base, &body, None, auth_mode())?;
        self.prs_at = None;
        host::notify(NotificationLevel::Info, "Gitee", &format!("{} #{}", t("已创建", "Created"), pr.number));
        if !pr.html_url.is_empty() {
            host::open_url(&pr.html_url)?;
        }
        Ok(None)
    }

    /// Comment on the selected pull request or issue.
    fn cmd_comment(&mut self, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        let mut key = self.target_key(arg);
        if key.is_none() {
            let pr_label = t("Pull Request", "Pull request");
            let issue_label = t("Issue", "Issue");
            let kinds = [PickItem::new("pr", &pr_label), PickItem::new("issue", &issue_label)];
            match host::pick(&t("评论对象", "Comment on"), &kinds)?.as_deref() {
                Some("pr") => key = self.resolve_pr(ctx, None)?.map(|(_, n)| format!("pr:{n}")),
                Some("issue") => key = self.resolve_issue(ctx, None)?.map(|(_, n)| format!("issue:{n}")),
                _ => return Ok(None),
            }
        }
        let target = match key.as_deref().and_then(Self::pr_number) {
            Some(number) => self.resolve_pr(ctx, key.as_deref())?.map(|(repo, _)| (repo, Some(number), None)),
            None => self
                .resolve_issue(ctx, key.as_deref())?
                .map(|(repo, number)| (repo, None, Some(number))),
        };
        let Some((repo, pr_number, issue_number)) = target else {
            return Ok(None);
        };
        let Some(body) = host::input_multiline(&t("评论内容（支持 Markdown）", "Comment (Markdown)"), None)? else {
            return Ok(None);
        };
        if body.trim().is_empty() {
            return Ok(None);
        }
        if !self.confirm(&t("发布评论？", "Post the comment?"), &body)? {
            return Ok(None);
        }
        if let Some(number) = pr_number {
            api::comment_pr(&repo, number, &body, auth_mode())?;
            host::notify(NotificationLevel::Info, "Gitee", &format!("{} #{number}", t("已评论", "Commented on")));
        } else if let Some(number) = issue_number {
            api::comment_issue(&repo, &number, &body, auth_mode())?;
            host::notify(NotificationLevel::Info, "Gitee", &format!("{} #{number}", t("已评论", "Commented on")));
        }
        Ok(None)
    }

    /// Approve the selected pull request.
    fn cmd_approve(&mut self, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        let Some((repo, number)) = self.resolve_pr(ctx, arg)? else {
            return Ok(None);
        };
        if !self.confirm(&t("通过这个 PR 的审查？", "Approve this pull request?"), &format!("{repo} #{number}"))? {
            return Ok(None);
        }
        api::approve_pr(&repo, number, auth_mode())?;
        self.prs_at = None;
        host::notify(NotificationLevel::Info, "Gitee", &format!("{} #{number}", t("已通过审查", "Approved")));
        Ok(None)
    }

    /// Merge the selected pull request, asking for the method.
    fn cmd_merge(&mut self, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        let Some((repo, number)) = self.resolve_pr(ctx, arg)? else {
            return Ok(None);
        };
        let pr = api::get_pr(&repo, number, auth_mode())?;
        if pr.status() == "merged" {
            return Err(Error::Invalid(t("这个 PR 已经合并了。", "This pull request is already merged.")));
        }

        let methods: Vec<PickItem> = MergeMethod::all()
            .iter()
            .map(|m| {
                let id = format!("{m:?}").to_lowercase();
                let label = format!("{m:?}");
                PickItem::new(&id, &label)
            })
            .collect();
        let Some(chosen) = host::pick(&format!("PR #{number} · {}", t("合并方式", "merge method")), &methods)? else {
            return Ok(None);
        };
        let method = MergeMethod::all()
            .into_iter()
            .find(|m| format!("{m:?}").to_lowercase() == chosen)
            .unwrap_or(MergeMethod::Merge);

        let close = host::confirm(
            &t("合并后关闭关联 Issue？", "Close related issues after merging?"),
            &t("Gitee 会关闭这个 PR 提到的 Issue。", "Gitee closes the issues this pull request mentions."),
        )?;
        let prune = host::confirm(
            &t("合并后删除源分支？", "Delete the source branch after merging?"),
            pr.head.as_ref().map(|h| h.name().to_owned()).unwrap_or_default().as_str(),
        )?;
        if !self.confirm(
            &format!("{} #{number}?", t("合并", "Merge"), number = number),
            &format!("{repo}\n#{} {}\n{:?}", pr.number, pr.title, method),
        )? {
            return Ok(None);
        }
        api::merge_pr(&repo, number, method, prune, close, auth_mode())?;
        self.prs_at = None;
        host::notify(NotificationLevel::Info, "Gitee", &format!("{} #{number}", t("已合并", "Merged")));
        Ok(None)
    }

    /// Change the selected issue's state.
    fn cmd_issue_state(&mut self, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        let Some((repo, number)) = self.resolve_issue(ctx, arg)? else {
            return Ok(None);
        };
        let states = [
            ("closed", t("关闭", "Close")),
            ("open", t("重新打开", "Reopen")),
            ("progressing", t("标记为进行中", "Mark in progress")),
            ("rejected", t("标记为拒绝", "Mark rejected")),
        ];
        let items: Vec<PickItem> = states.iter().map(|(id, label)| PickItem::new(id, label)).collect();
        let Some(state) = host::pick(&format!("Issue #{number} · {}", t("新状态", "new state")), &items)? else {
            return Ok(None);
        };
        api::set_issue_state(&repo, &number, &state, auth_mode())?;
        self.issues_at = None;
        host::notify(NotificationLevel::Info, "Gitee", &format!("{} #{number} → {state}", t("已更新", "Updated")));
        Ok(None)
    }

    /// Create an issue in the project's repository.
    fn cmd_issue_create(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let repo = current_repo(ctx).ok_or("no Gitee repository in this project")?;
        let Some(title) = host::input(&t("Issue 标题", "Issue title"), None)? else { return Ok(None) };
        if title.trim().is_empty() {
            return Ok(None);
        }
        let body = host::input_multiline(&t("Issue 描述（可留空）", "Issue description (optional)"), None)?
            .unwrap_or_default();
        let labels = self.pick_labels(&repo)?;
        if !self.confirm(&t("创建 Issue？", "Create the issue?"), &format!("{repo}\n{title}"))? {
            return Ok(None);
        }
        let issue = api::create_issue(&repo, &title, &body, &labels, None, auth_mode())?;
        self.issues_at = None;
        host::notify(
            NotificationLevel::Info,
            "Gitee",
            &format!("{} #{}", t("已创建 Issue", "Created issue"), issue.number),
        );
        if !issue.html_url.is_empty() {
            host::open_url(&issue.html_url)?;
        }
        Ok(None)
    }

    /// Labels to attach, from the repository's own list.
    fn pick_labels(&self, repo: &RepoRef) -> Result<Vec<String>, Error> {
        let Ok(labels) = api::list_labels(repo, auth_mode()) else {
            return Ok(Vec::new());
        };
        if labels.is_empty() {
            return Ok(Vec::new());
        }
        let items: Vec<PickItem> = labels.iter().map(|l| PickItem::new(&l.name, &l.name)).collect();
        Ok(host::pick_many(&t("标签（可多选）", "Labels (pick any)"), &items)?.unwrap_or_default())
    }

    /// Create a repository under the user's account.
    fn cmd_create_repo(&mut self, _ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        let name = match arg.map(str::trim).filter(|a| !a.is_empty()) {
            Some(name) => name.to_owned(),
            None => match host::input(&t("仓库名", "Repository name"), None)? {
                Some(name) if !name.trim().is_empty() => name,
                _ => return Ok(None),
            },
        };
        let private = host::confirm(
            &t("创建为私有仓库？", "Create it private?"),
            &t("取消则创建公开仓库。", "Cancel creates a public repository."),
        )?;
        if !self.confirm(&t("创建仓库？", "Create the repository?"), &format!("{name} (private: {private})"))? {
            return Ok(None);
        }
        let repo = api::create_repository(&name, "", private, None, auth_mode())?;
        self.repos_at = None;
        host::notify(NotificationLevel::Info, "Gitee", &format!("{} {}", t("已创建仓库", "Created"), repo.full_name));
        Ok(Some(format!("Created {} — {}", repo.full_name, repo.html_url)))
    }

    /// Clone the repository of a `repo:` item (or the given one).
    fn cmd_clone_item(&self, _ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        let Some(full) = arg.map(str::to_owned).or_else(|| self.selected.clone()).filter(|k| k.starts_with("repo:"))
        else {
            return Err(Error::Invalid(t("请选择一个仓库。", "Select a repository first.").to_owned()));
        };
        let remote = full.trim_start_matches("repo:");
        self.clone_remote(remote)
    }

    /// Clone a Gitee repository: verify it, choose a directory, `git clone`,
    /// then open the result as a project.
    fn clone_remote(&self, remote: &str) -> Result<Option<String>, Error> {
        let repo = parse_remote(remote).ok_or_else(|| Error::Invalid(format!("not a Gitee remote: {remote}")))?;
        let info = api::get_repository_at(&repo, auth_mode())?;
        let url = if !info.clone_url.is_empty() {
            info.clone_url
        } else if !info.ssh_url.is_empty() {
            info.ssh_url
        } else {
            format!("https://gitee.com/{}.git", repo.slug())
        };
        let target = host::pick_directory(&t("克隆到…", "Clone into…"), None)?;
        let Some(target) = target else { return Ok(None) };
        let dest = format!("{}/{}", target.trim_end_matches(['/', '\\']), repo.repo);
        // Cloning takes a while: show it in the status bar (and allow 60 s calls).
        let task = host::begin_task(&format!("{} {}", t("克隆", "Cloning"), repo), false);
        task.report(0, None, Some(&t("正在克隆…", "Cloning…")));
        let out = host::run_process("git", &["clone".to_owned(), url, dest.clone()], None)?;
        if out.status != Some(0) {
            let msg = if out.stderr.is_empty() { out.stdout } else { out.stderr };
            return Err(Error::Failed(msg));
        }
        task.report(1, Some(1), Some(&t("已克隆", "Cloned")));
        drop(task);
        host::open_project(&dest, false)?;
        host::notify(NotificationLevel::Info, "Gitee", &format!("{} {} → {}", t("已克隆", "Cloned"), repo, dest));
        Ok(Some(dest))
    }

    /// Clone a repository picked from the token's list (or typed).
    fn cmd_clone(&mut self, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        let remote = match arg.map(str::trim).filter(|a| !a.is_empty()) {
            Some(a) => a.to_owned(),
            None => {
                let _ = self.ensure_repos(false);
                let repos = self.repos.clone();
                if repos.is_empty() {
                    match host::input(&t("Gitee 仓库（owner/repo 或 URL）", "Gitee repository (owner/repo or URL)"), None)? {
                        Some(input) => input,
                        None => return Ok(None),
                    }
                } else {
                    let items: Vec<PickItem> =
                        repos.iter().map(|r| PickItem::new(&r.full_name, &r.full_name)).collect();
                    match host::pick(&t("克隆 Gitee 仓库", "Clone a Gitee repository"), &items)? {
                        Some(key) => key,
                        None => return Ok(None),
                    }
                }
            }
        };
        let _ = ctx;
        self.clone_remote(&remote)
    }

    /// Push the current branch to its upstream via `git`.
    fn cmd_push(&self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let project = ctx.project().ok_or("no project open")?;
        let out = project.run_process("git", &["push".to_owned()], None)?;
        if out.status != Some(0) {
            return Err(Error::Failed(if out.stderr.is_empty() { out.stdout } else { out.stderr }));
        }
        Ok(Some(t("已推送到远程。", "Pushed to remote.")))
    }

    /// Fetch the inbox and show it in the panel.
    fn cmd_notifications(&mut self, _ctx: &Invocation) -> Result<Option<String>, Error> {
        self.notifications_at = None;
        self.ensure_notifications(true)?;
        host::reveal_view(VIEW_ID)?;
        let unread = self.unread();
        Ok(Some(format!(
            "{} {} · {} {}",
            self.notifications.len(),
            t("条通知", "notifications"),
            unread,
            t("条未读", "unread")
        )))
    }

    /// Open the item's page in the browser.
    fn cmd_open_item(&mut self, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        let key = self.target_key(arg).ok_or("select something first")?;
        let repo = self.repo_of(Some(&key), ctx);
        let url = if let Some(number) = Self::pr_number(&key) {
            match repo {
                Some(repo) => api::get_pr(&repo, number, auth_mode())?.html_url,
                None => return Err(Error::Invalid("no Gitee repository in this project".into())),
            }
        } else if let Some(number) = Self::issue_number(&key) {
            match repo {
                Some(repo) => api::get_issue(&repo, number, auth_mode())?.html_url,
                None => return Err(Error::Invalid("no Gitee repository in this project".into())),
            }
        } else if let Some(full) = key.strip_prefix("repo:") {
            format!("https://gitee.com/{full}")
        } else if let Some(id) = key.strip_prefix("notif:") {
            self.notifications.iter().find(|n| n.id == id).and_then(|n| n.url.clone()).unwrap_or_default()
        } else {
            String::new()
        };
        if url.is_empty() {
            return Err(Error::Invalid(format!("no link for {key:?}")));
        }
        host::open_url(&url)?;
        Ok(None)
    }

    /// Open the focused file's page on Gitee at the current branch.
    fn cmd_open_file(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let repo = current_repo(ctx).ok_or("no Gitee repository in this project")?;
        let relative = editor_relative_path(ctx).ok_or("the focused file is not inside the project")?;
        let info = api::get_repository_at(&repo, auth_mode())?;
        let branch = current_branch(ctx).unwrap_or_else(|| info.base_branch());
        let base = if info.html_url.is_empty() {
            format!("https://gitee.com/{}", repo.slug())
        } else {
            info.html_url.trim_end_matches('/').to_owned()
        };
        let url = format!("{base}/blob/{}/{relative}", api::enc(&branch));
        host::open_url(&url)?;
        Ok(None)
    }

    /// Put a Gitee issue reference into the commit message being prepared.
    fn cmd_commit_message(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let Some(draft) = host::pending_commit()? else {
            return Err(Error::NotFound(t("当前没有待提交的更改。", "There is no commit being prepared.")));
        };
        let repo = current_repo(ctx);
        let issues = match &repo {
            Some(repo) => {
                self.ensure_issues(repo, false)?;
                self.issues.clone()
            }
            None => Vec::new(),
        };
        if issues.is_empty() {
            let number = host::input(&t("Issue 编号", "Issue number"), None)?
                .ok_or_else(|| Error::Invalid("no issue".into()))?;
            return self.apply_issue_to_message(&draft, &number);
        }
        let items: Vec<PickItem> = issues
            .iter()
            .map(|i| {
                let label = format!("#{} {}", i.number, i.title);
                PickItem::new(&i.number, &label)
            })
            .collect();
        let Some(number) = host::pick(&t("引用 Issue", "Reference an issue"), &items)? else {
            return Ok(None);
        };
        self.apply_issue_to_message(&draft, &number)
    }

    fn apply_issue_to_message(&self, draft: &CommitDraft, number: &str) -> Result<Option<String>, Error> {
        let reference = format!("#{number}");
        let message = if draft.message.contains(&reference) {
            draft.message.clone()
        } else if draft.message.trim().is_empty() {
            format!("{reference} ")
        } else {
            format!("{}\n\n{reference}", draft.message.trim_end())
        };
        host::set_commit_message(draft.revision, &message)?;
        Ok(Some(t("已把 Issue 引用写入提交信息。", "Added the issue reference to the commit message.")))
    }

    /// Every Gitee command, as a picker: the panel's "commands" button and the
    /// status bar menu. It is the one place to see what the plugin can do.
    fn cmd_commands(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let items: Vec<PickItem> = COMMANDS
            .iter()
            .map(|(id, zh, en)| {
                let label = t(zh, en);
                PickItem::new(id, &label).description(id)
            })
            .collect();
        let Some(id) = host::pick(&t("Gitee 命令", "Gitee commands"), &items)? else {
            return Ok(None);
        };
        let id = id.to_owned();
        self.run_command(&id, ctx, None)
    }
}

/// The commands the picker lists: id, Chinese label, English label.
const COMMANDS: &[(&str, &str, &str)] = &[
    ("show-panel", "打开面板", "Open panel"),
    ("detail", "查看详情", "Show details"),
    ("files", "查看改动文件…", "Changed files…"),
    ("prs", "列出 PR", "List pull requests"),
    ("issues", "列出 Issue", "List issues"),
    ("create-pr", "新建 PR…", "New pull request…"),
    ("comment", "评论…", "Comment…"),
    ("approve", "通过审查", "Approve"),
    ("merge", "合并 PR…", "Merge…"),
    ("issue-create", "新建 Issue…", "New issue…"),
    ("issue-state", "改 Issue 状态…", "Change issue state…"),
    ("notifications", "通知", "Notifications"),
    ("notifications-read", "通知：全部已读", "Mark all read"),
    ("clone", "克隆仓库…", "Clone repository…"),
    ("clone-item", "克隆选中仓库", "Clone selected repository"),
    ("create-repo", "创建仓库…", "Create repository…"),
    ("push", "推送到远程", "Push"),
    ("open-item", "浏览器打开", "Open in browser"),
    ("open-file", "打开当前文件", "Open this file"),
    ("commit-message", "引用 Issue 到提交…", "Issue in commit message…"),
    ("refresh", "刷新", "Refresh"),
];

impl Plugin for GiteePlugin {
    fn run_command(&mut self, id: &str, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        match id {
            "show-panel" => {
                host::reveal_view(VIEW_ID)?;
                Ok(None)
            }
            "commands" => self.cmd_commands(ctx),
            "prs" => self.cmd_prs(ctx),
            "issues" => self.cmd_issues(ctx),
            "notifications" => self.cmd_notifications(ctx),
            "notifications-read" => {
                api::mark_all_read(auth_mode())?;
                for note in &mut self.notifications {
                    note.unread = false;
                }
                self.notifications_at = Some(Instant::now());
                host::refresh_view(VIEW_ID);
                Ok(Some(t("已全部标记为已读。", "Marked every notification read.")))
            }
            "create-pr" => self.cmd_create_pr(ctx),
            "issue-create" => self.cmd_issue_create(ctx),
            "issue-state" => self.cmd_issue_state(ctx, arg),
            "create-repo" => self.cmd_create_repo(ctx, arg),
            "clone" => self.cmd_clone(ctx, arg),
            "clone-item" => self.cmd_clone_item(ctx, arg),
            "push" => self.cmd_push(ctx),
            "detail" => self.cmd_detail(ctx, arg),
            "files" => self.cmd_files(ctx, arg),
            "comment" => self.cmd_comment(ctx, arg),
            "approve" => self.cmd_approve(ctx, arg),
            "merge" => self.cmd_merge(ctx, arg),
            "open-item" => self.cmd_open_item(ctx, arg),
            "open-file" => self.cmd_open_file(ctx),
            "commit-message" => self.cmd_commit_message(ctx),
            "refresh" => {
                self.invalidate();
                self.refresh_all(ctx, true);
                self.show_status(ctx);
                host::refresh_view(VIEW_ID);
                Ok(None)
            }
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }

    fn handle_events(&mut self, events: &[Event], ctx: &Invocation) {
        // On a schedule and on VCS changes: refetch when the panel is next
        // shown, and keep the status bar current.
        if events.iter().any(|e| matches!(e, Event::Scheduled | Event::VcsChanged(_))) {
            self.invalidate();
            if has_token() {
                if let Some(repo) = current_repo(ctx) {
                    let _ = self.ensure_prs(&repo, true);
                }
                let _ = self.ensure_notifications(true);
            }
            self.show_status(ctx);
        }
    }
}

// ---------------------------------------------------------------- commit check

/// Whether a commit message names an issue (`#12`, `#ICK440`).
fn references_issue(message: &str) -> bool {
    let mut chars = message.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '#' && chars.peek().is_some_and(|n| n.is_alphanumeric()) {
            return true;
        }
    }
    false
}

impl CommitChecks for GiteePlugin {
    fn check(&mut self, check: &str, draft: &CommitDraft, _ctx: &Invocation) -> Result<Vec<CommitProblem>, Error> {
        if check != COMMIT_CHECK || !setting_bool("requireIssueInCommit", false) {
            return Ok(Vec::new());
        }
        if references_issue(&draft.message) {
            return Ok(Vec::new());
        }
        Ok(vec![CommitProblem::new(t(
            "提交信息里没有引用 Gitee Issue（例如 #12）。",
            "The commit message names no Gitee issue (e.g. #12).",
        ))])
    }
}

// ---------------------------------------------------------------- MCP tools

impl McpProvider for GiteePlugin {
    fn call(&mut self, tool: &str, arguments: &str, ctx: &Invocation) -> Result<String, Error> {
        let args: serde_json::Value = if arguments.trim().is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_str(arguments).map_err(|e| Error::Invalid(format!("arguments: {e}")))?
        };
        let repo = mcp_repo(&args, ctx)?;
        match tool {
            "gitee_list_prs" => {
                let filter = match args.get("state").and_then(|s| s.as_str()).unwrap_or("open") {
                    "merged" => PrFilter::Merged,
                    "closed" => PrFilter::Closed,
                    "all" => PrFilter::All,
                    _ => PrFilter::Open,
                };
                let limit = args.get("limit").and_then(|l| l.as_u64()).unwrap_or(20).clamp(1, 100) as u32;
                let prs = api::list_prs(&repo, filter, auth_mode(), Page::first(limit))?;
                Ok(to_json(&prs.iter().map(pr_json).collect::<Vec<_>>()))
            }
            "gitee_pr_detail" => {
                let number = args
                    .get("number")
                    .and_then(|n| n.as_u64())
                    .ok_or_else(|| Error::Invalid("number is required".into()))?;
                let pr = api::get_pr(&repo, number, auth_mode())?;
                let mut value = pr_json(&pr);
                if args.get("files").and_then(|f| f.as_bool()).unwrap_or(true) {
                    let files = api::pr_files(&repo, number, auth_mode()).unwrap_or_default();
                    let files: Vec<serde_json::Value> = files
                        .iter()
                        .map(|f| {
                            serde_json::json!({
                                "filename": f.filename,
                                "status": f.status,
                                "additions": f.additions,
                                "deletions": f.deletions,
                                "has_patch": f.patch.is_some(),
                            })
                        })
                        .collect();
                    value["files"] = serde_json::Value::Array(files);
                }
                Ok(value.to_string())
            }
            "gitee_list_issues" => {
                let filter = match args.get("state").and_then(|s| s.as_str()).unwrap_or("open") {
                    "closed" => IssueFilter::Closed,
                    "all" => IssueFilter::All,
                    _ => IssueFilter::Open,
                };
                let limit = args.get("limit").and_then(|l| l.as_u64()).unwrap_or(20).clamp(1, 100) as u32;
                let issues = api::list_issues(&repo, filter, auth_mode(), Page::first(limit))?;
                let values: Vec<serde_json::Value> = issues
                    .iter()
                    .map(|i| {
                        serde_json::json!({
                            "number": i.number,
                            "title": i.title,
                            "state": i.state,
                            "url": i.html_url,
                            "labels": i.labels.iter().map(|l| l.0.clone()).collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                Ok(to_json(&values))
            }
            "gitee_create_issue" => {
                let title = args
                    .get("title")
                    .and_then(|t| t.as_str())
                    .ok_or_else(|| Error::Invalid("title is required".into()))?;
                let body = args.get("body").and_then(|b| b.as_str()).unwrap_or_default();
                let labels: Vec<String> = args
                    .get("labels")
                    .and_then(|l| l.as_array())
                    .map(|l| l.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
                    .unwrap_or_default();
                let assignee = args.get("assignee").and_then(|a| a.as_str());
                let issue = api::create_issue(&repo, title, body, &labels, assignee, auth_mode())?;
                Ok(serde_json::json!({
                    "number": issue.number,
                    "title": issue.title,
                    "url": issue.html_url,
                })
                .to_string())
            }
            _ => Err(Error::NotFound(format!("tool {tool}"))),
        }
    }
}

fn mcp_repo(args: &serde_json::Value, ctx: &Invocation) -> Result<RepoRef, Error> {
    if let Some(name) = args.get("repo").and_then(|r| r.as_str()).filter(|r| !r.trim().is_empty()) {
        return parse_remote(name).ok_or_else(|| Error::Invalid(format!("not a Gitee repository: {name}")));
    }
    current_repo(ctx).ok_or_else(|| {
        Error::Invalid("no repository: pass repo=owner/name or open a project with a Gitee remote".into())
    })
}

fn pr_json(pr: &PullRequest) -> serde_json::Value {
    serde_json::json!({
        "number": pr.number,
        "title": pr.title,
        "state": pr.status(),
        "author": pr.user.as_ref().map(|u| u.login.clone()),
        "head": pr.head.as_ref().map(|h| h.name().to_owned()),
        "base": pr.base.as_ref().map(|b| b.name().to_owned()),
        "url": pr.html_url,
        "body": pr.body,
        "labels": pr.labels.iter().map(|l| l.0.clone()).collect::<Vec<_>>(),
        "updated_at": pr.updated_at,
    })
}

fn to_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "[]".into())
}

// ---------------------------------------------------------------- search

impl SearchProvider for GiteePlugin {
    fn search(&mut self, _category: &str, query: &str, limit: u32, ctx: &Invocation) -> Result<SearchPage, Error> {
        let needle = query.trim().to_lowercase();
        let mut items: Vec<SearchItem> = Vec::new();

        // What the plugin can do, when nothing is filtered out yet.
        if needle.is_empty() {
            for (id, zh, en) in COMMANDS.iter().take(6) {
                items.push(SearchItem {
                    id: format!("cmd:{id}"),
                    title: t(zh, en),
                    description: None,
                    target: SearchTarget::Command { id: (*id).into(), arg: None },
                });
            }
            return Ok(SearchPage { items, has_more: false });
        }

        // Cached data only: the search runs on every keystroke.
        let matches = |s: &str| s.to_lowercase().contains(&needle);

        for repo in &self.repos {
            if matches(&repo.full_name) || repo.description.as_deref().is_some_and(matches) {
                items.push(SearchItem {
                    id: format!("repo:{}", repo.full_name),
                    title: repo.full_name.clone(),
                    description: repo.description.clone(),
                    target: SearchTarget::Command {
                        id: "open-item".into(),
                        arg: Some(format!("repo:{}", repo.full_name)),
                    },
                });
            }
        }
        for pr in &self.prs {
            if matches(&pr.title) {
                items.push(SearchItem {
                    id: format!("pr:{}", pr.number),
                    title: format!("#{} {}", pr.number, pr.title),
                    description: Some(format!("PR · {}", pr.status())),
                    target: SearchTarget::Command { id: "detail".into(), arg: Some(format!("pr:{}", pr.number)) },
                });
            }
        }
        for issue in &self.issues {
            if matches(&issue.title) || matches(&issue.number) {
                items.push(SearchItem {
                    id: format!("issue:{}", issue.number),
                    title: format!("#{} {}", issue.number, issue.title),
                    description: Some(format!("Issue · {}", issue.state)),
                    target: SearchTarget::Command {
                        id: "detail".into(),
                        arg: Some(format!("issue:{}", issue.number)),
                    },
                });
            }
        }
        let _ = ctx;
        let has_more = items.len() > limit as usize;
        items.truncate(limit as usize);
        Ok(SearchPage { items, has_more })
    }
}

register_plugin!(GiteePlugin, views, commit_checks, search, mcp);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    /// A host granting what the panel reads, with a token and HTTP answers.
    fn panel_host() -> MockHost {
        let host = MockHost::install()
            .with_capabilities(&["secrets", "net:gitee.com", "project.read", "vcs.read", "events"]);
        host.set_secret("gitee-token", "test-token");
        host.on_fetch(|req| {
            let body = if req.url.contains("/notifications/threads") {
                r#"[{"id":1,"unread":true,"subject":{"title":"New PR","type":"PullRequest"},"repository":{"full_name":"acme/widgets"}}]"#
            } else if req.url.contains("/user/repos") {
                r#"[{"full_name":"acme/widgets","clone_url":"https://gitee.com/acme/widgets.git","private":false}]"#
            } else {
                "[]"
            };
            crate::api::json(200, body)
        });
        host
    }

    #[test]
    fn auth_mode_defaults_to_token() {
        let _host = MockHost::install();
        assert_eq!(auth_mode(), AuthMode::Token);
    }

    #[test]
    fn panel_asks_for_a_token_before_anything_else() {
        let host = MockHost::install();
        let ctx = host.invocation();
        let mut plugin = GiteePlugin::default();
        let view = plugin.render(VIEW_ID, None, &ctx).unwrap();
        match view.body {
            Body::Message(text) => assert!(text.contains("令牌") || text.contains("token"), "{text}"),
            other => panic!("expected a message body, got {other:?}"),
        }
        assert!(host.requests().is_empty(), "nothing may be fetched without a token");
    }

    #[test]
    fn panel_shows_the_inbox_and_the_repositories() {
        let host = panel_host();
        let ctx = host.invocation();
        let mut plugin = GiteePlugin::default();
        let view = plugin.render(VIEW_ID, None, &ctx).unwrap();
        match view.body {
            Body::Tree(items) => {
                let keys: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
                assert!(keys.contains(&"notifs"), "{keys:?}");
                assert!(keys.contains(&"notif:1"), "{keys:?}");
                assert!(keys.contains(&"repo:acme/widgets"), "{keys:?}");
            }
            other => panic!("expected a tree body, got {other:?}"),
        }
        // The second render reuses the cache: no further requests.
        let before = host.requests().len();
        let _ = plugin.render(VIEW_ID, None, &ctx).unwrap();
        assert_eq!(host.requests().len(), before, "the cache must hold within its TTL");
    }

    #[test]
    fn the_filter_button_cycles_the_pull_request_state() {
        let host = panel_host();
        let ctx = host.invocation();
        let mut plugin = GiteePlugin::default();
        assert_eq!(plugin.pr_filter, PrFilter::Open);
        plugin.handle(VIEW_ID, ViewEvent::Button("filter".into()), &ctx).unwrap();
        assert_eq!(plugin.pr_filter, PrFilter::Merged);
        plugin.handle(VIEW_ID, ViewEvent::Button("filter".into()), &ctx).unwrap();
        assert_eq!(plugin.pr_filter, PrFilter::Closed);
        plugin.handle(VIEW_ID, ViewEvent::Button("filter-issues".into()), &ctx).unwrap();
        assert_eq!(plugin.issue_filter, IssueFilter::Closed);
    }

    #[test]
    fn selecting_a_notification_marks_it_read() {
        let host = panel_host();
        let ctx = host.invocation();
        let mut plugin = GiteePlugin::default();
        let _ = plugin.render(VIEW_ID, None, &ctx).unwrap();
        assert_eq!(plugin.unread(), 1);
        plugin.handle(VIEW_ID, ViewEvent::Select("notif:1".into()), &ctx).unwrap();
        assert_eq!(plugin.unread(), 0);
        assert!(host.requests().iter().any(|r| r.method == "PATCH" && r.url.ends_with("/threads/1")));
    }

    #[test]
    fn item_keys_map_to_numbers() {
        assert_eq!(GiteePlugin::pr_number("pr:12"), Some(12));
        assert_eq!(GiteePlugin::pr_number("issue:12"), None);
        assert_eq!(GiteePlugin::issue_number("issue:ICK440"), Some("ICK440"));
        assert_eq!(GiteePlugin::issue_number("pr:1"), None);
    }

    #[test]
    fn commit_messages_are_checked_for_an_issue_reference() {
        assert!(references_issue("fix: something (#12)"));
        assert!(references_issue("#ICK440 retry"));
        assert!(!references_issue("fix: something"));
        assert!(!references_issue("a # b"));

        let host = panel_host();
        let ctx = host.invocation();
        let mut plugin = GiteePlugin::default();
        let draft = |message: &str| CommitDraft {
            revision: 1,
            message: message.into(),
            amend: false,
            changes: Vec::new(),
        };
        // The check is off by default.
        assert!(plugin.check(COMMIT_CHECK, &draft("no reference"), &ctx).unwrap().is_empty());
    }

    #[test]
    fn html_becomes_readable_text() {
        let text = html_to_text("<p>Hello&nbsp;<b>world</b></p><p>second</p>");
        assert!(text.contains("Hello world"), "{text}");
        assert!(text.contains("second"), "{text}");
        assert!(!text.contains('<'), "{text}");
    }
}
