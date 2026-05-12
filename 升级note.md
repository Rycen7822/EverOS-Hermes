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
