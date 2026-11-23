# Gitee for OpenIDEA — 系统性升级计划

> 起点：0.2.0（命令 + 一个原生渲染的 Gitee 面板）
> 目标：1.0.0（覆盖 Gitee 日常协作闭环，并通过市场长期维护）
> 状态：**1.0.0 已实施**（0.2.1 – 0.5.0 的功能合并到同一个版本发布，中途不单独发版）

### 实施结果

| 计划 | 状态 | 说明 |
| --- | --- | --- |
| T1 网络层重构 | ✅ | `src/api/{mod,types,repos,pulls,issues,notify}.rs`，每模块 < 350 行 |
| T2 错误分类 | ✅ | `ApiError` 七类，消息带修复建议（"在设置里填令牌"…） |
| T3 缓存与节流 | ✅ | 仓库/PR/Issue/通知各带 TTL（默认 30 秒，可设置） |
| T4 分页 | ✅ | 所有列表 `page`/`per_page`，上限 100 |
| T5 面板交互 | ✅ | PR/Issue 状态循环筛选、`viewItem.context` 子菜单、API 1.14 状态栏菜单 |
| T6 多仓库 | ✅ | `preferredRemote` 设置项 |
| T7 设置项 | ✅ | 每页数量 / 缓存 / 首选远程 / 写前确认 / 提交需引用 Issue |
| T8 测试基建 | ✅ | 31 个测试：URL 与 body golden、错误映射、宽松解析、视图四种状态 |
| T9 PR 详情 | ✅ | 只读标签页：状态、作者、分支、时间、描述、文件、提交、评论 |
| T10 PR diff | ✅ | 选文件 → `open_content_tab`（diff 语法高亮）；无 patch 时提示浏览器查看 |
| T11 Issue 详情 | ✅ | 描述、标签、时间、评论 |
| T12 通知收件箱 | ✅ | 未读优先、选中即已读、全部已读 |
| T13 编辑器/项目树右键 | ✅ | `editor.context` 打开当前文件、`projectView.context` 打开面板 |
| T14 表格视图 | ⚠️ 改为树 | 面板用树 + detail 列（更贴近 IDE 的 VCS 面板），未用 `Body::Table` |
| T15 建 PR | ✅ | 当前分支 → 选目标分支 → 标题/描述 → 二次确认 |
| T16 评论与审查 | ✅ | PR/Issue 评论、PR 审查通过 |
| T17 合并 | ✅ | 选 merge/squash/rebase、删源分支、关关联 Issue，再确认 |
| T18 Issue 操作 | ✅ | 新建（多选标签）、评论、关闭/重开/进行中/拒绝 |
| T19 提交信息联动 | ✅ | Commit 工具栏按钮，写入 `#编号`（revision 保护） |
| T20 提交检查 | ✅ | 可选"提交信息需引用 Issue" |
| T21 MCP 工具 | ✅ | 4 个工具，`gitee_create_issue` 标记 destructive |
| T22 全局搜索 | ✅ | `searchCategories` + 本地缓存过滤 |
| T23 任务进度 | ✅ | 克隆时状态栏任务（`tasks.begin`） |
| T24 行首动作 | ❌ 不做 | 收益低、易干扰编辑，留作后续 |
| T25 可选 OAuth | ❌ 不做 | Gitee 无 PKCE 且必须 client_secret，不在公开仓库内置密钥 |
| T26 i18n | ✅ | zh-CN 全量文案 + 运行时中英切换 |
| T27 文档 | ✅ | README、CHANGELOG、本计划 |
| T28 稳定性 | ✅ | 宽松解析 + 缓存降级 + 每次失败都有可操作提示 |

本版本 `engines.openidea = "^1.14"`（状态栏菜单用到 API 1.14），新增能力 `vcs.write`（仅用于写提交信息）。

---

## 0. 摘要

当前插件解决的是「能在 IDE 里看到 Gitee 项目并克隆」。升级方向是把它做成**日常协作台**：
看（PR/Issue/通知/差异）→ 做（建 PR、评论、审查、合并、管 Issue）→ 智能（AI Agent 工具、全局搜索）。

分五个版本台阶，每个台阶都可以独立发版、独立回滚：

| 版本 | 主题 | 一句话 |
| --- | --- | --- |
| 0.2.1 | 稳定与地基 | 网络层重构、缓存、错误分类、面板交互补齐 |
| 0.3.0 | 只读体验 | PR/Issue 详情、diff 查看、通知收件箱 |
| 0.4.0 | 写操作闭环 | 建 PR、评论审查、合并、Issue 全操作、提交信息联动 |
| 0.5.0 | 智能集成 | MCP 工具、全局搜索分类、任务进度 |
| 1.0.0 | 打磨 | 可选 OAuth、i18n 全量、文档与稳定性 |

三条平台硬限制（已核实，不做）：

1. **不能加「Git › Gitee」子菜单** — `src-tauri/src/menu.rs` 里 Git 菜单与 GitHub 子菜单是硬编码的。
2. **不能加「克隆仓库」对话框的来源项** — `registerCloneSource` 是前端内部 API（`src/workbench/contributions.ts:2131`），唯一调用方是内置 GitHub；WIT 无对应接口。
3. **无项目窗口（欢迎页）不能跑插件命令、不能存密钥** — `plugin_run_command` / `plugin_set_secret` 声明为 `trust = trusted`（已提 issue）。

---

## 1. 现状盘点

### 1.1 已实现

| 能力 | 形态 |
| --- | --- |
| 状态栏 | 当前仓库 + open PR 数，点击打开面板 |
| Gitee 面板（右侧 tool window） | 项目 PR/Issue 树 + 我的仓库列表；工具栏：刷新 / 克隆选中项 / 新建仓库；双击仓库克隆 |
| 命令 | List PRs、List Issues、Create Repository、Clone、Push、Refresh、Open Panel |
| 鉴权 | PAT（`gitee-token`）与 OAuth Bearer（`gitee-oauth-token`），经 `{{secret:}}` 注入 |
| 设置 | `authMode` 单页 |
| 质量 | 11 个单测；`check` 通过（7 commands / 1 menu / 1 view / 2 secrets） |
| 打包 | 签名 `.oiplugin`，API 1.14，`engines.openidea = "^1.3"` |

### 1.2 已知短板

1. **只读**：不能建 PR、评论、审查、合并、关 Issue、打标签、指派。
2. **无详情**：PR/Issue 只有标题，看不到描述、评论、diff、提交。
3. **无通知**：Gitee 的 `/notifications/*` 完全没用。
4. **无缓存无节流**：每次 `render` 都可能打网络；`per_page` 固定 100/50，无分页。
5. **错误一刀切**：401/403/404/429/网络错误都当字符串抛出，用户不知道该做什么。
6. **无多仓库**：只认第一个 Gitee remote。
7. **OAuth 是伪 OAuth**：只是粘贴 Bearer token，没有授权流程。
8. **面板交互薄**：无筛选、无右键菜单、无状态栏菜单、无搜索。
9. **无 MCP / 搜索集成**：AI Agent 和全局搜索都用不到 Gitee。
10. **i18n 半覆盖**：只有 zh-CN，运行时字符串靠 `locale()` 分支。
11. **测试面窄**：写请求、错误映射、分页都没有测试。
12. **无 CHANGELOG**：版本演进不可追溯。

### 1.3 代码结构

```
src/lib.rs   ~600 行  命令 + 面板 + 视图事件 + 状态栏
src/api.rs   ~320 行  HTTP + 4 个只读端点 + 1 个写端点
```

→ 0.4.0 之前必须拆分，否则写操作会让 `lib.rs` 失控。

---

## 2. 能力边界（已核实）

### 2.1 OpenIDEA 插件 API（目标 1.14 / IDE 0.1.50）

| 接口 | 能做什么 | 版本 | 计划用途 |
| --- | --- | --- | --- |
| `views` + `view-provider` | 树/列表/表单/表格/HTML + 工具栏 + 状态行，IDE 原生渲染，支持 viewport 分页 | 1.0 | 面板主体（列表/表格） |
| `viewItem.context` 菜单 | 给视图 item 加右键菜单，命令收到 item key 作为参数 | 1.2 | 打开浏览器、复制链接、详情 |
| `views.open-content-tab` / `reply-content-tab` | 在编辑器区开插件自有 tab（只读文本 + 按钮），按钮回调插件 | — | PR/Issue 详情、评论编辑 |
| `views.open-diff` / `open-file-diff` | 用 IDE 的 diff 查看器显示两段文本 | 1.7 / 1.10 | PR 单文件 diff |
| `auth.begin-loopback` | 桌面端本地回调监听 + `ui.open-url` | 1.4 | 可选 OAuth 授权码流程 |
| `ui.set-status-item-menu` | 状态栏项挂菜单（action/separator/header） | 1.14 | 快速跳转、刷新、打开面板 |
| `ui.open-url` / `confirm` / `input-multiline` / `pick-many` | 打开浏览器、二次确认、多行输入、多选 | — | 写操作的输入与确认 |
| `tasks.begin` | 状态栏任务进度 + 取消 | — | 克隆、批量刷新 |
| `commit.pending-commit` / `set-commit-message` | 读写待提交信息（需 `vcs.read` / `vcs.write`） | — | 从 Issue 生成提交信息 |
| `commitChecks` | 提交前检查（可选导出） | — | 校验提交信息引用 Issue |
| `mcpTools` + `mcp-provider` | 向 IDE AI Agent 暴露工具 | 1.4 | Gitee 读写工具 |
| `searchCategories` + `search-provider` | 全局搜索的一个分类 | 1.13 | 搜仓库/PR/Issue |
| `hover-actions` | 行首悬浮动作（需 `providers` kinds） | 1.14 | "在 Gitee 打开此行" |
| `vcs.status` / `remotes` / `log` / `blame` / `diff` | 分支、upstream、ahead/behind、提交、blame | — | 建 PR、行级追溯 |
| `editor.context` / `projectView.context` 菜单 | 编辑器/项目树右键 | 1.0 | "在 Gitee 打开" |

### 2.2 Gitee OpenAPI v5（可用面）

| 分组 | 关键端点 | 计划用途 |
| --- | --- | --- |
| 仓库 | `GET /user/repos`、`/orgs/{org}/repos`、`/enterprises/{e}/repos`、`GET /repos/{o}/{r}`、`POST /user/repos` | 列表、详情、创建 |
| 分支/提交 | `GET /repos/{o}/{r}/branches`、`/commits`、`/commits/{sha}`、`/compare/{base}...{head}` | 建 PR 时选分支、对比 |
| PR 读 | `GET /pulls?state=`、`/pulls/{n}`、`/pulls/{n}/files`（≤300 diff）、`/pulls/{n}/commits`（≤250）、`/pulls/{n}/issues`、`/pulls/{n}/operate_logs`、`/pulls/{n}/merge`（是否已合并） | 详情、diff、关联 Issue |
| PR 写 | `POST /pulls`（title/head/base/body/issue/labels/assignees/testers）、`PUT /pulls/{n}/merge`（merge/squash/rebase、删源分支、关关联 Issue）、`POST /pulls/{n}/comments`、`POST /pulls/{n}/review`（审查通过）、`POST /pulls/{n}/test`、`PATCH /pulls/{n}`、`POST|PUT /pulls/{n}/labels`、`POST /pulls/{n}/assignees`、`PATCH /pulls/comments/{id}` | 写操作闭环 |
| Issue | `GET /repos/{o}/{r}/issues?state=`、`POST /repos/{o}/issues`、`PATCH /repos/{o}/issues/{n}`、`POST /repos/{o}/{r}/issues/{n}/comments`、labels、milestones | Issue 全操作 |
| 通知 | `GET /notifications/threads`（unread/participating/type/since）、`GET /notifications/count`、`PATCH /notifications/threads/{id}`、`PUT /notifications/threads`、`GET /repos/{o}/{r}/notifications` | 收件箱 |
| Release | `GET|POST /repos/{o}/{r}/releases`、附件上传 | Release 管理（低优先） |
| 动态 | `/repos/{o}/{r}/events`、`/users/{u}/received_events`、star/watch | 可选：项目动态 |
| 鉴权 | PAT（`access_token` query 或 `Authorization`）、OAuth2 授权码 + refresh（`user_info/projects/pull_requests/issues/notes/keys/hook/groups/gists/enterprises/emails`） | PAT 为主，OAuth 可选 |

### 2.3 明确的 Non-goals

- ❌ Git 菜单子菜单 / 克隆对话框来源项（平台不可扩展）
- ❌ PR 行级（代码行）评论：Gitee 只有 `POST /pulls/{n}/comments`（普通评论）
- ❌ 内置 OAuth `client_secret`：Gitee 无 PKCE 且 token 交换必须带 secret，公开仓库里放 secret 等于泄露
- ❌ 仓库设置、成员管理、Webhook 管理（低频高危，收益低）
- ❌ 欢迎页（无项目）可用：受 `trust = trusted` 限制，等官方修

---

## 3. 版本路线与验收

### 0.2.1 — 稳定与地基（规模 M）

**目标**：不改功能面，把地基打牢，为写操作铺路。

| # | 任务 | 做法 | 验收 |
| --- | --- | --- | --- |
| T1 | 网络层重构 | `api/` 拆 `client.rs`（认证/超时/UA/重试/429 退避）、`repos.rs`、`pulls.rs`、`issues.rs`、`notify.rs` | 每个模块 < 200 行；`cargo clippy -D warnings` 零告警 |
| T2 | 错误分类 | `ApiError { Unauthorized, Forbidden, NotFound, RateLimited{retry_after}, Conflict, Network, Server }` | 面板/通知给出可操作提示（"去设置填令牌"/"稍后重试"） |
| T3 | 缓存与节流 | 内存 TTL（repos 60s、PR/Issue 30s、通知 60s）；同一 key 并发请求合并；请求预算计数 | 面板首屏 ≤ 2 次请求；10 秒内重复刷新命中缓存 |
| T4 | 分页 | `page/per_page` 封装，面板用 `Body::Table` + viewport 分页 | 100+ 仓库可分页浏览 |
| T5 | 面板交互补齐 | 状态筛选（open/all/closed）按钮；右键菜单（`viewItem.context`：浏览器打开/复制链接/详情）；状态栏菜单（API 1.14） | 右键菜单出现且命令收到 key |
| T6 | 多仓库 | 读全部 remote，按设置项选首选；面板顶部显示"切换仓库" | 两个 Gitee remote 可切换 |
| T7 | 设置项扩充 | `defaultPageSize`、`refreshSeconds`、`preferredRemote`、`confirmBeforeWrite` | 设置页可改且生效 |
| T8 | 测试基建 | `on_fetch` 断言 method/url/body；错误映射表测试；URL 构造 golden test | api 层关键路径 100% 覆盖 |

### 0.3.0 — 只读体验（规模 L）

| # | 任务 | 做法 | 验收 |
| --- | --- | --- | --- |
| T9 | PR 详情 | `open-content-tab`：标题/状态/作者/分支/描述/审查状态/关联 Issue + 按钮「浏览器打开」「评论」 | 面板双击 PR 打开详情 tab |
| T10 | PR diff | `GET /pulls/{n}/files` → 文件列表（tree）→ `open-diff` 显示单文件 patch；patch 缺失/截断时提示 | 300 文件内可逐一看 diff |
| T11 | Issue 详情 | 描述 + 评论列表（只读文本 tab） | 双击 Issue 打开详情 |
| T12 | 通知收件箱 | 面板加「通知」根节点（未读数）；`PATCH` 标记已读/全部已读 | 未读数与网页一致 |
| T13 | 编辑器/项目树右键 | `editor.context`：在 Gitee 打开此文件 / 此行的提交；`projectView.context`：在 Gitee 打开仓库 | 菜单出现并可跳转 |
| T14 | 表格视图 | PR/Issue 用 `Body::Table`（列：编号/标题/作者/更新/状态），支持排序 | 200 条流畅滚动 |

### 0.4.0 — 写操作闭环（规模 L）

| # | 任务 | 做法 | 验收 |
| --- | --- | --- | --- |
| T15 | 创建 PR | `vcs.status` 取当前分支 → 选 base（`/branches`）→ 标题/描述（`input_multiline`）→ `POST /pulls`；支持从 Issue 自动填充 | 从当前分支一键开 PR |
| T16 | 评论与审查 | `POST /pulls/{n}/comments`；`POST /pulls/{n}/review`（通过）；评论列表刷新 | 评论出现在网页 |
| T17 | 合并 PR | `confirm` 二次确认 + 合并方式选择（merge/squash/rebase）+ 删源分支/关 Issue 选项 → `PUT /merge` | 合并成功后面板状态更新 |
| T18 | Issue 操作 | 创建、评论、关闭/重开、标签（`pick_many`）、指派 | 全流程不离开 IDE |
| T19 | 提交信息联动 | `commit.pending-commit` 取改动 + Issue 选择 → `set-commit-message` 生成 `fix: … (#123)` | Commit 窗口信息被替换（用户可改） |
| T20 | 提交检查 | `commitChecks`：提交信息未引用 Issue 时警告（可关） | Commit 前出现检查项 |

### 0.5.0 — 智能集成（规模 M）

| # | 任务 | 做法 | 验收 |
| --- | --- | --- | --- |
| T21 | MCP 工具 | `mcpTools`：`gitee_list_prs`、`gitee_pr_detail`、`gitee_list_issues`、`gitee_create_issue`（destructive）、`gitee_repo_info` | Agent 会话里可调用并返回结构化 JSON |
| T22 | 全局搜索 | `searchCategories` + `search-provider`：搜我的仓库/PR/Issue | 搜索框出现 "Gitee" 分类 |
| T23 | 任务进度 | `tasks.begin`：克隆、批量刷新显示进度并可取消 | 状态栏出现进度条 |
| T24 | 行首动作 | `hover-actions`（API 1.14）：当前行对应提交在 Gitee 的链接 | 悬浮出现动作 |

### 1.0.0 — 打磨（规模 M）

| # | 任务 | 做法 | 验收 |
| --- | --- | --- | --- |
| T25 | 可选 OAuth | 自建 OAuth 应用（用户填 client_id/secret）→ `begin_loopback` + `open-url` → 换 token + refresh | 授权一次后自动续期 |
| T26 | i18n 全量 | en + zh-CN 完整；运行时字符串统一走 `locale()` | 英文环境无中文残留 |
| T27 | 文档 | README 中英、截图、CHANGELOG、能力说明表 | registry 复核一次通过 |
| T28 | 稳定性 | 全路径错误注入测试；离线降级（缓存优先） | 断网时面板不空白 |

---

## 4. 工程质量门槛

### 4.1 测试矩阵

| 层 | 覆盖 |
| --- | --- |
| URL/参数构造 | 每个端点 1 个 golden test（含分页、筛选、编码） |
| 响应解析 | 正常/缺字段/类型异常（含 Issue 字符串编号回归） |
| 错误映射 | 401/403/404/409/429/5xx/超时 各 1 例 |
| 写请求 | method + path + body 断言 |
| 视图 | 无令牌/空列表/有数据/出错 四种 body |
| 事件 | 按钮/选择/激活/未知事件 |

### 4.2 性能预算

- 面板首屏 ≤ 2 次请求，≤ 800ms（缓存命中 ≤ 50ms）
- 表格 viewport 分页，单页 ≤ 50 行
- 单次网络超时 10s，可取消

### 4.3 安全清单

- token 只经 `{{secret:...}}`，代码不接触明文
- 日志/通知不打印 header、URL query 里的 token
- 不落盘、不写 storage
- 能力保持最小：`project.read`、`vcs.read`、`events`、`net:gitee.com`、`process:git`、`secrets`；新增 `vcs.write`（提交信息）需在 PR 说明

### 4.4 发布纪律

- 一个 PR 只加一个版本条目；tag 与包 sha256、源码 commit 三者一致
- 版本语义：功能 minor、修复 patch
- 每次发版更新 CHANGELOG + change-notes
- registry 复核关注点：能力说明、测试证据、包与 tag 一致性、i18n 完整性

---

## 5. 风险登记册

| # | 风险 | 影响 | 应对 |
| --- | --- | --- | --- |
| R1 | Gitee 限流（社区数据约 1000 次/小时/令牌，官方未明示） | 面板刷新失败 | TTL 缓存 + 请求合并 + 429 退避 + 显示剩余额度提示 |
| R2 | `/pulls/{n}/files` patch 截断（>300 diff 或大文件） | diff 不完整 | 明确提示并提供"浏览器查看"；必要时用 `compare` 拉全量 |
| R3 | 分页深度限制（社区反馈 offset 深处需 since/ids） | 大账户列表不全 | 限制深度 + 跳转浏览器 |
| R4 | 无项目窗口 `trust = trusted` 拦截 | 欢迎页无法配置/使用 | 已提 issue；文档写明"先打开任意项目" |
| R5 | Gitee OAuth 无 PKCE、必须 client_secret | 不能做开箱即用授权 | 默认 PAT；OAuth 作为自建应用的高级选项 |
| R6 | SDK 0.1.50 只到 API 1.14 | 不能用 1.15（skills 等） | 升级 SDK 单独作为一个版本任务，不混在功能里 |
| R7 | 写操作误触 | 误合并/误关闭 | 默认二次确认；合并/关闭必须 `confirm` |
| R8 | registry 复核周期不确定 | 发布延迟 | 小步发版、单版本 PR、提前准备证据（测试输出、包校验） |

---

## 6. 建议的执行顺序（下一步）

1. **等 #306（0.1.1）合并** —— 不要在一个待审 PR 里叠加新版本。
2. 合并后立刻提 **0.2.0**（已完成：Gitee 面板 + 工具菜单），作为第一个"可视化"版本。
3. 然后按 **0.2.1 → 0.3.0 → 0.4.0** 推进，每个版本一个 PR、一个 release、一个 tag。
4. 0.5.0 的 MCP 与搜索建议在写操作稳定后再做（依赖同一套 API 层）。

---

## 附录 A：核实来源

- 插件 API 版本与点位：`plugins/README.md`（API 1.14 段、贡献点表、views/commits/MCP 章节）、`wit/*.wit`（`openidea:plugin@1.15.0`）
- 平台限制：`src-tauri/src/menu.rs`（Git 菜单硬编码）、`src/workbench/contributions.ts:2131`（`registerCloneSource`）、`crates/ide-host/src/trust.rs:320`（`authorize`）
- Gitee API：`https://gitee.com/api/v5/swagger`、`https://gitee.com/api/v5/oauth_doc`，以及社区 SDK 端点清单（`gitee-php/gitee-sdk`、`gitee-java-sdk`）
- 已核实端点示例：`POST /repos/{o}/{r}/pulls`、`PUT /repos/{o}/{r}/pulls/{n}/merge`、`POST /repos/{o}/{r}/pulls/{n}/review`、`GET /repos/{o}/{r}/pulls/{n}/files`、`GET /notifications/threads`、`POST /repos/{o}/issues`

## 附录 B：规模与依赖

| 版本 | 规模 | 依赖 |
| --- | --- | --- |
| 0.2.1 | M | 无 |
| 0.3.0 | L | 0.2.1 的网络层/缓存 |
| 0.4.0 | L | 0.2.1 的错误分类 + 0.3.0 的详情 UI |
| 0.5.0 | M | 0.4.0 的统一 API 层 |
| 1.0.0 | M | 全部 |

> 规模：S ≈ 单文件改动，M ≈ 一个模块 + 若干命令，L ≈ 跨模块 + 新 UI 面。
