# EverOS-Hermes 升级 note（压缩版）

更新时间：2026-05-13 22:25 CST

## 历史结论

上一轮升级已完成并验证：Python/Rust workflow 已统一处理 `messages[].timestamp` epoch-ms 校验、dry-run warning、执行前 `validation_failed`、payload metrics、`split_count`、Cloud 403 adaptive split。README、Rust README、Cloud v1 contract、`problems.md` 与 Hermes skill pressure reference 已同步。

## 已收敛能力

- Python full tests：95 passed。
- Rust gates：`cargo fmt --all --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test --tests --no-fail-fast` 均通过；Rust tests 39 passed。
- prebuilt package 已构建并安装到本地 Hermes：installed MCP `tools/list` 为 13 tools，provider schemas 为 8 tools，`hermes mcp test everos` 通过。
- 低层 primitive client 行为保持稳定，workflow helper 负责 timestamp/metrics/split-on-403。

## 当前升级焦点

下一阶段不再重复已收敛的 timestamp / split-on-403 修复。当前核心问题转向 Agent Memories 可见性与 Cloud raw residual 语义：

1. `scope="agent"` 写入、provider hooks capture/sync/flush 返回成功或 queued，但 `memory_types=["agent_memory"]` 检索持续 0 命中，`agent_case` / `agent_skill` 也为 0。
2. all-types hybrid search 同时保持 27-29 hits，说明 agent-scope raw/queued 数据或 personal/episodic 路径存在，但 Agent Memories structured visibility 未建立。
3. delete 后 structured memories 可清空；raw_message 仍可能保留或被 raw search 命中，后续执行必须区分 structured cleanup 与 raw residual。
4. `flush agent` 存在低频 transient request send failure，需要 retry/reporting 明确化。

## 执行纪律

- 使用已有非默认 user id 做真实 Cloud 压测；禁止新建 user id；禁止触碰 `hermes_default`。
- 小 patch、先 RED 后 GREEN、Python 与 Rust surfaces 同步。
- 每个阶段写入临时工作日志；遇到长文件先摘要到临时文档。
- 不把 `.tmp_*`、`dist/`、`target/`、`__pycache__`、`.pytest_cache` 等临时/构建产物纳入提交。

## 升级规划2执行日志

### 2026-05-13 13:25 CST — 执行启动

- 状态：实际代码升级开始；`升级规划2.md` 的代码、测试、文档、smoke、release/install 内容尚未完成。
- 当前工作区：仅已有 `升级note.md` 修改与 `升级规划2.md` 新增；临时摘录目录保持 ignored。
- 执行顺序：先 Python RED tests，再 Python GREEN；之后 Rust parity、文档、smoke、package/install。
- 当前信心：对规划文件 100% 有信心；对实现尚未开始，必须通过每阶段 RED/GREEN 与复核建立事实信心。

### 2026-05-13 13:28 CST — P-A1/P-A2 RED 完成

- 新增 `tests/test_agent_visibility.py`，覆盖 report not_visible、partial、audit 独立 search/get、nested hit count。
- 已运行：`python -m pytest -p no:cacheprovider tests/test_agent_visibility.py -q`。
- RED 结果：按预期失败，`ModuleNotFoundError: No module named 'everos_hermes.agent_visibility'`。
- 反思：测试初稿曾错误使用非规划状态 `partial_error`；已修正为允许值 `error`。当前对 RED 测试 100% 有信心，可以进入 P-B1。

### 2026-05-13 13:30 CST — P-B1/P-B2 GREEN 完成

- 新增 `src/everos_hermes/agent_visibility.py`。
- 实现 `build_agent_visibility_report()` 与 `audit_agent_visibility()`。
- 核心行为：query trim、每个 check 独立捕获 `EverOSError`/`EverOSTimeoutError`、每个 check 记录 `latency_ms`、使用 `workflows.count_hits()` 统计 nested hit shapes。
- 已运行：`python -m pytest -p no:cacheprovider tests/test_agent_visibility.py -q`，结果 `4 passed`。
- 反思：当前核心 helper 对规划 2.1/2.3 的 Python 部分达到 100% 信心；后续风险在 workflow/provider/MCP 集成，不在 helper 单元行为。

### 2026-05-13 13:32 CST — P-A3 workflow RED 完成

- 在 `tests/test_workflows.py` 新增 agent workflow RED tests。
- 已运行 targeted pytest，2 个测试按预期失败：
  - `save_and_verify(scope="agent")` 缺少 top-level `agent_visibility`。
  - `verify_session_ingest(scope="agent")` 仍返回 `not_yet_searchable`，而不是规划固定的 `agent_not_visible`。
- 当前信心：RED 测试准确覆盖 P2-08 workflow 语义缺口，可以进入 P-B3。

### 2026-05-13 13:35 CST — P-B3 workflow GREEN 完成

- 修改 `src/everos_hermes/workflows.py`：
  - `verify_session_ingest(scope="agent")` 追加 `agent_visibility` audit。
  - `save_and_verify(scope="agent")` 返回 top-level `agent_visibility`，并把 queued/flush 与 structured visible 分开。
  - `import_and_verify(scope="agent")` 在显式 verification_queries 存在时同步 top-level `agent_visibility`。
  - 新增 `_agent_workflow_status()`，将 `not_visible` 固定映射为 `agent_not_visible`。
- 已运行：
  - `python -m pytest -p no:cacheprovider tests/test_agent_visibility.py tests/test_workflows.py::test_save_and_verify_agent_reports_not_visible_separately_from_queue tests/test_workflows.py::test_verify_session_ingest_agent_scope_returns_visibility_checks -q`，`6 passed`。
  - `python -m pytest -p no:cacheprovider tests/test_workflows.py -q`，`5 passed`。
- 反思：当前 Python workflow 语义对规划 B2 达到 100% 信心；下一风险是 MCP/provider primitive 与 flush retry 尚未接入。

### 2026-05-13 13:40 CST — P-A4/P-A5 provider/MCP RED 完成

- RED targeted command 覆盖 5 个测试：MCP save/add unchecked visibility、MCP agent flush transient retry、provider save unchecked visibility、provider sync_turn visibility gap status、provider agent flush transient retry。
- 失败原因符合预期：`agent_visibility` 缺失、`EverOSError("error sending request")` 未 retry、`_last_agent_visibility_status` 未定义。
- 根因定位：primitive save/add/flush 仍直接返回旧 payload；flush helper 只处理 timeout；agent trajectory 写入后未在配置开启时执行 structured visibility audit。
- 当前信心：100% 确认 RED 有效，可以进入 P-B5-P-B9 最小实现。

### 2026-05-13 13:48 CST — P-B5/P-B9 Python provider/MCP GREEN 完成

- 新增 `src/everos_hermes/flush_retry.py`：仅对 `EverOSError` 中的 `error sending request` 做一次 retry；`EverOSTimeoutError` 不自动 retry，保留“先 search/status 再 retry”的语义。
- MCP primitive：`everos_save_memory`、`everos_add_memories`、`everos_flush_memories` 在 `scope=agent` 时返回 unchecked `agent_visibility`。
- Provider primitive：`everos_memory_save`、`everos_memory_flush` 在 `scope=agent` 时返回 unchecked `agent_visibility`。
- Provider auto trajectory：新增 `agent_visibility_verify_after_flush` 配置，开启后 flush 成功立即执行 `audit_agent_visibility()`，并记录 `_last_agent_visibility_status`。
- 回归修复：内部 trajectory flush 不传 `timeout=None`，避免改变既有 capture 调用形状；工具/MCP primitive 仍保留 timeout 参数。
- 验证：provider/MCP targeted 5 tests passed；provider/MCP/workflows/visibility 49 tests passed；Python 全量 `tests` 106 passed；`git diff --check` 已运行。
- 阶段反思：对 Python provider/MCP 当前实现 100% 有信心。已覆盖 RED 失败点、旧调用形状回归、full Python suite。下一步进入 Rust parity。

### 2026-05-13 13:51 CST — R-A Rust parity RED 完成

- 新增/扩展 `rust-version/tests/parity.rs`：
  - `mcp_agent_save_add_flush_return_unchecked_visibility_and_retry_transient_flush`
  - `mcp_agent_flush_retries_transient_send_error_and_reports_visibility`
  - `mcp_save_and_verify_agent_scope_reports_structured_visibility`
  - `provider_save_tool_scope_agent_posts_agent_endpoint` 追加 unchecked visibility 断言
- RED 结果：`cargo test --test parity mcp_agent -- --nocapture` 失败，错误为 agent flush transient request-send failure 未 retry，以及 primitive agent visibility envelope 缺失。
- 下一步：实现 Rust `agent_visibility`、`flush_retry`，同步 MCP/provider/workflows 接入。

### 2026-05-13 14:05 CST — R-B Rust parity GREEN 完成

- Rust 侧补齐 `agent_visibility` 与 `flush_retry` helper，并在 MCP/provider/workflows 中对齐 Python agent scope 语义。
- 新增/扩展 Rust parity tests 覆盖：
  - MCP `everos_save_memory`/`everos_add_memories` agent scope 返回 unchecked `agent_visibility`；
  - MCP `everos_flush_memories` agent scope 对 transient send error 执行 retry，并返回 `attempt_count` 与 unchecked visibility；
  - MCP `save_and_verify` agent scope 使用 agent_memory/case/skill 可见性检查并报告 `agent_not_visible`；
  - provider `everos_memory_save` scope=agent POST `/api/v1/memories/agent` 并返回 unchecked visibility。
- Rust provider 同步新增 `agent_visibility_verify_after_flush` 配置、agent trajectory flush retry 与可选 post-flush audit 状态记录。
- 验证：`cargo fmt --check` 通过；`cargo test --all-targets` 42 passed；`cargo clippy --all-targets -- -D warnings` 通过；Python `python -m pytest -p no:cacheprovider tests -q` 106 passed。
- 信心复核：R-B 经过 targeted RED/GREEN、Rust 全量、Python 全量、fmt/clippy 后，对 Rust parity 当前实现达到事实上的 100% 自信；下一步进入 D/S docs-smoke。

### 2026-05-13 14:22 CST — D/S 前置复核：补齐 agent_visibility 配置实现

- 在进入 README/contract 同步前复核 `升级规划2.md` 2.2 配置清单，发现 Python/Rust provider 只落了 `agent_visibility_verify_after_flush`，文档若直接更新会超前实现。
- 新增 RED：Python `_normalize_config` 与 Rust `ProviderConfig/load_config` 的 `agent_visibility_*` 默认值、覆盖值、边界 clamp 测试。
- GREEN：补齐 `agent_visibility_verify_after_write`、`agent_visibility_queries`、`agent_visibility_top_k`、`agent_visibility_timeout`、`agent_visibility_get_page_size`、`agent_visibility_retry_flush_attempts`、`agent_visibility_retry_flush_backoff_ms` 的 Python/Rust 配置默认值与规范化；provider 自动 audit 改为使用 queries/top_k/timeout/get_page_size 与 verify_after_write/verify_after_flush。
- 验证：`python -m pytest -p no:cacheprovider tests -q` = 107 passed；`cargo test --all-targets` = 43 passed；`cargo fmt --check` = pass；`cargo clippy --all-targets -- -D warnings` = pass。
- 信心复核：此前不能 100% 自信，因为配置文档会暴露未实现字段；补 RED/GREEN 与双端回归后，对配置/默认行为/门禁一致性达到事实 100% 自信，继续 D/S 文档与 smoke。

### 2026-05-13 14:55 CST — D/S 文档与 smoke 完成
- 文档同步：更新 `README.md`、`rust-version/README.md`、`docs/everos_cloud_v1_contract.md`、`problems.md`，补充 agent visibility envelope、配置项、primitive unchecked 语义、flush transient retry、fake/real smoke 说明；`problems.md` 更新时间更新为 2026-05-13 14:40 CST。
- 新增 smoke 脚本：
  - `scripts/everos_agent_visibility_smoke.py`：本地 fake EverOS Cloud + Rust MCP stdio；覆盖 tools/list=13、`not_visible/partial/visible`、primitive unchecked save/add、`role=tool` 缺 `tool_call_id` 本地拒绝、agent flush transient retry；summary 脱敏 `authorization`/`Authorization`。
  - `scripts/everos_real_cloud_smoke.py`：真实 EverOS Cloud smoke；强制拒绝 `hermes_default`，默认使用非默认 user `hermes_mcp_stress_main_20260512_125809`，写入唯一 session 后 session-scoped cleanup。
- Fake-server smoke：`python scripts/everos_agent_visibility_smoke.py --binary rust-version/target/debug/everos-hermes-rust --mode build-tree --output .tmp_everos_visibility_smoke/build_tree_summary.json` 通过；22 assertions，visibility=`not_visible/partial/visible`，transient retry attempts=1，summary authorization 已复核为 `Bearer ***`。
- Real Cloud smoke：`python scripts/everos_real_cloud_smoke.py --binary rust-version/target/debug/everos-hermes-rust --output .tmp_everos_visibility_smoke/real_cloud_summary.json` 通过；user_id=`hermes_mcp_stress_main_20260512_125809`，session_id=`eh_real_cloud_smoke_20260513_145229_2095607`，`save_status=agent_not_visible`，`agent_visibility_status=not_visible`，cleanup 204，post-cleanup agent_memory search empty。未触碰 `hermes_default`。
- 100% 信心复核：docs-smoke 对本地实现和真实 Cloud 的可见性语义有信心；仍明确保留 Cloud 行为限制：agent structured visibility 真实 Cloud 当前仍可能 `not_visible`，因此本地只报告状态，不把 queued/flush 当成 structured visible。

### 2026-05-13 15:13 CST — V/C/P/I release-install 完成
- Full pre-commit gate：`python -m pytest -p no:cacheprovider tests -q` = 107 passed；`python -m py_compile src/everos_hermes/*.py integrations/hermes/__init__.py scripts/everos_agent_visibility_smoke.py scripts/everos_real_cloud_smoke.py` 通过；`cargo fmt --all --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test --tests --no-fail-fast` 通过（Rust 43 tests）；`git diff --check` 与 staged secret scan 通过。
- Commit：初始提交 `fb6d19b Add agent memory visibility reporting`；package/install 结果写入 note 后执行 amend，最终 SHA 以后续 `git rev-parse --short HEAD` 为准。
- Package：`/home/xu/project/tools/EverOS-Hermes/rust-version/dist/everos-hermes-rust-0.2.0-x86_64-unknown-linux-gnu.tar.gz`；SHA256 `d252a95a9cec9cd52e797906b80d6da4d184499ce21456eb2027bb5b62729108`；`sha256sum -c` OK。
- Archive 内容：`bin/everos-hermes-rust` 可执行，`integrations/hermes/__init__.py` / `plugin.yaml` 存在，`README.md` / `INSTALL.md` 存在；binary version `everos-hermes-rust 0.2.0`，plugin version `0.2.0`，provider tool schemas=8 且含 visibility 文案。
- Installed：binary `/home/xu/.local/bin/everos-hermes-rust` -> `/home/xu/.local/share/everos-hermes/bin/everos-hermes-rust`；plugin `/home/xu/.hermes/plugins/everos`；`provider is-available --hermes-home /home/xu/.hermes` 返回 available=true。
- Hermes MCP：`hermes mcp test everos` 通过，stdio 指向 `/home/xu/.local/share/everos-hermes/bin/everos-hermes-rust`，tools discovered=13。
- Installed fake-server smoke：`python scripts/everos_agent_visibility_smoke.py --binary /home/xu/.local/bin/everos-hermes-rust --mode installed --output .tmp_everos_visibility_smoke/installed_summary.json` 通过；22 assertions，`not_visible/partial/visible` 覆盖，transient retry attempts=1，summary authorization=`Bearer ***`。
- 100% 信心复核：升级规划2的本地实现、文档、fake/real Cloud smoke、package/install 门禁均已完成；唯一保留项是 Cloud 侧 agent structured extraction/search 真实 contract 仍表现为 `not_visible`，本地已正确报告而不误判 visible。

### 2026-05-13 22:25 CST — plugin 文档与版本收口
- README、plugin README、Rust README 已更新为当前单 plugin 安装路径：`hermes plugins enable everos` 暴露 8 个 `everos_memory_*` standalone tools 与 qualified skill `everos:everos-memory-curation`，`hermes config set memory.provider everos` 启用自动 recall/capture hooks。
- `everos-memory-curation` 已改为薄 `SKILL.md` router，详细 runbook 拆到 `references/user-intent-runbooks.md`、`memory-routing-policy.md`、`agent-case-visibility.md`、`agent-visibility-contract-audits.md`、`plugin-triage-and-migration.md`、`cleanup-and-verification.md`。
- Python package、Rust crate/binary、Hermes plugin manifest 版本统一到 `0.3.0`；历史 0.2.x package/install 记录保留为历史，不再代表当前安装说明。
- 当前文档明确区分 provider explicit tools=8 与 stdio compatibility MCP-13 tools；旧 README badge/旧九工具说法不再代表当前状态。
