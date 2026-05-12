# EverOS-Hermes 升级执行清单：对齐 EverOS Cloud v1（排除 Group 与 Multimodal）

生成日期：2026-05-12
适用仓库：`Rycen7822/EverOS-Hermes`
目标：在**不实现 group memory、group/sender CRUD、multimodal object signing/upload** 的前提下，将 EverOS-Hermes 升级为对 EverOS Cloud v1 的 **personal memory、agent memory、search/get/delete、task、settings、Hermes provider 自动记忆** 的高一致性实现。

---

## 0. 范围界定

### 0.1 本轮明确纳入

- EverOS Cloud v1 base URL、Bearer auth、v1 endpoint 形状。
- Personal memory：add / flush / get / search / delete。
- Agent memory：add / flush / search / get。
- Search/Get 的官方 filters DSL、memory type、retrieval method、pagination、ranking、radius、top_k 语义。
- Delete 的 single delete 与 batch delete 安全约束。
- Task status：异步任务状态查询与 queued / accumulated / flushed / searchable 状态表达。
- Settings：get / update 的 schema、校验、diff 与文档化。
- Hermes MCP server：显式工具层。
- Hermes memory provider：自动 recall、自动 capture、explicit provider tools。
- Python 版本与 Rust 版本的 parity。
- Mock conformance tests、provider tests、MCP tools/list smoke tests、可选 live tests。

### 0.2 本轮明确排除

- Group memory：`/api/v1/memories/group`、`/api/v1/memories/group/flush`。
- Groups CRUD：`/api/v1/groups`。
- Senders CRUD：`/api/v1/senders`。
- Multimodal storage：`/api/v1/object/sign`、S3 upload、`objectKey` content item 自动封装。
- 多参与者 sender attribution。

> 这三个领域后续可以作为 `v0.3+` 的独立 milestone。当前升级不要把 group/sender/object 参数混入默认工具签名，以免造成“不完整支持但对外宣称支持”的语义风险。

---

## 1. 事实基线与设计判断

### 1.1 EverOS Cloud v1 的相关能力面

EverOS Cloud 文档把 Cloud 描述为 production-grade managed infrastructure，用于给 AI agent 提供 persistent、evolving memory。其工作流包括 episodic trace formation、structured encoding、semantic consolidation、profile evolution、reconstructive recollection 与 grounded reasoning。对 Hermes 来说，关键不是把 Cloud 当普通 CRUD 数据库，而是把写入、抽取、合并、检索之间的异步生命周期暴露清楚。

Cloud v1 的基础要求：

- Base URL：`https://api.evermind.ai`。
- Auth：`Authorization: Bearer <api-key>`。
- v0 deprecated，新集成应使用 v1。
- extraction 默认可能异步；flush 可以触发边界检测与抽取；search 结果可能不会立刻反映最新写入。

本轮只覆盖以下 endpoint：

| 能力 | Cloud endpoint | 当前升级目标 |
|---|---|---|
| Add personal memories | `POST /api/v1/memories` | 完整支持，含 schema 校验、状态返回、flush 联动 |
| Flush personal memories | `POST /api/v1/memories/flush` | 完整支持，含 timeout guidance |
| Add agent memories | `POST /api/v1/memories/agent` | 完整支持显式写入；补齐 provider 自动 trajectory capture |
| Flush agent memories | `POST /api/v1/memories/agent/flush` | MCP + provider 都支持 |
| Get memories | `POST /api/v1/memories/get` | 补齐 filters、rank_by、rank_order、类型映射 |
| Search memories | `POST /api/v1/memories/search` | 补齐 filters、radius、top_k=-1、raw_message/agent_memory 可选搜索 |
| Delete memories | `POST /api/v1/memories/delete` | 强化 single/batch 互斥和 destructive confirmation |
| Get task status | `GET /api/v1/tasks/{task_id}` | 已有；补齐状态解释与 write/flush 返回联动 |
| Get settings | `GET /api/v1/settings` | 已有；补齐格式化与文档 |
| Update settings | `PUT /api/v1/settings` | 已有；补齐 whitelist / schema / diff |

### 1.2 当前 EverOS-Hermes 的相关状态

基于 README 与源码静态审查，当前 EverOS-Hermes 已具备较好的最小核心路径：

- Python client 已使用 `https://api.evermind.ai`、Bearer header 和 JSON request。
- Python client 已有 personal/agent add、personal/agent flush、get、search、delete、task、settings。
- Python client 的 `get_memories` / `search_memories` 底层已经接受 `filters`、`group_id`、`radius`、`rank_by`、`rank_order` 等参数；但是 MCP 和 provider 并未完整暴露。
- MCP server 目前暴露 9 个工具：save、add、flush、search、get、delete、task status、get settings、update settings。
- MCP search 当前主要暴露 `query/user_id/session_id/method/top_k/memory_types/include_original_data/include_vectors/response_format`，缺少 `filters`、`radius`、`timeout`。
- MCP get 当前主要暴露 `user_id/session_id/memory_type/page/page_size/response_format`，缺少 `filters`、`rank_by`、`rank_order`。
- Provider 当前支持 `prefetch`、`sync_turn`、`on_memory_write`、`on_session_end`，但 `sync_turn` 与 explicit provider tools 仍按 personal memory 写入；`capture_agent_memory` 配置项存在但未形成自动 agent trajectory capture。
- Rust version README 声称 Rust 版本与 Python 版本 feature parity，包含同样 9 个 MCP tools 与 provider behavior。因此本次升级必须同步改 Python 与 Rust，否则 release 后会出现 runtime 行为分裂。

### 1.3 本轮升级的核心判断

排除 group 与 multimodal 后，EverOS-Hermes 的关键缺口不是“endpoint 缺失”，而是：

1. **Cloud 参数语义未完整上浮到 MCP/provider**：filters DSL、radius、top_k=-1、rank_by/rank_order、agent memory 类型映射。
2. **agent memory 没有自动生命周期**：显式 `agent=True` 可用，但 provider 不会自动把任务轨迹、tool 结果、错误修正、成功方案写入 `/memories/agent`。
3. **delete 与 settings 安全性不足**：delete 需要严格 single/batch 互斥；settings update 不应任意透传未知字段。
4. **异步 memory lifecycle 表达不充分**：`saved=true` 不等于 structured memory 可检索；task_id、request_id、flush status、searchability 应在响应中显式区分。
5. **测试缺少 contract coverage**：应从 Cloud v1 endpoint/parameter 语义反推 client、MCP、provider、Rust parity 的测试矩阵。

---

## 2. 目标架构

### 2.1 双接口面：MCP tools 与 MemoryProvider 分离

保持现有双接口设计，但每个接口都要对齐 Cloud v1 的 personal/agent 能力。

```text
Hermes agent
├── MCP explicit tools
│   ├── everos_save_memory
│   ├── everos_add_memories
│   ├── everos_flush_memories
│   ├── everos_search_memories
│   ├── everos_get_memories
│   ├── everos_delete_memories
│   ├── everos_get_task_status
│   ├── everos_get_settings
│   └── everos_update_settings
│
└── MemoryProvider hooks
    ├── prefetch(query)
    ├── sync_turn(user, assistant, session_id)
    ├── on_memory_write(action, target, content, metadata)
    ├── on_session_end(messages)
    └── provider tools
        ├── everos_memory_save
        ├── everos_memory_search
        ├── everos_memory_get
        ├── everos_memory_flush
        └── everos_memory_forget
```

### 2.2 Memory lifecycle state contract

引入统一的返回状态结构，避免 agent 把写入成功误判为已可检索。

```json
{
  "ok": true,
  "scope": "personal | agent",
  "user_id": "...",
  "session_id": "...",
  "message_queued": true,
  "extraction_requested": true,
  "task_id": "... or null",
  "flush": {
    "ok": true,
    "request_id": "... or null",
    "status": "no_extraction | queued | processing | success | unknown",
    "message": "..."
  },
  "searchable": null,
  "next_actions": [
    "search after extraction completes",
    "poll everos_get_task_status if task_id exists"
  ]
}
```

原则：

- `saved=true` 或 `message_queued=true` 只代表 Cloud 接受消息。
- `extraction_requested=true` 代表 async task 或 flush 已触发。
- `searchable=null` 是默认值；只有 live search 命中目标记忆后才可设为 true。
- timeout 不等价失败；应给出 retryable guidance，并建议先 search/status 检查。

### 2.3 Memory scope contract

本轮只允许两个 scope：

```text
personal: /api/v1/memories, /api/v1/memories/flush
agent:    /api/v1/memories/agent, /api/v1/memories/agent/flush
```

禁止在当前 release 对外暴露 `group_id`、`sender_id`、`objectKey`、`objectList`，除非以 `experimental_unsupported` 明确标识并默认关闭。

---

## 3. 优先级 P0：Cloud v1 contract 固化

### P0.1 建立 contract 文档

新增：`docs/everos_cloud_v1_contract.md`

内容必须覆盖：

- Base URL/Auth/v1。
- Endpoint whitelist：personal、agent、get、search、delete、task、settings。
- Out-of-scope endpoint blacklist：group、senders、storage。
- Request body schema。
- Response body 关键字段。
- Memory type 映射。
- Filter DSL 规则。
- Delete mode 规则。
- Async lifecycle 状态规则。

验收：

- [ ] 文档中每个 endpoint 都有 method/path/body/response/status note。
- [ ] 文档中明确写出本轮不覆盖 group/multimodal。
- [ ] README 中的 claim 与该 contract 一致。

### P0.2 新增 schema/contract 模块

新增 Python 模块：`src/everos_hermes/schemas.py` 或 `src/everos_hermes/cloud_contract.py`

建议定义：

```python
from typing import Literal, TypedDict, Any

RetrievalMethod = Literal["keyword", "vector", "hybrid", "agentic"]
SearchMemoryType = Literal[
    "episodic_memory",
    "profile",
    "raw_message",
    "agent_memory",
]
GetMemoryType = Literal[
    "episodic_memory",
    "profile",
    "agent_case",
    "agent_skill",
]
RankOrder = Literal["asc", "desc"]
MemoryScope = Literal["personal", "agent"]
```

校验函数：

- `validate_messages(messages, scope)`
- `validate_search_params(method, memory_types, top_k, radius)`
- `validate_get_params(memory_type, page, page_size, rank_by, rank_order)`
- `validate_filters(filters, require_user=True)`
- `validate_delete_request(memory_id, user_id, session_id)`
- `validate_settings_update(settings, strict=True)`

验收：

- [ ] 所有 MCP/provider 输入先过 schema 校验，再发请求。
- [ ] client 层也做基本校验，避免只在工具层安全。
- [ ] Rust 中同步建立 enum/validator，或用等价类型与测试保障。

### P0.3 规范 memory type 映射

Cloud search 与 get 的 memory type 命名不完全相同，应显式区分：

| 操作 | 可用类型 | 说明 |
|---|---|---|
| search | `episodic_memory`, `profile`, `raw_message`, `agent_memory` | `agent_memory` 返回 cases 与 skills 聚合结构 |
| get | `episodic_memory`, `profile`, `agent_case`, `agent_skill` | `agent_case` 与 `agent_skill` 分开分页获取 |

执行项：

- [ ] 不允许把 `agent_case` / `agent_skill` 传给 search。
- [ ] 不允许把 `agent_memory` 传给 get。
- [ ] MCP docstring 中写清楚 search/get 类型差异。
- [ ] provider search schema 对 `memory_types` 设置 enum，而不是任意 string。

### P0.4 规范 filters DSL

本轮只支持 personal/agent，所以 filters 至少要能表达：

```json
{
  "user_id": "u1",
  "AND": [
    {"session_id": "s1"},
    {"timestamp": {"gte": 1700000000000}}
  ]
}
```

执行项：

- [ ] `user_id` 必须在 personal/agent 查询中出现。MCP 可用 default user 自动填充，但 batch delete 不能静默用 default user。
- [ ] 支持 `session_id` plain eq 与 operator object。
- [ ] 支持 `timestamp` 的 `eq/gt/gte/lt/lte`。
- [ ] 支持 `AND` / `OR` 嵌套数组。
- [ ] 禁止未知字段；虽然 Cloud 文档提到 unknown fields 可能被忽略，但 SDK/tool 层应拒绝，避免用户以为 filter 生效。
- [ ] `_build_filters()` 合并 `session_id` 时，不能破坏用户传入的 `AND/OR`；若用户同时在 filters 与 top-level 参数中设置冲突值，应报错而不是覆盖。

建议新增：

```python
def build_filters(
    *,
    user_id: str | None,
    session_id: str | None,
    filters: dict[str, Any] | None,
    require_user_id: bool = True,
) -> dict[str, Any]:
    # 1. validate allowlist
    # 2. detect conflict between filters["user_id"] and user_id
    # 3. detect conflict between filters session predicates and session_id if exact equality differs
    # 4. append session_id as AND only when not already specified
    # 5. ensure user_id exists when require_user_id
```

---

## 4. 优先级 P1：Python client 升级

当前 `EverOSClient` 已基本覆盖本轮 endpoint。P1 的重点是**参数校验、状态统一、生命周期表达**。

### P1.1 request layer

执行项：

- [ ] `request_json()` 返回结构中保留 HTTP status code，至少内部可访问。
- [ ] 对 204 No Content 返回 `{"ok": true, "status_code": 204}`，不要只返回 `{}`。
- [ ] timeout error 保持 retryable，但补充 operation/path/user_id/session_id/scope。
- [ ] HTTP error 中解析 Cloud 的 `code/message/request_id`，当前已有基础逻辑；补充 body snippet 截断与安全脱敏。
- [ ] 禁止在 error message 中泄露 API key、Authorization header。

建议：

```python
class EverOSResponse(TypedDict, total=False):
    ok: bool
    status_code: int
    data: dict[str, Any]
    request_id: str
```

### P1.2 add_memories

当前：`add_memories(user_id, messages, session_id=None, async_mode=True, agent=False)`。

执行项：

- [ ] 增加 `scope: Literal["personal", "agent"] | None`，并保留 `agent: bool` 作为 backward-compatible alias。
- [ ] 校验 `messages` 长度 1-500。
- [ ] 校验每条 message 至少包含 `role/timestamp/content`。
- [ ] personal scope role 建议只允许 `user/assistant/system`；agent scope 允许 `user/assistant/tool/system`。如果官方 schema 不限制 system，可作为 soft warning。
- [ ] timestamp 支持 int epoch ms；若用户传 ISO string，工具层可转换，但 client 层建议只接收最终 JSON。
- [ ] 返回统一 lifecycle payload，保留 Cloud 原始响应在 `raw` 字段。

验收测试：

- [ ] personal add 发 `POST /api/v1/memories`。
- [ ] agent add 发 `POST /api/v1/memories/agent`。
- [ ] `agent=True` 与 `scope="agent"` 行为一致。
- [ ] 空 messages 报错，不发请求。
- [ ] 501 条 messages 报错，不发请求。

### P1.3 flush_memories

当前：`flush_memories(user_id, session_id=None, agent=False, timeout=None)`。

执行项：

- [ ] 增加 `scope` 参数，保留 `agent` alias。
- [ ] personal flush 发 `/api/v1/memories/flush`。
- [ ] agent flush 发 `/api/v1/memories/agent/flush`。
- [ ] 返回 `_flush_result_payload()` 标准结构。
- [ ] timeout 时返回 retryable payload，不要让 provider 背景线程静默吞掉。

验收测试：

- [ ] `scope="personal"` request body 只有 `user_id/session_id`。
- [ ] `scope="agent"` path 正确。
- [ ] timeout guidance 包含“先 search/status 后 retry”。

### P1.4 search_memories

当前底层已有 `filters/radius/include_vectors`，但需校验和对 MCP/provider 上浮。

执行项：

- [ ] `top_k` 改为允许 `-1..100`。Python 已用 `int`，但 MCP/provider 当前可能 clamp 到 1-20；应区分 Cloud request 与 prompt context limit。
- [ ] `radius` 校验 `0 <= radius <= 1`；仅 vector/hybrid/agentic 相关，keyword 下传入 radius 时给 warning 或报错。
- [ ] `memory_types` 使用 SearchMemoryType enum。
- [ ] `filters` 必填最终必须有 `user_id`。
- [ ] `method="agentic"` 默认 timeout 提升到 60s；失败时可 fallback hybrid，返回中标记 `fallback_used=true`。
- [ ] `include_vectors=false` 默认 strip vectors；`include_original_data=true` 也不应默认泄露 embeddings。
- [ ] 增加 `response_format` 只在 MCP/provider 层处理，client 保持 JSON。

验收测试：

- [ ] `top_k=-1` 可通过 Python client。
- [ ] Rust client 不能再用 `u64 top_k` 阻止 `-1`。
- [ ] `radius=1.1` 报错。
- [ ] `memory_types=["agent_case"]` search 报错。
- [ ] filters 中缺 user_id 报错，除非显式 `allow_missing_user_id=True` 用于未来 group。

### P1.5 get_memories

当前底层已有 `filters/rank_by/rank_order`，MCP/provider 未暴露。

执行项：

- [ ] `memory_type` 使用 GetMemoryType enum。
- [ ] `page >= 1`。
- [ ] `1 <= page_size <= 100`。
- [ ] `rank_order in {"asc", "desc"}`。
- [ ] `rank_by` 建议默认 `timestamp`；允许 Cloud 支持的字段，未知字段报错或以 `unsafe_rank_by=True` 才允许。
- [ ] filters 必须有 user_id。
- [ ] `agent_case` / `agent_skill` 只能 user scope；本轮无 group，因此简单要求 user_id。

验收测试：

- [ ] `memory_type="agent_memory"` get 报错。
- [ ] `page_size=101` 报错。
- [ ] rank_order 大小写归一化。

### P1.6 delete_memories

当前 client 若有 `memory_id` 就只发 memory_id，否则发 user/group/session。问题是缺少严格互斥校验，且 MCP 层 batch delete 可能静默使用 default user。

执行项：

- [ ] Single delete：只允许 `memory_id`，同时传 `user_id/session_id` 必须报错。
- [ ] Batch delete：本轮只允许 `user_id`，可选 `session_id`；不支持 group/sender。
- [ ] Batch delete 不允许自动填充 default user，必须由调用方显式传入 `user_id`。
- [ ] Batch delete 工具层需要 `confirm=true` + `confirm_scope_text` 精确匹配。
- [ ] 删除请求返回 204 时转为 `{"ok": true, "deleted": true, "mode": ...}`。
- [ ] README 明确 destructive。

建议确认文本：

```text
delete user_id=<USER_ID> session_id=<SESSION_ID_OR_*>
```

验收测试：

- [ ] `memory_id` + `user_id` 报错且不发请求。
- [ ] batch delete 无 `user_id` 报错。
- [ ] `confirm=true` 但 confirm text 不匹配，报错。
- [ ] single delete 成功不要求 scope text，但必须 `confirm=true`。

### P1.7 task/status

当前已有 `get_task_status(task_id)`。

执行项：

- [ ] 在 add/flush 返回中如果存在 `task_id`，自动加入 `next_actions`。
- [ ] `everos_get_task_status` 输出解释 status：`processing/success/failed/unknown`。
- [ ] task status TTL 可能有限；过期时提示不能证明任务失败，只代表 status 不再可查。
- [ ] provider 背景写入如果拿到 task_id，应记录到 provider state，便于后续 debug。

### P1.8 settings

当前 `update_settings(settings)` 直接透传 dict。

执行项：

- [ ] `get_settings()` 格式化展示 `timezone/created_at/updated_at/llm_custom_setting`。
- [ ] `update_settings()` 默认 `strict=True`，只允许官方已确认字段，如 `timezone`、`llm_custom_setting`。示例中出现的 `extraction_mode` 需要以当前 schema 再确认；未确认前不要默认开放。
- [ ] `timezone` 校验 IANA timezone 形式；Python 可用 `zoneinfo.ZoneInfo` 测试。
- [ ] `llm_custom_setting` 必须为 object。
- [ ] update 前可选 fetch current，update 后 fetch updated，并返回 diff。
- [ ] 为未来字段提供 `unsafe_passthrough=False` 开关，仅开发时使用。

验收测试：

- [ ] `timezone="Asia/Tokyo"` 通过。
- [ ] `timezone="Tokyo"` 报错。
- [ ] 未知字段 strict 模式报错。
- [ ] `llm_custom_setting=[]` 报错。

---

## 5. 优先级 P2：MCP server 工具升级

### P2.1 工具清单保持 9 个，但参数扩展

不建议本轮新增大量工具。保留九工具，但补齐参数：

1. `everos_save_memory`
2. `everos_add_memories`
3. `everos_flush_memories`
4. `everos_search_memories`
5. `everos_get_memories`
6. `everos_delete_memories`
7. `everos_get_task_status`
8. `everos_get_settings`
9. `everos_update_settings`

### P2.2 `everos_save_memory`

目标：作为单条记忆便捷写入，支持 personal/agent scope。

建议签名：

```python
async def everos_save_memory(
    content: str,
    user_id: str | None = None,
    session_id: str | None = None,
    scope: Literal["personal", "agent"] = "personal",
    role: Literal["user", "assistant", "tool", "system"] = "user",
    flush: bool = True,
    async_mode: bool = True,
    flush_timeout: float | None = None,
) -> str:
```

执行项：

- [ ] `scope="personal"` 时 role 默认 `user`；`role="tool"` 报错。
- [ ] `scope="agent"` 时允许 `tool`，用于记录 tool result / failed action / correction。
- [ ] 输出 lifecycle payload，不只输出 raw Cloud response。
- [ ] docstring 明确 searchable unknown。

### P2.3 `everos_add_memories`

建议签名：

```python
async def everos_add_memories(
    messages: list[dict[str, Any]],
    user_id: str | None = None,
    session_id: str | None = None,
    scope: Literal["personal", "agent"] = "personal",
    async_mode: bool = True,
    flush: bool = False,
    flush_timeout: float | None = None,
) -> str:
```

兼容：

- [ ] 保留旧参数 `agent: bool | None = None`，但如果同时传 `scope` 和 `agent` 冲突则报错。
- [ ] README 中标注 `agent` alias deprecated，建议用 `scope`。

执行项：

- [ ] 对 messages 做 1-500 校验。
- [ ] timestamp 缺失时可选自动填充；建议默认不自动填充，避免 agent 以为原始日志完整。如果要自动填充，返回中标记 `timestamp_autofilled=true`。
- [ ] flush 结果合并到 add response。

### P2.4 `everos_flush_memories`

建议签名：

```python
async def everos_flush_memories(
    user_id: str | None = None,
    session_id: str | None = None,
    scope: Literal["personal", "agent"] = "personal",
    timeout: float | None = None,
) -> str:
```

执行项：

- [ ] 保留 `agent` alias。
- [ ] agent flush 调 `/api/v1/memories/agent/flush`。
- [ ] timeout payload 含 retryable 与 next actions。

### P2.5 `everos_search_memories`

当前缺口最大。

建议签名：

```python
async def everos_search_memories(
    query: str,
    user_id: str | None = None,
    session_id: str | None = None,
    filters: dict[str, Any] | None = None,
    method: Literal["keyword", "vector", "hybrid", "agentic"] = "hybrid",
    top_k: int = 5,
    memory_types: list[Literal["episodic_memory", "profile", "raw_message", "agent_memory"]] | None = None,
    radius: float | None = None,
    include_original_data: bool = False,
    include_vectors: bool = False,
    response_format: Literal["json", "markdown"] = "json",
    timeout: float | None = None,
    fallback_to_hybrid: bool = True,
) -> str:
```

执行项：

- [ ] 暴露 `filters`。
- [ ] 暴露 `radius`。
- [ ] 支持 `top_k=-1`，但如果 `response_format="markdown"`，设置单独 `max_context_items`，避免一次塞满 prompt。
- [ ] `agentic` 默认 timeout 60s；超时可 fallback hybrid。
- [ ] 默认 memory_types 仍为 `episodic_memory/profile`，但 task-planning prompt 或用户明确要求 agent experience 时可建议加入 `agent_memory`。
- [ ] `raw_message` 默认不查，除非用户明确要求原始消息。
- [ ] 格式化输出中分区展示 episodes/profiles/raw_messages/agent_memory cases/skills。

验收测试：

- [ ] MCP schema 中存在 filters/radius/timeout/fallback_to_hybrid。
- [ ] `top_k=-1` schema 允许。
- [ ] `method="keyword"` + `radius=0.5` 触发校验错误或 warning。

### P2.6 `everos_get_memories`

建议签名：

```python
async def everos_get_memories(
    user_id: str | None = None,
    session_id: str | None = None,
    filters: dict[str, Any] | None = None,
    memory_type: Literal["episodic_memory", "profile", "agent_case", "agent_skill"] = "episodic_memory",
    page: int = 1,
    page_size: int = 20,
    rank_by: str = "timestamp",
    rank_order: Literal["asc", "desc"] = "desc",
    response_format: Literal["json", "markdown"] = "json",
) -> str:
```

执行项：

- [ ] 暴露 filters/rank_by/rank_order。
- [ ] 不再只靠 `user_id/session_id` 两个参数。
- [ ] get formatter 支持 agent_case/agent_skill。
- [ ] `page_size` 最大 100。

### P2.7 `everos_delete_memories`

建议签名：

```python
async def everos_delete_memories(
    memory_id: str | None = None,
    user_id: str | None = None,
    session_id: str | None = None,
    confirm: bool = False,
    confirm_scope_text: str | None = None,
) -> str:
```

执行项：

- [ ] Single delete：`memory_id` only；不能同时传 user/session。
- [ ] Batch delete：必须显式传 user_id，不使用 default_user_id。
- [ ] Batch delete：必须 `confirm=true` 且 `confirm_scope_text` 精确匹配。
- [ ] 对 destructive tool 保持 `destructiveHint=True`。
- [ ] 返回中包含 `mode: single | batch`。

### P2.8 `everos_get_task_status`

执行项：

- [ ] 格式化解释 processing/success/failed/unknown。
- [ ] 对 unknown/expired 状态提供后续动作：search、flush、检查 request_id。
- [ ] 保留 raw response。

### P2.9 `everos_update_settings`

建议签名：

```python
async def everos_update_settings(
    settings: dict[str, Any],
    strict: bool = True,
    return_diff: bool = True,
) -> str:
```

执行项：

- [ ] strict 默认开启。
- [ ] update 前后 diff。
- [ ] 未知字段错误信息中列出允许字段。

---

## 6. 优先级 P3：Provider 自动 agent memory 支持

这是本轮最有技术含量的部分。MCP 显式 `scope="agent"` 只能说明 agent 能手动写；真正的 EverOS-Hermes 升级应让 Hermes provider 能自动沉淀 agent cases/skills。

### P3.1 新增 provider 配置

当前配置已有：

```json
{
  "auto_recall": true,
  "auto_capture": true,
  "flush_after_turn": true,
  "search_method": "hybrid",
  "top_k": 5,
  "memory_types": ["episodic_memory", "profile"],
  "capture_agent_memory": false
}
```

建议升级为：

```json
{
  "auto_recall": true,
  "auto_capture": true,
  "flush_after_turn": true,
  "search_method": "hybrid",
  "top_k": 5,
  "memory_types": ["episodic_memory", "profile"],

  "capture_agent_memory": false,
  "agent_capture_mode": "parallel",
  "agent_recall": false,
  "agent_memory_types": ["agent_memory"],
  "agent_flush_after_turn": true,
  "agentic_timeout": 60.0,
  "max_context_items": 8
}
```

字段说明：

| 字段 | 默认 | 含义 |
|---|---:|---|
| `capture_agent_memory` | false | backward-compatible 开关 |
| `agent_capture_mode` | `parallel` | `parallel` 同时写 personal+agent；`agent_only` 只写 agent；`off` 禁用 |
| `agent_recall` | false | prefetch 时是否并行查 agent_memory |
| `agent_memory_types` | `["agent_memory"]` | search agent memory 的类型 |
| `agent_flush_after_turn` | true | 写 agent 后是否 flush agent scope |
| `agentic_timeout` | 60.0 | agentic retrieval 或 agent search 超时 |
| `max_context_items` | 8 | 注入 prompt 的最大记忆条数，区别于 Cloud top_k |

### P3.2 自动 agent trajectory capture

当前 `sync_turn()` 只写 personal：

```python
self._client.add_memories(..., agent=False)
self._client.flush_memories(..., agent=False)
```

升级逻辑：

```text
sync_turn(user_content, assistant_content, session_id)
├── clean/trivial filter
├── if auto_capture:
│   ├── personal capture: /memories, /memories/flush
│   └── if capture_agent_memory:
│       ├── build agent trajectory messages
│       ├── /memories/agent
│       └── /memories/agent/flush
└── record lifecycle status
```

建议 agent trajectory messages：

```json
[
  {
    "role": "user",
    "timestamp": 1710000000000,
    "content": "Task request: ..."
  },
  {
    "role": "assistant",
    "timestamp": 1710000000001,
    "content": "Agent response summary: ...\nOutcome: completed|partial|failed\nKey approach: ...\nReusable lesson: ..."
  }
]
```

如果 Hermes 后续能提供 tool call trace，则加入：

```json
{
  "role": "tool",
  "timestamp": 1710000000002,
  "content": "Tool: everos_search_memories\nInput summary: ...\nResult summary: ...\nFailure/retry: ..."
}
```

执行项：

- [ ] 新增 `_build_agent_trajectory_messages(user_content, assistant_content, metadata=None)`。
- [ ] 从 assistant_content 中剥离 `<everos-context>` / `<memory-context>`，避免把检索上下文又写回 agent memory。
- [ ] 对 trivial turn 不写 personal，也不写 agent。
- [ ] agent trajectory content 包含 task、approach、result、error/correction、reusable skill hint。
- [ ] agent 写入失败不阻断 personal 写入，但要记录 redacted error。
- [ ] 如果 personal 与 agent 都 flush，分别记录两个 flush 状态。

### P3.3 Provider prefetch 双路检索

当前 `prefetch()` 只按 `memory_types` 搜索 personal profile/episode。

升级：

```text
prefetch(query)
├── personal_search: memory_types=[episodic_memory, profile]
├── if agent_recall:
│   └── agent_search: memory_types=[agent_memory]
├── merge/rank/deduplicate
└── return <everos-context>...</everos-context>
```

执行项：

- [ ] 默认仍不开启 agent_recall，避免 prompt 噪音。
- [ ] 当 query 呈现 task-planning/debugging/code-fix 模式时，可自动启用 agent memory search，或提示用户配置开启。
- [ ] 格式化中区分：`User memory` 与 `Agent experience`。
- [ ] agentic retrieval 仅在复杂多跳问题时启用；失败 fallback hybrid。

### P3.4 Provider tools 升级

当前 provider tools：save/search/get/flush/forget。

执行项：

- [ ] `everos_memory_save` 增加 `scope: personal|agent`。
- [ ] `everos_memory_search` 增加 `filters/radius/memory_types/top_k/response_format`。
- [ ] `everos_memory_get` 增加 `filters/rank_by/rank_order`。
- [ ] `everos_memory_flush` 增加 `scope`。
- [ ] `everos_memory_forget` 保持 memory_id-only，避免 provider tool 做 batch delete。

### P3.5 Provider 背景线程可观测性

当前背景线程多处 `except Exception: return None`，这对研究和调试不够。

执行项：

- [ ] 新增 redacted debug log：`$HERMES_HOME/everos.log`，默认只记录 operation/status/request_id，不记录内容全文。
- [ ] 新增 provider 内部状态：last_write_status、last_flush_status、last_agent_write_status。
- [ ] provider tool `everos_memory_status` 可以后续新增；本轮若不想新增工具，可在 `system_prompt_block` 不暴露，只用于 debug CLI。
- [ ] Rust provider 同步实现。

---

## 7. 优先级 P4：Formatting 与 prompt 注入

### P4.1 Search result formatter

Search response 可能包含：

- `episodes`
- `profiles`
- `raw_messages`
- `agent_memory.cases`
- `agent_memory.skills`
- `original_data`

执行项：

- [ ] `format_search_context()` 支持上述所有字段。
- [ ] 默认不展示 raw message，除非用户显式搜索 raw_message。
- [ ] 默认不展示 vectors。
- [ ] agent cases/skills 单独分区，避免和用户 profile 混淆。
- [ ] 每条结果保留 memory id、timestamp、score/source type，如果 Cloud 返回。
- [ ] Markdown 输出使用短句、可扫描列表，避免大段 JSON 注入 prompt。

建议格式：

```md
<everos-context>
## User episodic memory
- [episode] ...

## User profile
- [profile] ...

## Agent experience
- [case] Prior task: ... Approach: ... Outcome: ...
- [skill] When debugging MCP timeout, first check task status before retry.
</everos-context>
```

### P4.2 Prompt 污染防护

执行项：

- [ ] 写入 memory 前剥离 `<everos-context>` 与 `<memory-context>`。
- [ ] 剥离 tool result 中的大型 JSON/vector 字段。
- [ ] 长 assistant response 做 summarization 或截断；建议单条 content 上限可配置，如 8k chars。
- [ ] 对 system prompt、secret、API key 做 redaction。

---

## 8. 优先级 P5：Rust parity

Rust README 声称 Rust port 与 Python 版本拥有同样 user-facing surfaces。因此每个 Python 改动必须同步 Rust。

### P5.1 Rust client

执行项：

- [ ] `top_k` 从 `u64` 改为 `i64`，允许 `-1`。
- [ ] 新增 validators：retrieval method、search memory types、get memory types、radius、page/page_size、delete mode。
- [ ] `delete_memories` single/batch 互斥。
- [ ] `request_json` 处理 204。
- [ ] timeout error 携带 operation/path。
- [ ] settings strict validation。

### P5.2 Rust MCP

执行项：

- [ ] tool schema 同步 filters/radius/rank/timeout/scope。
- [ ] delete confirmation 逻辑同步。
- [ ] search formatter 同步 agent memory 分区。
- [ ] `agentic` timeout/fallback 逻辑同步。

### P5.3 Rust provider

执行项：

- [ ] `capture_agent_memory` 真正控制 `/memories/agent` 写入。
- [ ] `agent_capture_mode`、`agent_recall`、`agent_flush_after_turn` 同步。
- [ ] provider shim 与 CLI helper 状态同步。
- [ ] redacted debug log 同步。

### P5.4 Rust packaging

执行项：

- [ ] 更新 `rust-version/README.md`。
- [ ] 更新 package release script，确保新配置文件示例打包。
- [ ] `cargo fmt --all --check`。
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`。
- [ ] `cargo test --tests --no-fail-fast`。

---

## 9. 优先级 P6：测试矩阵

### P6.1 Contract tests

新增：`tests/test_cloud_contract.py`

覆盖：

- [ ] endpoint whitelist 不含 group/sender/storage。
- [ ] personal add path。
- [ ] agent add path。
- [ ] personal flush path。
- [ ] agent flush path。
- [ ] get/search/delete/task/settings path。
- [ ] Cloud v1 path 全部以 `/api/v1` 开头。
- [ ] v0 path 不存在。

### P6.2 Client request snapshot tests

新增或扩展：`tests/test_everos_client.py`

Mock HTTP server 断言 method/path/body：

- [ ] `add_memories(scope="personal")`。
- [ ] `add_memories(scope="agent")`。
- [ ] `flush_memories(scope="personal")`。
- [ ] `flush_memories(scope="agent")`。
- [ ] `search_memories(filters={...}, top_k=-1, radius=0.5)`。
- [ ] `get_memories(memory_type="agent_case", rank_by="timestamp", rank_order="desc")`。
- [ ] `delete_memories(memory_id="...")`。
- [ ] `delete_memories(user_id="u", session_id="s")`。
- [ ] `get_task_status("task id/with space")` URL encoding。
- [ ] `update_settings({"timezone":"Asia/Tokyo"})`。

### P6.3 Validator tests

新增：`tests/test_schemas.py`

- [ ] search top_k `-1, 0, 5, 100` 通过。
- [ ] search top_k `-2, 101` 失败。
- [ ] radius `0, 0.5, 1` 通过。
- [ ] radius `-0.1, 1.1` 失败。
- [ ] search memory type `agent_memory` 通过。
- [ ] search memory type `agent_case` 失败。
- [ ] get memory type `agent_case` 通过。
- [ ] get memory type `agent_memory` 失败。
- [ ] filters unknown field 失败。
- [ ] filters missing user_id 失败。
- [ ] conflicting `user_id` between filters and param 失败。
- [ ] delete memory_id + user_id 失败。
- [ ] batch delete missing explicit user_id 失败。
- [ ] settings unknown field strict 失败。

### P6.4 MCP schema tests

扩展：`tests/test_everos_mcp_server.py`

- [ ] tools/list 仍为 9 个。
- [ ] `everos_search_memories` schema 包含 `filters/radius/timeout/fallback_to_hybrid`。
- [ ] `everos_get_memories` schema 包含 `filters/rank_by/rank_order`。
- [ ] `everos_save_memory` schema 包含 `scope`。
- [ ] `everos_flush_memories` schema 包含 `scope`。
- [ ] `everos_delete_memories` schema 包含 `confirm_scope_text`。
- [ ] destructiveHint 设置正确。

### P6.5 Provider tests

扩展：`tests/test_everos_provider.py`

- [ ] `capture_agent_memory=false` 时只写 personal。
- [ ] `capture_agent_memory=true` + `agent_capture_mode=parallel` 时写 personal + agent。
- [ ] agent 写入 path 是 `/api/v1/memories/agent`。
- [ ] agent flush path 是 `/api/v1/memories/agent/flush`。
- [ ] `agent_recall=true` 时 prefetch 并行查 `agent_memory`。
- [ ] `<everos-context>` 不会被写回 memory。
- [ ] trivial user turn 不写入。
- [ ] provider tool save with `scope=agent` 写 agent。
- [ ] provider tool flush with `scope=agent` flush agent。
- [ ] provider background error 被记录为 redacted log/status，而不是完全吞掉。

### P6.6 Rust parity tests

扩展：`rust-version/tests/parity.rs`

- [ ] Python/Rust tool schema snapshot 等价。
- [ ] Rust client top_k=-1。
- [ ] Rust delete safety。
- [ ] Rust provider agent capture。
- [ ] Rust stdio MCP initialize + tools/list + sample search request。

### P6.7 可选 live tests

新增：`tests/live/test_everos_cloud_live.py`，默认跳过。

触发条件：

```bash
EVEROS_LIVE_TEST=1 EVEROS_API_KEY=... pytest tests/live -q
```

覆盖最小 live flow：

- [ ] add personal async。
- [ ] flush personal。
- [ ] poll task 或等待。
- [ ] search personal。
- [ ] add agent。
- [ ] flush agent。
- [ ] search agent_memory。
- [ ] get profile / agent_case。
- [ ] delete test memory by memory_id 或 dedicated test user batch。

注意：live tests 必须使用 dedicated test user id，例如：

```text
hermes_live_test_<timestamp>
```

不得对真实用户默认 id 做 batch delete。

---

## 10. 优先级 P7：文档与 README claim

### P7.1 README 能力声明

当前不应写：

```text
fully supports all EverOS Cloud APIs
```

建议写：

```text
EverOS-Hermes supports EverOS Cloud v1 personal and agent memory workflows for Hermes Agent, including add, flush, search, get, delete, task status, and memory-space settings. Group-chat and multimodal storage APIs are intentionally out of scope for this release.
```

中文说明：

```text
EverOS-Hermes 当前对齐 EverOS Cloud v1 的 personal/agent memory、检索、读取、删除、任务状态与 settings 能力；group-chat 与 multimodal storage 暂不纳入本版本。
```

### P7.2 README 工具表更新

更新 MCP tools 表：

| Tool | 新增重点 |
|---|---|
| `everos_save_memory` | `scope=personal|agent` |
| `everos_add_memories` | `scope`, strict message schema |
| `everos_flush_memories` | `scope`, timeout lifecycle payload |
| `everos_search_memories` | `filters`, `radius`, `top_k=-1`, `agent_memory` |
| `everos_get_memories` | `filters`, `rank_by`, `rank_order`, `agent_case/agent_skill` |
| `everos_delete_memories` | strict delete mode, `confirm_scope_text` |
| `everos_get_task_status` | status explanation |
| `everos_get_settings` | formatted settings |
| `everos_update_settings` | strict schema + diff |

### P7.3 Provider 文档更新

新增：

- Agent memory capture 配置说明。
- `agent_capture_mode` 三种模式说明。
- `agent_recall` 何时开启。
- Memory lifecycle 状态说明。
- Timeout 不应盲目 retry 的说明。
- 删除工具安全约束。

### P7.4 Migration guide

新增：`docs/migration_v0_1_to_v0_2.md`

内容：

- `agent` 参数仍可用但建议改为 `scope`。
- search/get 新参数。
- delete batch 现在需要 explicit user_id + confirmation text。
- settings update strict 模式可能拒绝旧的 arbitrary dict。
- Rust prebuilt 用户需更新 binary 与 provider shim。

---

## 11. 优先级 P8：Release 与版本策略

### P8.1 版本建议

如果只扩展参数且兼容旧参数：`0.2.0`。
如果 delete batch 行为变严格并影响旧调用：仍建议 `0.2.0`，因为安全修复值得 minor bump，但 changelog 要明确 breaking-ish behavior。

### P8.2 Release checklist

- [ ] Python tests 全绿：`python -m pytest tests -q`。
- [ ] Rust tests 全绿：`cargo test --tests --no-fail-fast`。
- [ ] Rust clippy/fmt 全绿。
- [ ] README 与 rust-version README 同步。
- [ ] package release tarball 包含新 shim 与 docs。
- [ ] release note 明确 out-of-scope：group/multimodal。
- [ ] release note 明确 delete safety change。
- [ ] 手动验证：`hermes mcp test everos`。
- [ ] 手动验证 provider：fresh Hermes session + prefetch + sync_turn。

---

## 12. 建议执行顺序

### Sprint 1：Contract 与安全边界

- [ ] 新增 contract 文档。
- [ ] 新增 schema validators。
- [ ] Python client 接入 validators。
- [ ] Delete mode 安全修复。
- [ ] Settings strict validation。
- [ ] Client request snapshot tests。

验收：本阶段结束后，client 不再允许明显偏离 Cloud v1 的参数组合。

### Sprint 2：MCP 参数完整性

- [ ] search 暴露 filters/radius/top_k=-1/timeout/fallback。
- [ ] get 暴露 filters/rank_by/rank_order。
- [ ] save/add/flush 引入 scope。
- [ ] delete 引入 confirm_scope_text。
- [ ] MCP schema tests。
- [ ] README MCP tool 表更新。

验收：Hermes agent 通过 MCP 可以完整调用本轮范围内的 Cloud personal/agent/search/get/delete/task/settings 能力。

### Sprint 3：Provider agent memory

- [ ] provider config 扩展。
- [ ] `capture_agent_memory` 真正写 `/memories/agent`。
- [ ] agent flush 联动。
- [ ] agent_recall 双路搜索。
- [ ] provider redacted status/log。
- [ ] provider tests。

验收：开启 `capture_agent_memory=true` 后，Hermes 完成任务会自动沉淀 agent trajectory，并可通过 search `agent_memory` 检索到。

### Sprint 4：Rust parity

- [ ] Rust client validators。
- [ ] Rust top_k 改 i64。
- [ ] Rust MCP schema 同步。
- [ ] Rust provider agent capture。
- [ ] parity tests。
- [ ] packaging。

验收：Python 与 Rust 的 user-facing behavior 一致。

### Sprint 5：Live smoke 与 release

- [ ] 可选 live tests。
- [ ] Hermes MCP test。
- [ ] Hermes provider fresh session test。
- [ ] Changelog。
- [ ] Release asset。

---

## 13. 研究/论文角度的增强：Memory Capability Contract

如果目标是高质量系统论文，不建议只把升级描述为“补 API 参数”。更有 novelty 的抽象是 **Memory Capability Contract**：把 agent 对长期记忆系统的可用能力建模为 endpoint coverage、scope coverage、lifecycle state 与 retrieval affordance 的组合。

### 13.1 Contract 维度

```text
MemoryCapabilityContract
├── EndpointCoverage
│   ├── personal.add/flush
│   ├── agent.add/flush
│   ├── search/get/delete
│   ├── task/status
│   └── settings
├── ScopeCoverage
│   ├── personal
│   └── agent
├── LifecycleState
│   ├── queued
│   ├── accumulated
│   ├── extraction_requested
│   ├── task_processing
│   ├── extracted
│   └── searchable_unknown/searchable_confirmed
├── RetrievalAffordance
│   ├── keyword
│   ├── vector
│   ├── hybrid
│   └── agentic
└── SafetyPolicy
    ├── delete mode
    ├── settings strictness
    └── prompt contamination guard
```

### 13.2 对 Hermes 的意义

- Agent 不再把 memory 当“写入立即可见”的 KV store。
- Agent 可根据任务选择 personal profile、episodic memory、agent cases/skills。
- Agent 能理解 flush/task/search 三者关系，减少无效 retry。
- Provider 可以根据任务复杂度在 hybrid 与 agentic retrieval 之间切换。
- Delete 与 settings 成为带策略的 tool，而不是任意 REST pass-through。

### 13.3 可评估指标

- Memory write-to-search latency awareness：agent 是否能正确处理 queued 但未 searchable 的状态。
- Agent skill reuse rate：开启 agent_memory 后，复杂任务中检索到历史 agent case/skill 的比例。
- Hallucinated memory reduction：strict lifecycle + filters 后，agent 是否减少“我已经保存并可检索”的错误声明。
- Destructive action safety：delete 错误触发率。
- Tool contract conformance：Cloud spec 到 client/MCP/provider 的 coverage。

---

## 14. 最终验收标准

本轮升级完成后，应满足：

- [ ] 所有本轮纳入 endpoint 都有 client method、MCP tool path 或 provider path。
- [ ] group/multimodal 未被误宣称支持。
- [ ] Search 支持 filters、radius、top_k=-1、agent_memory、raw_message 可选。
- [ ] Get 支持 filters、rank_by、rank_order、agent_case、agent_skill。
- [ ] Provider `capture_agent_memory=true` 时确实写 `/api/v1/memories/agent` 并 flush agent。
- [ ] Delete single/batch 严格互斥，batch 不静默使用 default user。
- [ ] Settings update 默认 strict，未知字段不透传。
- [ ] Python 与 Rust parity tests 通过。
- [ ] README claim 准确。
- [ ] Optional live tests 在 dedicated test user 上通过。

---

## 15. 参考来源

- EverOS Cloud Overview: https://docs.evermind.ai/cloud/overview
- EverOS `llms.txt`: https://docs.evermind.ai/llms.txt
- EverOS API Overview: https://docs.evermind.ai/api-reference/introduction
- Add personal memories: https://docs.evermind.ai/api-reference/memories/add-personal-memories
- Add agent memories: https://docs.evermind.ai/api-reference/memories/add-agent-memories
- Flush personal memories: https://docs.evermind.ai/api-reference/memories/flush-personal-memories
- Flush agent memories: https://docs.evermind.ai/api-reference/memories/flush-agent-memories
- Search memories: https://docs.evermind.ai/api-reference/memories/search-memories
- Get memories: https://docs.evermind.ai/api-reference/memories/get-memories
- Delete memories: https://docs.evermind.ai/api-reference/memories/delete-memories
- Get task status: https://docs.evermind.ai/api-reference/tasks/get-task-status
- Get settings: https://docs.evermind.ai/api-reference/settings/get-memory-space-settings
- Update settings: https://docs.evermind.ai/api-reference/settings/update-memory-space-settings
- EverOS open-source repo: https://github.com/EverMind-AI/EverOS
- EverOS-Hermes repo: https://github.com/Rycen7822/EverOS-Hermes
