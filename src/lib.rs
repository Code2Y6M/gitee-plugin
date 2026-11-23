//! Gitee integration for OpenIDEA.
//!
//! A plugin that:
//! - shows the current project's Gitee remote and open PR count in the status bar,
//! - lists pull requests and issues,
//! - creates a remote Gitee repository,
//! - clones a Gitee repository and pushes the current branch (via `git`).
//!
//! Authentication: personal access token (default) or OAuth2, configured via
//! the settings page; the token is kept in the IDE secret store.

mod api;

use openidea_plugin::{
    Event, Invocation, PickItem, Plugin, StatusItem, host, register_plugin,
};
use api::{AuthMode, RepoRef, parse_remote};

/// Status bar item id.
const STATUS_ITEM: &str = "gitee";

#[derive(Default)]
pub struct GiteePlugin;

/// Read the user's chosen auth mode from settings (defaults to token).
fn auth_mode() -> AuthMode {
    match host::setting_string("authMode").as_deref() {
        Some("oauth") => AuthMode::OAuth,
        _ => AuthMode::Token,
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
            Err(openidea_plugin::Error::Denied(_)) => {
                format!("Gitee: {} · no token", repo)
            }
            Err(_) => format!("Gitee: {}", repo),
        };
        host::set_status_item(
            STATUS_ITEM,
            &StatusItem {
                text,
                tooltip: Some(format!(
                    "Gitee repository {}\nClick to list pull requests.",
                    repo
                )),
                command: Some("prs".into()),
            },
        );
    }

    fn cmd_prs(&self, ctx: &Invocation) -> Result<Option<String>, openidea_plugin::Error> {
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

    fn cmd_issues(&self, ctx: &Invocation) -> Result<Option<String>, openidea_plugin::Error> {
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
    ) -> Result<Option<String>, openidea_plugin::Error> {
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
    ) -> Result<Option<String>, openidea_plugin::Error> {
        // Resolve the remote: an explicit argument wins; otherwise ask the
        // user — entering `owner/repo` or a URL, or picking from their repos.
        let remote: String = match arg {
            Some(a) if !a.trim().is_empty() => a.to_owned(),
            _ => match self.pick_repository()? {
                Some(r) => r.full_name.clone(),
                None => return Ok(None), // user cancelled
            },
        };

        let repo = parse_remote(&remote)
            .ok_or_else(|| openidea_plugin::Error::Invalid(format!("not a Gitee remote: {remote}")))?;

        // Verify the repository exists and is reachable before cloning.
        let info = api::get_repository(&remote, auth_mode())?;
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
        let out = host::run_process(
            "git",
            &["clone".to_owned(), url, dest.clone()],
            None,
        )?;
        if out.status.map_or(true, |s| s != 0) {
            let msg = if out.stderr.is_empty() { out.stdout } else { out.stderr };
            return Err(openidea_plugin::Error::Failed(msg));
        }

        // Open the freshly cloned project in the IDE.
        host::open_project(&dest, false)?;
        host::notify(
            openidea_plugin::NotificationLevel::Info,
            "Gitee",
            &format!("Cloned {} into {}", repo, dest),
        );
        Ok(None)
    }

    /// Ask the user to pick one of their Gitee repositories, falling back to
    /// manual `owner/repo` input when the list is empty or unavailable.
    /// Returns `Ok(None)` when the user cancels.
    fn pick_repository(&self) -> Result<Option<api::Repository>, openidea_plugin::Error> {
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
    fn cmd_push(&self, ctx: &Invocation) -> Result<Option<String>, openidea_plugin::Error> {
        let project = ctx.project().ok_or("no project open")?;
        let out = project.run_process("git", &["push".to_owned()], None)?;
        if out.status.map_or(true, |s| s != 0) {
            return Err(openidea_plugin::Error::Failed(if out.stderr.is_empty() {
                out.stdout
            } else {
                out.stderr
            }));
        }
        Ok(Some("Pushed to remote.".to_owned()))
    }
}

impl Plugin for GiteePlugin {
    fn run_command(
        &mut self,
        id: &str,
        ctx: &Invocation,
        arg: Option<&str>,
    ) -> Result<Option<String>, openidea_plugin::Error> {
        match id {
            "prs" => self.cmd_prs(ctx),
            "issues" => self.cmd_issues(ctx),
            "create-repo" => self.cmd_create_repo(ctx, arg),
            "clone" => self.cmd_clone(ctx, arg),
            "push" => self.cmd_push(ctx),
            "refresh" => {
                self.show_status(ctx);
                Ok(None)
            }
            _ => Err(openidea_plugin::Error::NotFound(format!("command {id}"))),
        }
    }

    fn handle_events(&mut self, events: &[Event], ctx: &Invocation) {
        // Refresh the status bar on schedule and on VCS changes.
        if events
            .iter()
            .any(|e| matches!(e, Event::Scheduled | Event::VcsChanged(_)))
        {
            self.show_status(ctx);
        }
    }
}

register_plugin!(GiteePlugin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_mode_defaults_to_token() {
        // No settings in a fresh MockHost: falls back to token.
        let host = openidea_plugin::mock::MockHost::install();
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
}
