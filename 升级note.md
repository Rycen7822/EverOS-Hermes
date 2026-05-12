# EverOS-Hermes 升级 note

更新时间：2026-05-13 01:13:39 CST

## 本轮目标

依据 `problems.md` 与 `.tmp_everos_pressure/report_large_20260513_001736.md` 修复/改进真实 Cloud 压测暴露的问题，优先 Python，再同步 Rust。

## 硬约束

- 先测试后实现；每组变更按 RED -> GREEN -> REFACTOR。
- 及时记录读取结论、代码改动、测试命令、信心缺口。
- 小 patch；不回滚既有未提交改动。
- 不引入过度设计；只修当前问题。
- 保持目录整洁，临时材料放 `.tmp_everos_pressure/` 或本 note。

## 已读关键事实

- `problems.md` 活跃项：P2-04 raw_message delete/filter Cloud limitation；P2-05 flush-heavy 后异步 extraction 可能晚到；P2-06 长消息 batch_size 15/20 可能 Cloud 403，batch_size=5 成功；P2-07 workflow dry-run 未校验 timestamp epoch-ms。
- 大规模成功压测：已有非默认 user `hermes_mcp_stress_main_20260512_125809`；成功 session `stress-session-existing-user-large-20260513_001736`；约 289 条 synthetic messages；13/13 MCP tools；27/27 passed；structured cleanup 空；raw residual 仍存在。
- `hermes-agent` skill 参考要求：Python 与 Rust surfaces 语义一致；batch/import workflow 应处理 timestamp epoch-ms、长消息 Cloud 403、小 batch 建议；真实 Cloud raw cleanup 只能报告。
- 当前 git 状态已有既有未提交改动（README、Rust context parity、tests、升级note 等）；本轮不得误回滚。

## 当前计划

1. Python RED：给 workflow dry-run timestamp 校验、payload metrics、adaptive split-on-403 写失败测试。
2. Python GREEN：实现最小 helper/validator/分批重试逻辑；更新 schemas/tool 描述。
3. Python 反思/补洞：运行相关测试，检查边界。
4. Rust RED/GREEN：同步 timestamp 校验、payload metrics、split-on-403，保持 Python parity。
5. 文档/问题台账：更新 `problems.md` 状态和本 note。
6. 验证：Python targeted/full；Rust fmt/clippy/tests；diff check；清理临时噪声。

## 进展日志

- 00:41:48 创建本轮 note；尚未改代码。
- 00:46:06 读取 Python `workflows.py`/`client.py`/`schemas.py` 与 provider/MCP tests：
  - `schemas.validate_messages()` 已要求 `timestamp` 为非 bool 整数 epoch-ms，真实 `client.add_memories()` 会在 HTTP 前拒绝 ISO timestamp。
  - workflow 缺口：`normalize_import_messages()` 只把 timestamp 原样保留；`dry_run` 不调用 `validate_messages()`，因此不会提前暴露 ISO timestamp。
  - workflow 缺口：`import_and_verify()` 遇到 `client.add_memories()` 403/Forbidden 只记录整批失败，不会 split-on-403。
  - provider/MCP 的 `batch_ingest` 复用 `import_and_verify(workflow="batch_ingest")`，Python 修该 helper 即可覆盖 provider 与 MCP。
- 00:48:23 Python RED 完成：新增 `tests/test_workflows.py` 三个失败测试，分别覆盖 dry-run timestamp warning+metrics、真实写入前 timestamp validation_failed、Cloud 403 batch adaptive split。失败原因均为预期功能缺失。
- 00:54:09 Python GREEN 完成：`workflows.py` 增加 timestamp warning、metrics、validation_failed、403 adaptive split；`python -m pytest -q tests/test_workflows.py tests/test_schemas.py tests/test_everos_client.py tests/test_everos_provider.py tests/test_everos_mcp_server.py` 通过 59 passed。
- 00:56:58 读取 Rust `workflows.rs`/`client.rs`/`parity.rs`：
  - Rust client 已在 `validate_messages()` 中要求 timestamp 为 integer epoch-ms，底层 HTTP 前会拒绝 ISO timestamp。
  - Rust workflow 与旧 Python 一样：`normalize_import_messages()` 不 warning 非整数 timestamp，dry-run 不调用 validation，不输出 metrics。
  - Rust `import_and_verify()` 遇到 403 只把整批记 failed，不 split；输出也没有 `split_count`/`metrics`/`payload_bytes`。
  - 可改点集中在 `rust-version/src/workflows.rs`，现有 MCP/provider workflow tests 在 `rust-version/tests/parity.rs`。
- 01:01:11 Rust RED 完成：增强 dry-run workflow test，新增 `mcp_batch_ingest_splits_cloud_403_batches`；两者分别失败于 metrics 缺失与 403 不 split。
- 01:06:40 Rust GREEN targeted 完成：`workflows.rs` 增加 timestamp warning、metrics、validation_failed timestamp blocking、403 adaptive split；两个目标 Rust tests 均通过。
- 01:07:29 验证：`cargo fmt --all --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test --tests --no-fail-fast` 通过（39 Rust tests）；`python -m pytest tests -q` 通过 95 passed。
- 01:13:39 文档/台账/skill 参考已同步：README、rust-version README、Cloud v1 contract、`problems.md`、`hermes-agent` skill pressure reference。最终仓库级检查 `git diff --check`、Python py_compile、Rust fmt check 均通过。

## 信心循环

- 当前信心：100%。已完成 Python 优先、Rust 同步、文档台账同步与最终验证；当前剩余的 P2-04/P2-05 是 EverOS Cloud/API 或压测流程语义，不适合在客户端过度设计。
- 已执行修复闭环：
  - Python/Rust workflow dry-run 对非整数 timestamp 给 warning；真实执行前返回 `validation_failed`，不再把 ISO timestamp 送到 Cloud。
  - Python/Rust import workflows 输出 `metrics`、`payload_bytes`、`split_count`，遇到 multi-message Cloud 403 自动二分 split 并重试。
  - primitive client 行为未改，避免对低层 API 造成副作用。
  - `problems.md` 已把 P2-07 标为已解决、P2-06 标为 Cloud limitation + Hermes mitigation。
  - README/contract/skill 参考均已更新。
- 验证证据：Python full 95 passed；Rust fmt/clippy/39 tests passed；py_compile passed；git diff --check passed。
