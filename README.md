# Gitee for OpenIDEA

把 [Gitee（码云）](https://gitee.com) 集成进 OpenIDEA：在编辑器里查看和管理 Pull Request、Issue，创建远程仓库，克隆/推送到 Gitee。

OpenIDEA 插件是**沙箱化的 WebAssembly 组件**（WASI preview 2），用 Rust 编写，通过 `openidea-plugin` SDK 与 IDE 交互。

## 功能

| 命令 | 说明 |
| --- | --- |
| `Gitee: List Pull Requests` | 列出当前项目对应 Gitee 仓库的 open PR |
| `Gitee: List Issues` | 列出 open Issue |
| `Gitee: Create Repository` | 在账户下创建远程仓库（`<name>`） |
| `Gitee: Clone Repository` | 通过 `git clone` 克隆（`<owner/repo>` 或 URL） |
| `Gitee: Push to Remote` | 推送当前分支到上游 |
| `Gitee: Refresh Status` | 手动刷新状态栏 |

状态栏会显示当前项目对应的 Gitee 仓库和 open PR 数量，点击即可列出 PR。

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
