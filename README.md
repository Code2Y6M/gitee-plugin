# Gitee for OpenIDEA

把 [Gitee（码云）](https://gitee.com) 集成进 OpenIDEA：在编辑器的 Gitee 面板里看通知、Pull Request、Issue 和仓库，审阅改动、评论、合并、建 Issue、克隆与推送，在「克隆仓库」对话框里直接挑仓库，并把 Gitee 作为工具交给 AI 代理。

OpenIDEA 插件是**沙箱化的 WebAssembly 组件**（WASI preview 2），用 Rust 编写，通过 `openidea-plugin` SDK 与 IDE 交互。

## 功能

| 命令 | 说明 |
| --- | --- |
| `Gitee: Open Repositories Panel` | 打开 Gitee 工具窗口（也可从「工具 › Gitee: 打开仓库面板」进入） |
| `Gitee: All Commands` | 一个列出全部 Gitee 命令的选择器（面板工具栏与状态栏菜单也有入口） |
| `Gitee: List Pull Requests` / `List Issues` | 以文本列出当前仓库的 PR / Issue |
| `Gitee: Notifications` / `Mark All Notifications Read` | 打开通知收件箱 / 全部标为已读 |
| `Gitee: Create Pull Request` | 从当前分支建 PR（选目标分支、填标题描述） |
| `Gitee: Show Details` | 以只读标签页打开 PR / Issue 详情（描述、改动文件、提交、评论） |
| `Gitee: Show Changed Files` | 选一个改动文件，用 IDE 的 diff 视图查看补丁 |
| `Gitee: Comment` / `Approve Pull Request` / `Merge Pull Request` | 评论、通过审查、合并（merge / squash / rebase，可选删源分支、关关联 Issue） |
| `Gitee: New Issue` / `Change Issue State` | 新建 Issue（可多选标签）/ 关闭、重开、进行中、拒绝 |
| `Gitee: Create Repository` / `Clone Repository` / `Clone This Repository` | 建仓库、从仓库列表克隆、克隆选中项 |
| `Gitee: Push to Remote` | 推送当前分支到上游 |
| `Gitee: Open in Browser` / `Open This File on Gitee` | 在浏览器打开选中项 / 当前文件 |
| `Gitee: Use Issue in Commit Message` | 把 Issue 编号写进正在准备的提交信息 |
| `Gitee: Refresh` | 清缓存并刷新 |

状态栏显示当前仓库、open PR 数与未读通知；**点击状态栏项弹出菜单**（打开面板、通知、建 PR、全部命令、刷新）。

### Gitee 工具窗口

「工具 › Gitee: 打开仓库面板」打开右侧的 **Gitee** 面板：

- **通知**：未读优先，选中即标记已读
- **当前项目**：PR（可按 open / merged / closed / all 切换）与 Issue（open / closed / all），带作者与状态
- **我的仓库**：令牌可见的全部仓库；双击（或选中后点「克隆」）进入克隆流程——选目录、`git clone`（带状态栏进度）、自动打开项目
- **工具栏**：刷新 / 切换 PR 状态 / 新建 PR / 克隆 / 全部命令
- **右键菜单**：详情、改动文件、评论、通过审查、合并、改 Issue 状态、克隆、浏览器打开（按条目类型显示）
- 未配置令牌时，面板直接给出指引（设置 › 插件 › Gitee › Secrets）

### 克隆仓库对话框（Get from VCS）

**文件 › 新建 › 从 VCS 获取**（或欢迎页的 Clone Repository）左侧的来源列表里多一项 **Gitee**：

- 列出令牌可见的仓库，可搜索（名称与描述）
- **HTTPS / SSH** 一键切换克隆地址（仓库没有 SSH 地址时回退 HTTPS）
- 点一个仓库，地址填进对话框的「仓库 URL」，随后仍是你熟悉的目标目录 / 浅克隆表单——git 只在点 Clone 后运行
- 未配置令牌、网络失败时，右边的面板直接说明原因和下一步

这一项来自插件 API 1.15 的 `[[contributes.cloneSources]]`：页面（`clone/index.html`）跑在沙箱 frame 里，没有网络，只通过 `openidea.postMessage` 向插件要数据，选中后把地址交回对话框。**因此 1.1.0 需要 OpenIDEA 的插件 API 1.15**；更早的 IDE 请装 1.0.3。

### 与 IDE 的其它集成

- **提交信息工具栏**：「Gitee: 在提交信息里引用 Issue」把 `#编号` 写进待提交信息（基于 `Commit` 接口，冲突时会重试而不覆盖你正在输入的内容）。
- **提交检查**：开启「要求提交信息引用 Issue」后，提交前会提示缺少 `#编号`。
- **搜索**：双击 Shift 的搜索里多一个 **Gitee** 分类，可搜已加载的仓库 / PR / Issue。
- **AI 代理（MCP）**：暴露 `gitee_list_prs`、`gitee_pr_detail`、`gitee_list_issues`、`gitee_create_issue` 四个工具（创建 Issue 标记为 destructive）。

## 鉴权

支持两种方式，在 **设置 › 插件 › Gitee** 里选择：

- **私人令牌（token，推荐）**：在 Gitee 生成[私人令牌](https://gitee.com/profile/personal_access_tokens)，通过 IDE 的插件秘密管理界面填入 `gitee-token`。请求使用 `Authorization: token <token>`。
- **OAuth2（Bearer）**：填入 `gitee-oauth-token` 秘密，请求使用 `Authorization: Bearer <token>`。

令牌保存在系统钥匙串（IDE 秘密存储）中，插件代码**永远读不到明文**——它只通过 `{{secret:name}}` 占位符让 IDE 在发起请求时自动注入，因此不会泄露到日志或存储。

> 提示：未打开项目时，OpenIDEA 会拒绝插件命令与密钥保存（`plugin_run_command` / `plugin_set_secret` 声明为 `trust = trusted`）。首次配置令牌请先打开任意项目，或改用「从磁盘安装」的目录加载方式后在该窗口中配置。

## 设置

| 设置 | 默认 | 说明 |
| --- | --- | --- |
| 鉴权方式 | `token` | 私人令牌或 OAuth2 Bearer |
| 每页数量 | `50` | 一次请求多少仓库 / PR / Issue |
| 面板缓存（秒） | `30` | 列表复用时长，减少 API 调用（Gitee 有频率限制） |
| 首选远程 | `origin` | 项目有多个远程时用哪一个 |
| 修改前确认 | 开 | 合并 / 通过 / 评论 / 关 Issue / 建仓库前询问 |
| 要求提交信息引用 Issue | 关 | 开启后提交检查会提示缺少 `#编号` |

## 构建

前置：Rust（stable）与 `wasm32-wasip2` 目标、[openidea-plugin](https://openidea.sh) CLI 工具（可选，用于 `check`/`link`）。

```sh
# 安装 CLI（可选，用于校验与本地加载）
cargo install --locked --git https://github.com/l0ng-ai/openidea ide-host --bin openidea-plugin

# 构建
./build.sh            # 或：cargo build --release --target wasm32-wasip2
# 产物：dist/plugin.wasm

# 测试与校验（与「从磁盘安装」完全一致）
cargo test
openidea-plugin check

# 本地加载到 IDE 调试（编辑循环：build 后 Settings › Plugins › Reload）
openidea-plugin link
```

## 目录结构

```
openidea-gitee/
├─ Cargo.toml          # cdylib + rlib，依赖 openidea-plugin SDK（path 指向 vendor）
├─ plugin.toml         # 插件清单：能力 / 命令 / 菜单 / 视图 / 秘密 / 设置 / MCP / 提交检查
├─ icon.svg
├─ build.sh
├─ locales/
│  └─ zh-CN.json       # 中文文案（命令标题、设置项、密钥说明）
├─ clone/
│  └─ index.html       # 「克隆仓库」对话框里的 Gitee 页面（单文件，无网络）
├─ mcp/                # MCP 工具的输入 JSON Schema
├─ settings/
│  └─ settings.json    # 设置页 schema
├─ docs/
│  └─ UPGRADE-PLAN.md  # 升级计划（路线、边界、风险）
├─ vendor/
│  └─ openidea-0.1.50/ # vendored OpenIDEA 源码（含 sdk/rust/openidea-plugin）
└─ src/
   ├─ lib.rs           # 插件实现：面板、命令、写操作、提交检查、MCP、搜索
   ├─ i18n.rs          # 运行时中英文选择
   └─ api/
      ├─ mod.rs        # HTTP 客户端、错误分类、分页、表单编码
      ├─ types.rs      # Gitee 对象（宽松解析）
      ├─ repos.rs      # 仓库与分支
      ├─ pulls.rs      # PR 读取、创建、合并、评论、审查
      ├─ issues.rs     # Issue 读取、创建、状态、评论、标签
      └─ notify.rs     # 通知收件箱
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
| `net:gitee.com` | 通过 HTTPS 调用 Gitee OpenAPI v5：仓库、分支、PR、Issue、通知。域名限定 `gitee.com`，不访问其他主机。 |
| `process:git` | 执行 `git clone` 与 `git push`。仅在用户主动触发克隆/推送时运行，使用固定的 `git` 与固定参数。 |
| `secrets` | 读取秘密存储里的 `gitee-token` / `gitee-oauth-token`。代码不接触明文，只让 IDE 用 `{{secret:name}}` 注入鉴权头。 |
| `project.read` | 读取当前项目的远程地址与仓库根目录，确定 `owner/repo`、把编辑器文件映射成仓库相对路径。 |
| `vcs.read` | 读取当前分支与仓库状态：状态栏跟随聚焦仓库、建 PR 时确定源分支、失败时提示。 |
| `vcs.write` | 把 Issue 编号写入正在准备的提交信息（`commit.set-commit-message`）。只在用户运行「在提交信息里引用 Issue」时发生，不提交、不改工作区。 |
| `events` | 订阅定时刷新与 VCS 变更事件，让状态栏与面板数据保持更新。 |

## 技术要点

- **网络**：`host::fetch` + `{{secret:...}}`；统一的错误分类（401/403/404/409/422/429/5xx/网络），每类带可操作提示。
- **缓存**：仓库 / PR / Issue / 通知各带 TTL（默认 30 秒，可设置），面板重复渲染不重复请求。
- **分页**：所有列表 `page` + `per_page`，上限 100（Gitee 的上限）。
- **宽松解析**：Issue 编号是字符串（可能是 `ICK440`）、计数可能是字符串、标签可能是字符串或对象、文件 patch 是 `{"diff": …}`——都能解析。
- **VCS**：远程地址 `Project::vcs_remotes()`；分支 `Project::vcs_status()`；克隆/推送 `run_process("git", …)`。
- **界面**：`views` + `view-provider`（IDE 原生渲染）、`viewItem.context` 右键子菜单、`open-content-tab` 详情页、`open-diff` 补丁、API 1.14 的状态栏菜单。

## 安全设计

- **令牌零泄露**：只存在 IDE 秘密存储（系统钥匙串），通过 `{{secret:name}}` 注入请求头，代码不接触明文、不写日志。
- **能力最小化**：只声明必要能力；`vcs.write` 仅用于写提交信息。
- **注入防护**：远程地址经 `parse_remote` 严格校验（仅 `owner/repo` 两段、白名单字符 `[a-zA-Z0-9._-]`），拒绝路径穿越与 shell 元字符。
- **写操作可控**：默认「修改前确认」；合并需选择方式并二次确认；删除源分支、关关联 Issue 都是显式确认。
- **错误脱敏**：API 错误只保留消息与前 200 字符正文，不含鉴权信息。

## 上架

插件就绪后提交到 OpenIDEA 社区插件市场（community registry）。社区插件由作者自行构建并签名，用户在 `Settings › Plugins › Marketplace` 一键安装。
