# EverOS-Hermes Cloud v1 升级 note

更新时间：2026-05-12 11:22 CST（最终验证、0.2.0 package、本地安装、MCP/provider smoke 均通过）

## 必须优先报告的完成状态

- `EverOS-Hermes_Cloud_升级清单.md` 的代码/文档/打包/本地安装主项已完成；剩余若用户需要，是 commit/push/GitHub release 发布动作。
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

## 当前剩余动作

- 代码/文档/测试/打包/本地安装已完成。
- 若需要对外发布，还需执行：`git add` → `git commit` → `git push` → GitHub Release `v0.2.0` 上传 tar.gz 与 `.sha256` → 下载校验。
