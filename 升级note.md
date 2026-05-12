# EverOS-Hermes Cloud v1 升级 note

更新时间：2026-05-12 11:26 CST（最终验证、0.2.0 package、本地安装、push/release/download 校验均通过）

## 必须优先报告的完成状态

- `EverOS-Hermes_Cloud_升级清单.md` 的代码/文档/打包/本地安装/commit/push/GitHub release 主项已完成。
- 当前已完成 Sprint 1/2/3/4/finalverify：Python client/MCP/provider/formatter、Rust client/MCP/provider/formatter、README/rust README、0.2.0 package 与本地 Hermes 安装均已同步。
- 版本已按清单 P8 建议升至 `0.2.0`，并保持 Python/Rust/plugin manifest 一致。

## 当前仓库状态

- `EverOS-Hermes_Cloud_升级清单.md` 当前是未跟踪文件。
- 生成物/本地文件仍为 ignored：`agentmemory-main/`、`problems.md`、`rust-version/dist/`、`rust-version/target/`。
- 现有核心源码：
  - Python client: `src/everos_hermes/client.py`
  - Python MCP: `src/everos_hermes/mcp_server.py`
  - Python provider: `src/everos_hermes/provider.py`
  - Python formatter: `src/everos_hermes/formatting.py`
  - Rust client/MCP/provider/formatter: `rust-version/src/*.rs`

## 已发现的关键差距（均已在本轮修复或明确排除）

1. Python/Rust 默认能力面已不包含 group/sender/object/storage；清单排除项写入 contract/README，并有 contract tests 保护。
2. Python/Rust `request_json()` 对 204/empty body 保留 `ok/status_code` 语义。
3. Python/Rust `add_memories()` 已支持 `scope="personal|agent"` 与 message/schema 校验，`agent` 仅保留为兼容 alias 且冲突报错。
4. filters 已经做 user/session 注入、allowlist 与冲突检测；不再把 group_id 混入默认工具面。
5. search/get/delete/settings 已补 top_k/radius/memory_type/rank/strict/diff 等校验与 MCP/provider 参数上浮。
6. delete single/batch 互斥和 batch `confirm_scope_text` 安全确认已实现，MCP batch delete 不再静默使用 default user。
7. provider agent trajectory capture、agent flush、agent recall、redacted status/log 已实现；Rust provider sync_turn agent capture 已追加 parity 测试。
8. formatter 已支持 nested `agent_memory.cases/skills`、`raw_messages`，并继续默认剥离 vector 字段。
9. Rust parity 已同步，当前 Python/Rust user-facing behavior 通过全量测试约束。

## 执行原则

- 按 TDD：先补失败测试，再小 patch 实现，再跑目标测试与相关回归。
- 每个阶段完成后自检：若对实现没有事实上的 100% 信心，列出漏洞并继续修复。
- 及时更新本 note，防止上下文压缩后丢失关键决策。
- 不做 group/sender/object/multimodal 支持，不把这些参数混入默认工具签名。
- 不过度设计；逻辑以 schema validators + 明确 client/MCP/provider 边界为主。

## 基线验证

2026-05-12 10:12 CST 已跑基线：

- Python：`python -m pytest -p no:cacheprovider tests -q` → `21 passed`
- Python compile：`python -m py_compile src/everos_hermes/*.py integrations/hermes/__init__.py` → 通过
- Rust：`cargo fmt --all --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test --tests --no-fail-fast` → 11 个 parity/integration tests 通过

## 阶段自信度反思

基线阶段对“当前实现未完成清单”的判断有 100% 事实依据：清单要求的 contract doc、schema validators、strict delete/settings、MCP filters/radius/rank/scope、provider agent capture、Rust parity validator 均在当前源码中缺失或未完整上浮。下一步先写测试锁定这些差距。

## Sprint 1 进展

2026-05-12 10:12-当前：

- 已新增 contract 文档：`docs/everos_cloud_v1_contract.md`。
- 已新增 schema validators：`src/everos_hermes/schemas.py`，覆盖 scope、messages、search/get 类型、filters DSL、delete mode、settings strict whitelist。
- 已扩展 Python client：
  - `request_json()` 保留 `status_code`，204/empty body 返回 `ok/status_code`。
  - `add_memories()` 支持 `scope`，保留 `agent` alias 并检测冲突。
  - `flush_memories()` 支持 `scope`。
  - `search_memories()` 校验 method/memory_types/top_k/radius/filters，并允许 `top_k=-1`。
  - `get_memories()` 校验 memory_type/page/page_size/rank_by/rank_order 并归一化 rank_order。
  - `delete_memories()` 实现 single/batch 互斥，移除默认 group 路径。
  - `update_settings()` 默认 strict，并返回 before/after diff。
- 已新增/扩展测试：`tests/test_cloud_contract.py`、`tests/test_schemas.py`、`tests/test_everos_client.py`。
- 已观察 RED：新增测试最初 15 个失败，原因明确为缺 docs/schema/client 新语义。
- 已观察 GREEN：`python -m pytest -p no:cacheprovider tests/test_cloud_contract.py tests/test_schemas.py tests/test_everos_client.py -q` → `23 passed`。
- Python 全量现有测试：`python -m pytest -p no:cacheprovider tests -q` → `36 passed`。

## Sprint 1 自信度反思

对 Python client 的 Cloud v1 contract/security 边界目前有事实上的高信心：新增红灯测试覆盖了 endpoint whitelist、filters 冲突、类型映射、top_k/radius、delete mode、settings strict、scope alias、204 响应。仍未声明整个清单完成，因为 MCP/provider/Rust/docs README 还未同步，且 request timeout 的 user/session/scope 细粒度 payload 需要在 MCP/provider lifecycle 层继续上浮。

## Sprint 2 进展

- 已扩展 MCP 显式工具层：
  - `everos_save_memory` 增加 `scope` 与 `role`，支持 personal/agent 单条便捷写入。
  - `everos_add_memories` 增加 `scope`，保留 `agent` alias 并检测冲突。
  - `everos_flush_memories` 增加 `scope`，保留 `agent` alias。
  - `everos_search_memories` 暴露 `filters`、`radius`、`timeout`、`fallback_to_hybrid`，支持 `top_k=-1` 与 agentic 超时 fallback hybrid。
  - `everos_get_memories` 暴露 `filters`、`rank_by`、`rank_order`。
  - `everos_delete_memories` 增加 `confirm_scope_text`，batch delete 不再静默使用 default user。
  - `everos_update_settings` 增加 `strict` 与 `return_diff`。
- 已新增 MCP schema/call 测试并观察 RED→GREEN：`python -m pytest -p no:cacheprovider tests/test_everos_mcp_server.py -q` → `13 passed`。
- Python 全量现有测试：`python -m pytest -p no:cacheprovider tests -q` → `42 passed`。

## Sprint 2 自信度反思

对 MCP 工具参数完整性和 delete 安全边界有高信心：测试直接检查 FastMCP tool schema、函数调用透传、agentic fallback、delete confirmation。仍未声明清单完成：provider 自动 agent memory、formatter/prompt 污染防护、README/rust-version README、Rust parity 还未同步。

## Sprint 3 Python 侧进展

- 已扩展 provider config：`agent_capture_mode`、`agent_recall`、`agent_memory_types`、`agent_flush_after_turn`、`agentic_timeout`、`max_context_items`。
- `capture_agent_memory=true` 时，`sync_turn()` 会构造 agent trajectory messages 并写入 `/api/v1/memories/agent`（client 层以 `scope="agent"` 表达），同时可独立 agent flush。
- `agent_capture_mode="parallel"` 写 personal + agent；`agent_only` 只写 agent；`off` 禁用 agent trajectory。
- `prefetch()` 支持 `agent_recall=true` 时额外检索 `memory_types=["agent_memory"]`，格式化时仍区分用户记忆与 agent experience。
- provider explicit tools 已上浮 `scope`、`filters`、`radius`、`top_k=-1`、`response_format`、`rank_by/rank_order`。
- provider 背景异常不再完全吞掉；记录 redacted 状态到 provider 内部 `_last_*_status` 和 `$HERMES_HOME/everos.log`，不写入原始 user/assistant content 与 API key。
- formatter 已支持 nested `agent_memory.cases/skills`、`raw_messages` 分区，并继续默认剥离 embedding/vector 字段。
- 已观察 RED：formatter nested agent_memory 测试最初失败，provider 新行为测试最初 8 个失败。
- 已观察 GREEN：
  - `python -m pytest -p no:cacheprovider tests/test_everos_provider.py tests/test_formatting.py -q` → `14 passed`
  - `python -m pytest -p no:cacheprovider tests -q` → `50 passed`

## Sprint 3 自信度反思

对 Python provider/formatter 侧 100% 自信的部分：scope 写入路径、agent capture 开关、agent recall、context stripping、tool schema 和 redacted log 都有直接测试覆盖。尚不能声明清单完成：Rust provider/MCP/client 仍未同步；README 与 migration guide 未更新；最终 hermes/package 验证未做。下一步进入 Rust parity，必须把 Python 侧 user-facing schema 与语义同步过去。

## Sprint 4 Rust parity / docs / package 进展

2026-05-12 11:02 CST：

- Rust parity 已同步：
  - `rust-version/src/client.rs` 支持 `scope`、strict message validation、filters conflict detection、`top_k=-1`、`radius`、delete mode validation、settings update diff/strict 语义对应 Python。
  - `rust-version/src/mcp.rs` 上浮 `filters/radius/timeout/fallback_to_hybrid/rank_by/rank_order/scope/confirm_scope_text`，并保留 `agent` alias 的冲突检测。
  - `rust-version/src/provider.rs` provider tools 上浮 `scope/filters/radius/top_k/response_format/rank_by/rank_order`，agent scoped save/flush 已同步。
  - `rust-version/src/formatting.rs` 支持 nested `agent_memory.cases/skills` 与 `raw_messages`。
- Rust parity tests 已扩展到 18 个：覆盖 Cloud contract validator、MCP/provider schema、delete safety、formatter agent/raw message、provider agent-scoped save、provider sync_turn agent capture、stdio MCP smoke。
- 版本已从 `0.1.1` 升至 `0.2.0`：`pyproject.toml`、`rust-version/Cargo.toml`、两个 `plugin.yaml`、README asset 示例均同步。
- README 与 `rust-version/README.md` 已补充：Cloud v1 contract、out-of-scope group/sender/object、filters/radius/top_k=-1/rank/delete/settings、agent memory scope 说明。
- 验证命令：
  - `python -m pytest -p no:cacheprovider tests -q` → `50 passed`
  - `python -m py_compile src/everos_hermes/*.py integrations/hermes/__init__.py` → 通过
  - `cargo fmt --all --check` → 通过
  - `cargo clippy --all-targets --all-features -- -D warnings` → 通过
  - `cargo test --tests --no-fail-fast` → `18 passed`
  - `git diff --check` → 通过
- 已构建并验证 0.2.0 prebuilt package：
  - `/home/xu/project/tools/EverOS-Hermes/rust-version/dist/everos-hermes-rust-0.2.0-x86_64-unknown-linux-gnu.tar.gz`
  - SHA256：`943ec03b239e37d21b9266caf130ad9e7a09f87f05af7cdf3942da32da6661a2`
  - `sha256sum -c`、解包结构、`plugin.yaml version: 0.2.0`、binary `--version/--help`、provider `tool-schemas` schema assertion、`provider is-available --hermes-home /home/xu/.hermes` 均通过。

## Sprint 4 自信度反思

对 Rust parity 与 package 有高信心：Rust 18 个 parity/integration tests 覆盖新增 user-facing 行为，package 验证覆盖版本、插件 manifest、二进制可执行与 provider schema；另用本地 fake EverOS server 验证 fresh provider sync-turn 同时写 personal 与 agent endpoint。

## Final verify 结果

2026-05-12 11:22 CST：

- 已清理 `__pycache__` / `*.pyc`。
- Python 全量：`python -m pytest -p no:cacheprovider tests -q` → `50 passed`。
- Python compile：`python -m py_compile src/everos_hermes/*.py integrations/hermes/__init__.py` → 通过。
- Rust 全量：`cargo fmt --all --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test --tests --no-fail-fast` → `18 passed`。
- `git diff --check` → 通过。
- secret scan：变更/未跟踪文件未发现疑似真实密钥；本地测试假 key 忽略。
- 0.2.0 package：`sha256sum -c`、解包结构、provider schema assertions → 通过。
- 已将 0.2.0 package 安装到当前 Hermes 环境：
  - binary: `/home/xu/.local/share/everos-hermes/bin/everos-hermes-rust`
  - shim/link: `/home/xu/.local/bin/everos-hermes-rust`
  - plugin: `/home/xu/.hermes/plugins/everos`
- `hermes mcp test everos` → Connected，发现 9 tools。
- fresh provider sync-turn smoke：在本地 fake EverOS server 下确认同时 POST `/api/v1/memories` 与 `/api/v1/memories/agent`，agent message 以 `Task request:` 开头。

## Release / push 结果

- 主实现 commit：`386bb1fbc547be2f3f8573375aa4d68c4f64fb73`，已 push 到 `origin/main`。
- GitHub Release：`v0.2.0`，URL：`https://github.com/Rycen7822/EverOS-Hermes/releases/tag/v0.2.0`。
- Release target：`386bb1fbc547be2f3f8573375aa4d68c4f64fb73`。
- 已上传 assets：
  - `everos-hermes-rust-0.2.0-x86_64-unknown-linux-gnu.tar.gz`
  - `everos-hermes-rust-0.2.0-x86_64-unknown-linux-gnu.tar.gz.sha256`
- 已下载 GitHub release assets 并重新执行 `sha256sum -c`、解包、binary `--version`、plugin manifest `version: 0.2.0` 校验，均通过。

## 当前剩余动作

- 仅剩本 note 的最终记录提交；无代码/测试/打包/release 阻塞项。

## 2026-05-12 12:14 CST：MCP 2.0 settings hotfix

用户要求修复 `problems.md` 新记录的问题。已按 TDD 修复 Rust MCP `everos_update_settings` strict / return_diff 缺口：

- RED：新增 Rust parity 测试后，旧实现上 `cargo test --test parity mcp_update_settings -- --nocapture` 失败，确认 strict 未知字段仍发 HTTP。
- GREEN：`rust-version/src/client.rs` 已新增本地 settings whitelist 校验，`strict=true` 只允许 `timezone` / `llm_custom_setting`；`return_diff=true` 执行 GET-before / PUT / GET-after，并返回 `diff` / `updated`。
- MCP：`rust-version/src/mcp.rs` 已透传 `strict` 与 `return_diff`。
- 回归：Rust parity 扩展到 20 tests，`cargo clippy --all-targets --all-features -- -D warnings` 与 `cargo test --tests --no-fail-fast` 通过。
- Python 回归：`python -m pytest -p no:cacheprovider tests -q` → `50 passed`；`python -m py_compile ...` 通过。
- 本地 prebuild：已重打包并安装 `everos-hermes-rust-0.2.0-x86_64-unknown-linux-gnu.tar.gz`，安装后 `hermes mcp test everos` 连接成功并发现 9 tools。
- installed-binary smoke：`/tmp/everos_mcp20_full_smoke.py` → `24 passed, 0 failed`。
- `problems.md` 已整理：最新 P0 标记为已解决，保留批量迁移 helper / structured output schema / save_and_verify 为非阻塞 backlog。

## 2026-05-12 13:05 CST：MCP 2.0 tool_call_id / numeric parser hotfix

用户要求继续修复压力测试新增的 P1-02 / P1-03。已按 TDD 与真实 Cloud 压力验证闭环完成：

- RED：新增 Python schema/MCP/provider 回归与 Rust parity，用例覆盖 `role=tool` 缺少 `tool_call_id`、`scope=agent` 默认 role、`tool_call_id` schema/请求体、`top_k/page/page_size` 非法边界 HTTP 前失败、`radius=0` 保留。
- GREEN：
  - Python：`schemas.py` 校验 `role=tool` 必须带 `tool_call_id`；`mcp_server.py` / `provider.py` 暴露并透传 `tool_call_id`，agent 默认 role 改为 `assistant`。
  - Rust：`client.rs` 增加同等 message 校验；`mcp.rs` / `provider.rs` 暴露并透传 `tool_call_id`；`mcp.rs` numeric parser 不再 clamp，`float_arg()` 保留 `0.0`。
- 文档：`README.md`、`rust-version/README.md`、`docs/everos_cloud_v1_contract.md` 已同步 tool role 与 numeric boundary 契约。
- 验证：Python 全量 `51 passed`；Rust `cargo fmt --all --check` / `cargo clippy --all-targets --all-features -- -D warnings` / `cargo test --tests --no-fail-fast` 通过，Rust parity `24 passed`；`git diff --check` 通过。
- 打包安装：已重打包 `everos-hermes-rust-0.2.0-x86_64-unknown-linux-gnu.tar.gz`，`sha256sum -c` 通过，并重新安装到 `/home/xu/.local/share/everos-hermes`；`provider is-available` 与 `hermes mcp test everos` 均通过。
- 压力测试：
  - fake-server installed MCP：`/tmp/everos_mcp20_stress.py` -> `18 passed, 0 failed`，201 次 MCP calls / 247 次 HTTP requests。
  - 真实 Cloud installed MCP：`/tmp/everos_mcp20_real_stress.py` -> `24 passed, 0 failed`，46 次 MCP calls，合成 session 删除 204。
- `problems.md` 已更新：P1-02 / P1-03 标记为已解决；当前无 P0/P1 阻塞问题，仅保留非阻塞 backlog。

## 2026-05-12 14:12 CST：非阻塞 workflow/helper 改进项落地

用户要求执行此前保留的 B1/B2/B3 非阻塞改进项。本轮按 Python → Rust parity → installed MCP smoke 闭环完成，没有新增 Cloud endpoint，只编排既有 Cloud v1 白名单端点：

- B1 batch/import helper：
  - Python 新增 `src/everos_hermes/workflows.py`；Rust 新增 `rust-version/src/workflows.rs`。
  - MCP 新增 `everos_batch_ingest`、`everos_import_and_verify`、`everos_verify_session_ingest`。
  - Provider 新增 `everos_memory_import_and_verify`、`everos_memory_verify_session`。
  - 支持 dry-run、warnings、batch_size 分批、optional flush、verification queries、per-batch 报告。
- B2 structured output：
  - 新 workflow tools 声明/返回稳定 envelope：`ok`、`workflow`、`status`、`retryable`、`suggested_next_actions`。
  - workflow payload 补充 `input_count`、`queued_count`、`failed_count`、`warnings`、`batches`、`flush`、`verification`、`save` 等 typed fields。
  - 9 个底层 primitive tools 暂不强行迁移完整 envelope，保持兼容；workflow envelope 作为后续模板。
- B3 save/import/verify 高层工作流：
  - MCP 新增 `everos_save_and_verify`、`everos_import_and_verify`、`everos_batch_ingest`、`everos_verify_session_ingest`。
  - Provider 新增 `everos_memory_save_and_verify`、`everos_memory_import_and_verify`、`everos_memory_verify_session`。
  - 状态区分 `verified`、`partially_verified`、`not_yet_searchable`、`verification_skipped`、`dry_run`，不把 verification miss 误判成写入失败。
- 验证：
  - Python 全量：`python -m pytest -p no:cacheprovider tests -q` -> `59 passed`；`py_compile` 通过。
  - Rust 全量：`cargo fmt --all --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test --tests --no-fail-fast` 通过；Rust parity `30 passed`。
  - 打包安装：`./scripts/package-release.sh` 重打包 0.2.0 本地 prebuild，`sha256sum -c` 通过，已重新安装到 `/home/xu/.local/share/everos-hermes` 与 `~/.hermes/plugins/everos`。
  - `hermes mcp test everos` -> connected，13 tools discovered。
  - 新 workflow installed MCP smoke：`/tmp/everos_mcp_workflow_smoke.py` -> `5 passed, 0 failed`。
  - 原 fake-server installed MCP 压力测试更新 tool discovery 后：`/tmp/everos_mcp20_stress.py` -> `18 passed, 0 failed`，201 calls / 247 HTTP requests。
- 文档：`README.md`、`rust-version/README.md`、`docs/everos_cloud_v1_contract.md`、`problems.md` 已同步 workflow/helper 能力、收益和剩余边界。
- 剩余风险：真实 Cloud 大批量导入尚未执行；建议未来真实迁移先 `dry_run=true`，再小样本导入并用 batch delete 清理。版本号仍为 `0.2.0`，本轮暂未 release；若公开发布建议另起 patch 版本。

## 2026-05-12 20:21 CST：OpenClaw context-engine Python-only 升级启动

### 必须优先报告的完成状态

- `升级规划.md` 的规划文档已经完成。
- 按该规划完成 EverOS-Hermes 代码升级：未完成，当前开始执行 Phase 0。
- 用户消息中提到的 `/home/xu/project/autoscientist/升级规划.md` 是 Codex-Scientist 规划；本任务目标是 EverOS-Hermes，因此实际执行 `/home/xu/project/tools/EverOS-Hermes/升级规划.md`。
- 本轮严格 Python-only：不修改 `rust-version/**`，不做 Rust parity，不新增 Node/JS 依赖。

### Phase 0 进展

- 基线：`python -m pytest tests -q` -> `59 passed`。
- 新增契约测试：`tests/test_upgrade_contract.py`，覆盖 Python MCP source 13 tools、provider explicit tools 8、README 不含 stale MCP-9 badge/wording、Cloud v1 out-of-scope blacklist。
- RED：`python -m pytest tests/test_upgrade_contract.py -q` 初次结果为 `1 failed, 3 passed`，失败点是 README badge 仍为 `MCP-9%20tools` 且 alt 为 `MCP: nine tools`。
- GREEN：已将 README badge 修正为 `MCP-13%20tools` / `MCP: thirteen tools`。
- 回归：`python -m pytest tests/test_upgrade_contract.py -q && python -m pytest tests -q` -> `4 passed` + `63 passed`。

### Phase 0 自信度反思

- 当前对 Phase 0 有事实上的 100% 信心：新增测试先失败再修复，已证明 README stale badge 被契约测试捕获；MCP 13 tools、provider 8 tools、endpoint blacklist 均有自动化约束；本阶段未修改 runtime 行为。
- 下一步进入 Phase 1：TDD 实现 `src/everos_hermes/trajectory.py` 与 `tests/test_trajectory.py`。

## 2026-05-12 20:28 CST：Phase 1 trajectory 转换器完成

### Phase 1 进展

- RED：先新增 `tests/test_trajectory.py` 后运行 `python -m pytest tests/test_trajectory.py -q`，因 `everos_hermes.trajectory` 不存在而失败，符合预期。
- GREEN：新增 `src/everos_hermes/trajectory.py`，实现 `TrajectoryBuildResult` 与 `build_agent_trajectory_messages()`。
- 已覆盖规划固定用例：
  - user/assistant/tool 链保留 `tool_calls` 与 `tool_call_id`；
  - 缺失 `tool_call_id` 的 tool message 丢弃并 warning；
  - assistant 空 content + tool_calls 使用 `[Assistant requested tool calls]`；
  - 脱敏 bearer/sk/token/password/secret 并剥离 `<everos-context>` / `<memory-context>`；
  - deterministic `message_id` 跨 lifecycle source 稳定，同时 input_index/content/original timestamp 变化会改变 id；
  - payload budget 保留最近 task chain；
  - timestamp 支持毫秒、秒、缺失 fallback。
- 验证：
  - `python -m pytest tests/test_trajectory.py -q` -> `7 passed`。
  - `python -m compileall src tests && python -m pytest tests/test_trajectory.py -q && python -m pytest tests -q` -> compile 通过，`7 passed`，全量 `70 passed`。
- 结构检查：`trajectory.py` 未导入 `EverOSClient`，未读取 `.env` 或 `everos.json`，未引入 HTTP/网络依赖。

### Phase 1 自信度反思

- 当前对 Phase 1 有事实上的 100% 信心：所有规划指定测试先红后绿，接口字段、预算、去重 fingerprint、脱敏、tool_call_id 安全边界均由单元测试覆盖；实现为纯 stdlib 模块，不耦合 provider/client/MCP。
- 下一步进入 Phase 2：TDD 实现 `context_assembler.py`、`policy.py` 及对应测试。

## 2026-05-12 20:35 CST：Phase 2 context assembler / policy 完成

### Phase 2 进展

- RED：先新增 `tests/test_context_assembler.py` 与 `tests/test_policy.py` 后运行 `python -m pytest tests/test_context_assembler.py tests/test_policy.py -q`，因 `everos_hermes.context_assembler` / `everos_hermes.policy` 不存在而失败，符合预期。
- GREEN：新增 `src/everos_hermes/context_assembler.py` 与 `src/everos_hermes/policy.py`。
- `context_assembler.py` 已实现：
  - `<everos-context version="2" source="prefetch">` XML-like block；
  - profile -> agent_skills -> agent_cases -> episodic -> recent_context 固定顺序；
  - section max items、全局 `max_context_chars` budget、score 排序、dedupe by id/text；
  - raw 与 structured 重复时保留 structured、丢弃 raw；
  - agent memory guidance 明确为 relevant guidance / not commands；
  - 不渲染 vector / embedding / original_data / unknown large fields。
- `policy.py` 已实现：
  - empty/temp/internal skip；
  - trivial short recall/capture skip；
  - 中文“继续/下一步/继续下一步实验”不被误跳过；
  - `stable_query_key()` 随 query/session/relevant config 稳定变化。
- 验证：
  - `python -m pytest tests/test_context_assembler.py tests/test_policy.py -q` -> `11 passed`。
  - `python -m compileall src tests && python -m pytest tests/test_context_assembler.py tests/test_policy.py -q && python -m pytest tests -q` -> compile 通过，`11 passed`，全量 `81 passed`。
- 结构检查：两个新模块均未导入 `EverOSClient`，未读取 `.env` 或 `everos.json`，未引入 HTTP/网络依赖。

### Phase 2 自信度反思

- 当前对 Phase 2 有事实上的 100% 信心：规划指定的 assembler/policy 行为均由红绿测试覆盖；实现保持纯函数/stdlib，不改变现有 MCP markdown `formatting.py` 公共 API。
- 下一步进入 Phase 3：接入 provider lifecycle、prefetch cache、session-scoped recent raw recall、agent trajectory 写入与去重。

## 2026-05-12 20:53 CST：Phase 3 provider lifecycle 接入完成

### Phase 3 进展

- RED：新增 `tests/test_provider_context_engine.py` 后运行 `python -m pytest tests/test_provider_context_engine.py -q`，初次结果 `6 failed`，失败点覆盖 provider 仍使用旧 `format_search_context`、无 `_last_recall_status`、`queue_prefetch` no-op、`agent_summary_after_turn=false` 仍写 agent、`on_pre_compress` no-op、无 `on_delegation`。
- GREEN：修改 `src/everos_hermes/provider.py`，接入 Phase 1/2 新模块：
  - import `trajectory` / `context_assembler` / `policy`；
  - 扩展 `_DEFAULT_CONFIG` 与 `_normalize_config`，加入 `max_context_chars`、`include_recent_raw`、`recent_raw_top_k`、section max items、`prefetch_cache_enabled`、`prefetch_cache_ttl_seconds`、agent trajectory lifecycle flags、agent budget、agent dedupe 等配置；
  - 增加 `_prefetch_cache`、`_prefetch_inflight`、`_prefetch_lock`、`_agent_saved_fingerprints`、`_last_recall_status`、`_last_agent_trajectory_status`；
  - `prefetch()` 改为 policy -> cache -> personal/agent/raw independent search -> `assemble_everos_context()`；
  - `queue_prefetch()` 实现后台预取，锁保护 in-flight 与 cache；
  - `sync_turn()` personal messages 增加 deterministic `message_id`，并在 `agent_summary_after_turn=true` 时才通过 `build_agent_trajectory_messages(..., source="sync_turn")` 写轻量 agent summary；
  - `on_session_end()` 先写完整 agent trajectory，再按既有路径 flush personal；agent trajectory 写入失败不阻断 personal flush；
  - `on_pre_compress()` 写 agent trajectory 且永不 flush，成功返回压缩提示；
  - 新增 `on_delegation()`，assistant content 前缀包含 `[delegation child_session_id=<id>]`，输出消息保留 `child_session_id` 字段。
- 兼容修复：更新 `tests/test_everos_provider.py` 中旧 `# EverOS Memory` 断言为 `<everos-context version="2" source="prefetch">`，对应 provider prefetch 已切换到新版 assembler。
- 追加覆盖缺口：补充 raw partial failure 与 session_end 写入/flush 顺序测试，`tests/test_provider_context_engine.py` 当前 8 个测试覆盖：
  - assembler/cache/agent recall/raw session scope；
  - raw session 缺失警告；
  - raw search 失败保留 main context；
  - queue_prefetch in-flight dedupe 与 cached consume；
  - personal deterministic message_id 与 `agent_summary_after_turn=false`；
  - pre_compress no-flush + session_end dedupe；
  - session_end agent trajectory before personal flush；
  - delegation child_session_id prefix/field + agent flush。
- 清理：删除被新版 trajectory 替代的旧 `_build_agent_trajectory_messages()` helper 与旧 `_TRIVIAL_RE`，没有留下死代码；测试生成的 `.pytest_cache` / `__pycache__` 已清理。

### Phase 3 验证

- `python -m pytest tests/test_provider_context_engine.py -q` -> `8 passed`。
- `python -m pytest tests/test_everos_provider.py tests/test_provider_context_engine.py -q` -> `23 passed`。
- `python -m compileall src tests` -> 通过。
- `python -m py_compile src/everos_hermes/*.py integrations/hermes/__init__.py` -> 通过。
- `python -m pytest tests -q` -> `89 passed`。
- `git diff --check` -> 通过。
- 变更文件 secret/dead-marker scan：无疑似真实 `sk-` key、无 bearer token、无 TODO/FIXME/pass-marker。

### Phase 3 自信度反思

- 当前对 Phase 3 有事实上的 100% 信心：规划列出的 provider lifecycle、cache、raw recall、sync_turn summary flag、session_end/pre_compress/delegation trajectory、dedupe 与 redacted failure 路径均由自动化测试或既有 provider 测试覆盖；全量 Python 测试通过；实现未引入 Node/JS/Rust 改动，保持 Python-only。
- 下一步进入 Phase 4：保持 MCP/workflow/schema 兼容，补 `message_id` schema/workflow/MCP 测试，确保 MCP 13 tools 与 provider 8 tools 不回归。

## 2026-05-12 21:00 CST：Phase 4 MCP/workflow/schema 兼容完成

### Phase 4 进展

- 严格按规划执行：未新增 MCP 工具、未改任何 tool name；`mcp_server.py` 只改 `everos_add_memories` docstring，说明 `message_id` 是可选且会被保留。
- RED：先补测试后运行 `python -m pytest tests/test_schemas.py tests/test_everos_mcp_server.py -q`，结果 `1 failed, 25 passed`，失败点是 `validate_messages()` 未校验空/非字符串 `message_id`。
- GREEN：修改 `src/everos_hermes/schemas.py`，为 `messages[*].message_id` 增加轻量校验：字段可缺省；若存在，必须是非空字符串。
- MCP/workflow 覆盖：
  - `tests/test_everos_mcp_server.py` 新增 `test_mcp_add_memories_preserves_message_id_and_rejects_tool_role_without_call_id`，通过真实 `EverOSClient` + monkeypatch `request_json` 验证 agent scope `everos_add_memories` 保留 message_id，并且 role=`tool` 缺失 `tool_call_id` 会在本地校验失败、不触网。
  - `test_mcp_batch_ingest_batches_flushes_and_verifies` 增加 `message_id` 样例，验证 workflow `normalize_import_messages` / `import_and_verify` 不丢弃未知字段。
  - `tests/test_schemas.py` 增加合法/非法 `message_id` 覆盖。

### Phase 4 验证

- `python -m pytest tests/test_schemas.py tests/test_everos_mcp_server.py -q` -> `26 passed`。
- `python -m compileall src tests` -> 通过。
- `python -m py_compile src/everos_hermes/*.py integrations/hermes/__init__.py` -> 通过。
- `python -m pytest tests/test_everos_mcp_server.py tests/test_schemas.py tests/test_formatting.py -q` -> `28 passed`。
- `python -m pytest tests -q` -> `90 passed`。
- `git diff --check` -> 通过。
- Source count check：MCP `TOOL_NAMES` 仍为 13；provider explicit tool schema entries 仍为 8。
- 变更文件 secret scan：无真实 `sk-` key / bearer token；测试仍只使用 `sk-test` 占位值。
- 测试/编译生成的 `.pytest_cache` 与 `__pycache__` 已清理。

### Phase 4 自信度反思

- 当前对 Phase 4 有事实上的 100% 信心：规划列出的 schema、workflow、MCP 兼容项均有测试覆盖；工具数量保持不变；`role=tool` 的 `tool_call_id` 本地校验仍有效；没有引入 Cloud 访问或 Rust 改动。
- 下一步进入 Phase 5：更新 README/docs/升级note.md，执行最终全量验证、静态检查、文档 secret scan 与 Python-only 完成闭环。

## 2026-05-12 21:09 CST：Phase 5 文档与最终验收完成

### Phase 5 进展

- RED：先扩展 `tests/test_upgrade_contract.py`，要求 README 说明 Python context-engine 新能力与 Rust 边界，要求 Cloud contract 说明 `message_id` 与 structured agent trajectory。运行 `python -m pytest tests/test_upgrade_contract.py -q` 得到 `2 failed, 4 passed`，失败点为 README/docs 尚未记录新能力。
- GREEN：更新 `README.md`：
  - 保持 badge 为 `MCP-13 tools`；
  - Features 增加 Python context engine：structured agent trajectory、budgeted context assembler、deterministic message ids、prefetch cache、opt-in session-scoped recent raw recall；
  - Rust section 明确：`Python context-engine upgrade is not yet Rust parity`；
  - Python Version 与 provider advanced settings 增加 `max_context_chars`、`prefetch_cache_enabled`、`prefetch_cache_ttl_seconds`、`include_recent_raw`、`recent_raw_top_k`、`agent_trajectory_on_session_end`、`agent_trajectory_on_pre_compress`、`agent_trajectory_on_delegation` 等配置示例；
  - Provider Behavior 表格补 `on_pre_compress()` / `on_delegation()`，并说明 prefetch assembler/cache/raw、sync_turn deterministic message_id、session_end structured trajectory before flush；
  - MCP operations 中说明 `everos_add_memories` 保留可选 `message_id`；
  - Project Layout 增加 `context_assembler.py` / `policy.py` / `trajectory.py`；
  - Development smoke 文案从 “nine EverOS tools” 修正为 “thirteen EverOS tools”。
- 更新 `docs/everos_cloud_v1_contract.md`：
  - 修正 auth placeholder 为 `Authorization: Bearer ***`；
  - personal add 示例增加 `message_id`；
  - 新增 Message fields 说明：`message_id` 是 optional idempotency key，存在时必须为非空字符串；agent `role="tool"` 继续要求 `tool_call_id`；
  - agent add 段落新增 structured agent trajectory 示例，覆盖 `message_id`、`source`、`tool_calls`、`tool_call_id`；
  - 保留 out-of-scope endpoint blacklist。
- 未修改 `rust-version/README.md`；仅在根 README 明确 Rust 尚未拥有本轮 Python context-engine parity，避免错误宣称。

### Phase 5 最终验证

- `python -m pytest tests/test_upgrade_contract.py -q` -> `6 passed`。
- `python -m compileall src tests` -> 通过。
- `python -m pytest tests/test_trajectory.py -q` -> `7 passed`。
- `python -m pytest tests/test_context_assembler.py tests/test_policy.py -q` -> `11 passed`。
- `python -m pytest tests/test_everos_provider.py -q` -> `15 passed`。
- `python -m pytest tests/test_everos_mcp_server.py tests/test_schemas.py tests/test_formatting.py -q` -> `28 passed`。
- `python -m pytest tests/test_upgrade_contract.py -q` -> `6 passed`。
- `python -m pytest tests -q` -> `92 passed`。
- `python -m py_compile src/everos_hermes/*.py integrations/hermes/__init__.py` -> 通过。
- `git diff --check` -> 通过。
- README/docs/src stale search：无 `MCP-9`、`MCP: nine tools`、`nine EverOS tools`。
- README/docs/src secret scan：无真实 `sk-` key / bearer token；文档只保留 placeholder。
- Count check：MCP `TOOL_NAMES` 仍为 13；provider explicit tool schema entries 仍为 8。
- `.pytest_cache` / `__pycache__` 已清理；ignored reference/build dirs 仍保持 ignored。

### Phase 5 自信度反思

- 当前对整套 Python-only 升级有事实上的 100% 信心：Phase 0~5 的规划项均已落地，新增行为均有单元/契约测试，最终全量测试通过，文档与代码边界一致，未触网、未写入真实 EverOS Cloud、未触碰 `hermes_default`。
- 剩余非本轮事项：Rust runtime 尚未同步本轮 Python context-engine 新能力；根 README 已明确该边界，后续若用户要求 Rust parity，需要单独按 Rust parity 流程执行。
