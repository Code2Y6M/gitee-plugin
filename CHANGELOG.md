# Changelog

All notable changes to the Gitee plugin. Versions follow [Semantic Versioning](https://semver.org/):
a new feature bumps the minor version, a fix the patch version.

## 1.0.0

The first release meant for daily use: the plugin now covers reading, writing and
handing Gitee to the IDE's agent.

### Added

- **Gitee tool window** (`工具 › Gitee: 打开仓库面板`, the status bar item, the
  command palette): notifications, the project's pull requests and issues, and
  every repository the token can see.
- **Notifications inbox**: unread first, selecting one marks it read
  (`PATCH /notifications/threads/{id}`), plus "mark all read".
- **Pull request details** as a read-only tab: state, author, branches, dates,
  description, changed files, commits, comments.
- **Changed files**: pick a file and read its patch with the IDE's diff syntax
  highlighting (Gitee returns up to 300 diffs; large files have none).
- **Issue details**: description, labels, dates and comments.
- **Writing**: create a pull request from the current branch, comment, approve,
  merge (merge / squash / rebase, optional source-branch deletion and related
  issue closing), create an issue with labels, close/reopen/mark an issue.
- **Commit message toolbar**: put an issue reference (`#12`) into the commit
  being prepared, without overwriting text typed meanwhile.
- **Commit check**: optionally warn when a commit message names no issue.
- **Search Everywhere category** (`Gitee`) over the loaded repositories, pull
  requests and issues.
- **MCP tools** for the AI agent: `gitee_list_prs`, `gitee_pr_detail`,
  `gitee_list_issues`, `gitee_create_issue` (marked destructive).
- **Status bar menu** (plugin API 1.14): open the panel, notifications, create a
  pull request, all commands, refresh.
- **All commands picker**: one place listing every Gitee command.
- **Settings**: items per page, cache lifetime, preferred remote, confirm before
  writing, require an issue in commit messages.

### Changed

- The plugin's HTTP layer is split per resource, with an error taxonomy
  (unauthorized / forbidden / not found / conflict / rate limited / server /
  network) whose messages carry the fix ("add a token in Settings › …").
- Lists are cached (30 s by default) so re-rendering the panel costs no requests;
  every list is paged, up to Gitee's 100-per-page limit.
- Responses are parsed leniently: issue numbers may be strings (`ICK440`),
  counts may be strings, labels may be strings or objects, a file patch is
  `{"diff": …}`.
- `engines.openidea` is `^1.14` (the status bar menu and hover APIs need it).

### Fixed

- **Issue numbers are strings**: a repository whose issues are numbered
  `ICK440` no longer fails to list ("invalid type: string, expected u64").
- The status bar no longer counts merged pull requests as open.

## 0.2.0

- Gitee tool window with the project's pull requests and issues, and every
  repository of the token; double-click a repository to clone it.
- Tools menu entry, status bar item, interactive clone flow.

## 0.1.1

- Interactive clone: pick a repository (or enter a URL), choose a folder, clone
  and open the project.
- Issue numbers are deserialized as strings (regression test included).

## 0.1.0

- First version: list pull requests and issues, create a repository, clone and
  push, status bar item, personal token and OAuth2 bearer authentication.
