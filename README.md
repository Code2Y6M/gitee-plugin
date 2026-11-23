# Gitee for OpenIDEA

把 [Gitee（码云）](https://gitee.com) 集成进 OpenIDEA：在编辑器里查看和管理 Pull Request、Issue，创建远程仓库，克隆/推送到 Gitee。

OpenIDEA 插件是**沙箱化的 WebAssembly 组件**（WASI preview 2），用 Rust 编写，通过 `openidea-plugin` SDK 与 IDE 交互。

## 功能

| 命令 | 说明 |
| --- | --- |
| `Gitee: Open Repositories Panel` | 打开 Gitee 工具窗口（也可从「工具 › Gitee: 打开仓库面板」进入） |
| `Gitee: List Pull Requests` | 列出当前项目对应 Gitee 仓库的 open PR |
| `Gitee: List Issues` | 列出 open Issue |
| `Gitee: Create Repository` | 在账户下创建远程仓库（`<name>`） |
| `Gitee: Clone Repository` | 通过 `git clone` 克隆（`<owner/repo>` 或 URL） |
| `Gitee: Push to Remote` | 推送当前分支到上游 |
| `Gitee: Refresh Status` | 手动刷新状态栏 |

状态栏会显示当前项目对应的 Gitee 仓库和 open PR 数量，点击即打开 Gitee 面板。

### Gitee 工具窗口

「工具 › Gitee: 打开仓库面板」打开右侧的 **Gitee** 面板：

- **当前项目**：当前仓库的 open PR 与 Issue（树形列出，带作者）
- **我的仓库**：令牌可见的全部仓库；双击（或选中后点「克隆选中项」）即进入克隆流程——选目录、`git clone`、自动打开项目
- 工具栏：刷新、克隆选中项、新建仓库
- 面板未配置令牌时给出提示，指引到「设置 › 插件 › Gitee › Secrets」

## 鉴权

支持两种方式，在 **设置 › 插件 › Gitee** 里选择：

- **私人令牌（token，推荐）**：在 Gitee 生成[私人令牌](https://gitee.com/profile/personal_access_tokens)，通过 IDE 的插件秘密管理界面填入 `gitee-token`。请求使用 `Authorization: token <token>`。
- **OAuth2（Bearer）**：配置 OAuth 应用的 client ID/secret，填入 `gitee-oauth-token` 秘密。

令牌保存在系统钥匙串（IDE 秘密存储）中，插件代码**永远读不到明文**——它只通过 `{{secret:name}}` 占位符让 IDE 在发起请求时自动注入，因此不会泄露到日志或存储。

## 构建

前置：Rust（stable）与 `wasm32-wasip2` 目标、[openidea-plugin](https://openidea.sh) CLI 工具（可选，用于 `check`/`link`）。

```sh
# 安装 CLI（可选，用于校验与本地加载）
cargo install --locked --git https://github.com/l0ng-ai/openidea ide-host --bin openidea-plugin

# 构建
./build.sh            # 或：cargo build --release --target wasm32-wasip2
# 产物：dist/plugin.wasm

# 校验（与「从磁盘安装」完全一致）
openidea-plugin check

# 本地加载到 IDE 调试（编辑循环：build 后 Settings › Plugins › Reload）
openidea-plugin link
```

## 目录结构

```
openidea-gitee/
├─ Cargo.toml          # cdylib + rlib，依赖 openidea-plugin SDK（path 指向 vendor）
├─ plugin.toml         # 插件清单：id / 能力 / 命令 / 状态栏 / 设置 / 秘密
├─ icon.svg
├─ build.sh
├─ settings/
│  └─ settings.json    # 设置页 schema（鉴权方式等）
├─ vendor/
│  └─ openidea-0.1.50/ # vendored OpenIDEA 源码（含 sdk/rust/openidea-plugin）
└─ src/
   ├─ lib.rs           # Plugin trait 实现：命令 + 事件 + 状态栏
   └─ api.rs           # Gitee OpenAPI v5 客户端（host::fetch）
```

> **关于 `vendor/`**：由于插件 SDK 以 git 依赖方式声明（`git = "…", tag = "v0.1.50"`）
> 在国内网络环境下常因 `git clone` 走不通而无法构建，本仓库把该 tag 的源码
> **vendored** 到了 `vendor/openidea-0.1.50/`，并以 `path` 依赖方式引用。
> 若你在能直连 GitHub 的环境构建，可把 `Cargo.toml` 里的依赖改回：
> `openidea-plugin = { git = "https://github.com/l0ng-ai/openidea", tag = "v0.1.50" }`
> 并删除 `vendor/` 目录。

## 能力用途说明（Capability justification）

上架审核要求说明每个高风险能力的用途。本插件在 `plugin.toml` 中声明：

| 能力 | 用途 |
| --- | --- |
| `net:gitee.com` | 通过 HTTPS 调用 Gitee OpenAPI v5，获取 PR/Issue 列表、仓库信息，以及创建远程仓库。域名限定为 `gitee.com`，不访问其他主机。 |
| `process:git` | 执行 `git clone`（克隆命令）与 `git push`（推送命令）。这是实现「克隆仓库」「推送到远程」两个功能的必要手段，仅在你主动触发这些命令时运行，且只使用固定的 `git` 可执行文件与固定参数。 |
| `secrets` | 读取你在 IDE 秘密存储（系统钥匙串）里配置的 `gitee-token` / `gitee-oauth-token`。插件代码不接触明文，仅通过 `{{secret:name}}` 占位符让 IDE 在发请求时自动注入鉴权头。 |
| `project.read` | 读取当前项目的 Gitee 远程地址（`Project::vcs_remotes()`），用于确定要查询的 `owner/repo`。 |
| `vcs.read` | 获取版本控制状态，用于在状态栏跟随当前聚焦的仓库。 |
| `events` | 订阅定时刷新（`on-schedule`）与 VCS 变更事件，让状态栏的 PR 数量保持更新。 |

## 技术要点

- **网络**：`host::fetch`，HTTPS 需 `net:gitee.com` 能力；鉴权头用 `{{secret:...}}` 占位符 + `secrets` 能力。
- **VCS**：远程地址通过 `Project::vcs_remotes()` 获取；克隆/推送通过 `Project::run_process("git", ...)` 执行（需 `process:git` 能力）。
- **状态栏**：`on-schedule` 与 `on-event:vcs-changed` 激活，定时刷新。

## 安全设计

- **令牌零泄露**：私人令牌/OAuth token 只存在 IDE 秘密存储（系统钥匙串），插件通过 `{{secret:name}}` 占位符让 IDE 注入请求头，代码从不接触明文，也不写日志。
- **能力最小化**：`plugin.toml` 只声明必要能力；`process:git`、`net:gitee.com`、`secrets` 等均需用户授权，高风险操作（如 `git clone`）仅在可信项目触发。
- **注入防护**：Gitee 远程地址经 `parse_remote` 严格校验（仅 `owner/repo` 两段、白名单字符 `[a-zA-Z0-9._-]`），拒绝路径穿越与 shell 元字符，杜绝 SSRF 与命令注入。
- **错误脱敏**：API 错误信息只截取前 200 字符，不含鉴权信息。

## 上架

插件就绪后，可提交到 OpenIDEA 社区插件市场（community registry）。社区插件由作者自行构建并签名，用户在 `Settings › Plugins › Marketplace` 一键安装。
