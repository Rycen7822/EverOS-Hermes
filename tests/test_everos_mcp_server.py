import asyncio
import json


def test_mcp_search_tool_calls_client_with_defaults(monkeypatch):
    from everos_hermes import mcp_server

    captured = {}

    class FakeClient:
        def search_memories(self, **kwargs):
            captured.update(kwargs)
            return {"data": {"episodes": [{"id": "ep1", "summary": "Coffee preference"}]}}

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr(mcp_server, "make_client", lambda: FakeClient())

    raw = asyncio.run(mcp_server.everos_search_memories(
        query="coffee",
        user_id=None,
        session_id=None,
        method="hybrid",
        top_k=5,
        memory_types=None,
        include_original_data=False,
        response_format="json",
    ))

    assert captured == {
        "query": "coffee",
        "user_id": "u1",
        "session_id": None,
        "filters": None,
        "method": "hybrid",
        "memory_types": ["episodic_memory", "profile"],
        "top_k": 5,
        "radius": None,
        "include_original_data": False,
        "include_vectors": False,
        "timeout": None,
    }
    assert json.loads(raw)["data"]["episodes"][0]["summary"] == "Coffee preference"


def test_mcp_search_strips_vectors_unless_explicitly_requested(monkeypatch):
    from everos_hermes import mcp_server

    class FakeClient:
        def search_memories(self, **kwargs):
            assert kwargs["include_original_data"] is True
            assert kwargs["include_vectors"] is False
            return {
                "data": {
                    "episodes": [{"id": "ep1", "summary": "Coffee", "vector": [0.1, 0.2]}],
                    "original_data": {"episodes": {"ep1": {"vector": [0.1, 0.2], "summary": "Coffee"}}},
                }
            }

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr(mcp_server, "make_client", lambda: FakeClient())

    raw = asyncio.run(mcp_server.everos_search_memories(
        query="coffee",
        include_original_data=True,
        include_vectors=False,
    ))

    rendered = json.dumps(json.loads(raw))
    assert "Coffee" in rendered
    assert "vector" not in rendered


def test_mcp_save_tool_adds_memory_and_flushes(monkeypatch):
    from everos_hermes import mcp_server

    calls = []

    class FakeClient:
        def add_memories(self, **kwargs):
            calls.append(("add", kwargs))
            return {"data": {"status": "queued", "task_id": "task-1"}}

        def flush_memories(self, **kwargs):
            calls.append(("flush", kwargs))
            return {"data": {"status": "extracted"}}

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr(mcp_server, "make_client", lambda: FakeClient())

    raw = asyncio.run(mcp_server.everos_save_memory(
        content="User prefers morning meetings before 10am.",
        user_id=None,
        session_id="sess-1",
        flush=True,
        async_mode=True,
    ))

    result = json.loads(raw)
    assert result["saved"] is True
    assert result["message_queued"] is True
    assert result["extraction_requested"] is True
    assert result["flush"]["status"] == "extracted"
    assert result["searchable"] is None
    assert calls[0][0] == "add"
    assert calls[0][1]["user_id"] == "u1"
    assert calls[0][1]["session_id"] == "sess-1"
    assert calls[0][1]["messages"][0]["role"] == "user"
    assert calls[1] == ("flush", {"user_id": "u1", "session_id": "sess-1", "scope": "personal", "timeout": None})


def test_mcp_save_tool_reports_flush_timeout_without_losing_task(monkeypatch):
    from everos_hermes import mcp_server
    from everos_hermes.client import EverOSTimeoutError

    class FakeClient:
        def add_memories(self, **kwargs):
            return {"data": {"status": "queued", "task_id": "task-1"}}

        def flush_memories(self, **kwargs):
            raise EverOSTimeoutError("EverOS request timed out; search before retrying")

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr(mcp_server, "make_client", lambda: FakeClient())

    raw = asyncio.run(mcp_server.everos_save_memory(
        content="User prefers morning meetings before 10am.",
        session_id="sess-1",
        flush=True,
    ))

    result = json.loads(raw)
    assert result["saved"] is True
    assert result["task_id"] == "task-1"
    assert result["flush"]["ok"] is False
    assert result["flush"]["retryable"] is True
    assert "search" in result["flush"]["suggested_next_actions"][0]


def test_mcp_flush_tool_passes_timeout_and_returns_actionable_timeout(monkeypatch):
    from everos_hermes import mcp_server
    from everos_hermes.client import EverOSTimeoutError

    captured = {}

    class FakeClient:
        def flush_memories(self, **kwargs):
            captured.update(kwargs)
            raise EverOSTimeoutError("EverOS request timed out; search before retrying")

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr(mcp_server, "make_client", lambda: FakeClient())

    raw = asyncio.run(mcp_server.everos_flush_memories(session_id="sess-1", timeout=45))

    result = json.loads(raw)
    assert captured == {"user_id": "u1", "session_id": "sess-1", "scope": "personal", "timeout": 45}
    assert result["ok"] is False
    assert result["retryable"] is True
    assert result["operation"] == "flush"


def test_mcp_make_client_and_user_id_read_hermes_dotenv(monkeypatch, tmp_path):
    from everos_hermes import mcp_server

    monkeypatch.delenv("EVEROS_API_KEY", raising=False)
    monkeypatch.delenv("EVEROS_USER_ID", raising=False)
    monkeypatch.delenv("EVEROS_BASE_URL", raising=False)
    monkeypatch.delenv("EVEROS_TIMEOUT", raising=False)
    monkeypatch.setenv("HERMES_HOME", str(tmp_path))
    (tmp_path / ".env").write_text(
        "EVEROS_API_KEY=sk-from-dotenv\n"
        "EVEROS_USER_ID=dotenv-user\n"
        "EVEROS_BASE_URL=https://everos.example.test\n"
        "EVEROS_TIMEOUT=4\n",
        encoding="utf-8",
    )

    client = mcp_server.make_client()

    assert client.api_key == "sk-from-dotenv"
    assert client.base_url == "https://everos.example.test"
    assert client.timeout == 4.0
    assert mcp_server.default_user_id() == "dotenv-user"


def test_mcp_server_exposes_expected_tool_names():
    from everos_hermes.mcp_server import TOOL_NAMES

    assert {
        "everos_save_memory",
        "everos_add_memories",
        "everos_flush_memories",
        "everos_search_memories",
        "everos_get_memories",
        "everos_delete_memories",
        "everos_get_task_status",
        "everos_get_settings",
    }.issubset(set(TOOL_NAMES))



def _mcp_tool_properties(name: str):
    from everos_hermes import mcp_server

    return mcp_server.mcp._tool_manager._tools[name].parameters["properties"]


def test_mcp_tool_schemas_expose_cloud_v1_parameters():
    from everos_hermes import mcp_server

    tools = mcp_server.mcp._tool_manager._tools
    assert len(tools) == 9
    assert "scope" in _mcp_tool_properties("everos_save_memory")
    assert "scope" in _mcp_tool_properties("everos_add_memories")
    assert "scope" in _mcp_tool_properties("everos_flush_memories")
    search_props = _mcp_tool_properties("everos_search_memories")
    for name in ["filters", "radius", "timeout", "fallback_to_hybrid"]:
        assert name in search_props
    get_props = _mcp_tool_properties("everos_get_memories")
    for name in ["filters", "rank_by", "rank_order"]:
        assert name in get_props
    assert "confirm_scope_text" in _mcp_tool_properties("everos_delete_memories")
    assert tools["everos_delete_memories"].annotations.destructiveHint is True


def test_mcp_save_memory_supports_agent_scope_and_tool_role(monkeypatch):
    from everos_hermes import mcp_server

    calls = []

    class FakeClient:
        def add_memories(self, **kwargs):
            calls.append(("add", kwargs))
            return {"data": {"status": "queued", "task_id": "task-1"}}

        def flush_memories(self, **kwargs):
            calls.append(("flush", kwargs))
            return {"data": {"status": "success"}}

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr(mcp_server, "make_client", lambda: FakeClient())

    raw = asyncio.run(mcp_server.everos_save_memory(
        content="Tool failed once, then retry with timeout=60 succeeded.",
        scope="agent",
        role="tool",
        session_id="sess-1",
        flush=True,
    ))

    result = json.loads(raw)
    assert result["scope"] == "agent"
    assert result["flush"]["status"] == "success"
    assert calls[0][1]["scope"] == "agent"
    assert calls[0][1]["messages"][0]["role"] == "tool"
    assert calls[1][1] == {"user_id": "u1", "session_id": "sess-1", "scope": "agent", "timeout": None}


def test_mcp_search_passes_filters_radius_timeout_and_fallback(monkeypatch):
    from everos_hermes import mcp_server
    from everos_hermes.client import EverOSTimeoutError

    calls = []

    class FakeClient:
        def search_memories(self, **kwargs):
            calls.append(kwargs)
            if kwargs["method"] == "agentic":
                raise EverOSTimeoutError("EverOS request timed out during search")
            return {"data": {"episodes": [{"summary": "fallback result"}]}}

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr(mcp_server, "make_client", lambda: FakeClient())

    raw = asyncio.run(mcp_server.everos_search_memories(
        query="debug timeout",
        filters={"AND": [{"timestamp": {"gte": 1700000000000}}]},
        method="agentic",
        top_k=-1,
        memory_types=["agent_memory"],
        radius=0.5,
        timeout=12,
        fallback_to_hybrid=True,
    ))

    result = json.loads(raw)
    assert result["fallback_used"] is True
    assert calls[0]["method"] == "agentic"
    assert calls[0]["filters"] == {"AND": [{"timestamp": {"gte": 1700000000000}}]}
    assert calls[0]["radius"] == 0.5
    assert calls[0]["top_k"] == -1
    assert calls[0]["timeout"] == 12
    assert calls[1]["method"] == "hybrid"


def test_mcp_get_passes_filters_rank_by_rank_order(monkeypatch):
    from everos_hermes import mcp_server

    captured = {}

    class FakeClient:
        def get_memories(self, **kwargs):
            captured.update(kwargs)
            return {"data": {"items": []}}

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr(mcp_server, "make_client", lambda: FakeClient())

    asyncio.run(mcp_server.everos_get_memories(
        filters={"AND": [{"timestamp": {"lte": 1800000000000}}]},
        memory_type="agent_case",
        rank_by="timestamp",
        rank_order="asc",
    ))

    assert captured == {
        "user_id": "u1",
        "session_id": None,
        "filters": {"AND": [{"timestamp": {"lte": 1800000000000}}]},
        "memory_type": "agent_case",
        "page": 1,
        "page_size": 20,
        "rank_by": "timestamp",
        "rank_order": "asc",
    }


def test_mcp_delete_requires_explicit_batch_confirmation(monkeypatch):
    from everos_hermes import mcp_server

    calls = []

    class FakeClient:
        def delete_memories(self, **kwargs):
            calls.append(kwargs)
            return {"ok": True, "deleted": True, "mode": "batch"}

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "default-user")
    monkeypatch.setattr(mcp_server, "make_client", lambda: FakeClient())

    no_user = asyncio.run(mcp_server.everos_delete_memories(confirm=True, confirm_scope_text="delete user_id=default-user session_id=*"))
    assert "explicit user_id" in json.loads(no_user)["error"]
    wrong_text = asyncio.run(mcp_server.everos_delete_memories(user_id="u1", session_id="s1", confirm=True, confirm_scope_text="delete user_id=u1 session_id=*"))
    assert "confirm_scope_text" in json.loads(wrong_text)["error"]

    ok = asyncio.run(mcp_server.everos_delete_memories(user_id="u1", session_id="s1", confirm=True, confirm_scope_text="delete user_id=u1 session_id=s1"))
    assert json.loads(ok)["mode"] == "batch"
    assert calls == [{"memory_id": None, "user_id": "u1", "session_id": "s1"}]


def test_mcp_update_settings_passes_strict_and_return_diff(monkeypatch):
    from everos_hermes import mcp_server

    captured = {}

    class FakeClient:
        def update_settings(self, settings, **kwargs):
            captured["settings"] = settings
            captured.update(kwargs)
            return {"data": {"timezone": "Asia/Tokyo"}, "diff": {"timezone": {"before": "UTC", "after": "Asia/Tokyo"}}}

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setattr(mcp_server, "make_client", lambda: FakeClient())

    raw = asyncio.run(mcp_server.everos_update_settings({"timezone": "Asia/Tokyo"}, strict=True, return_diff=True))

    assert captured == {"settings": {"timezone": "Asia/Tokyo"}, "strict": True, "return_diff": True}
    assert json.loads(raw)["diff"]["timezone"]["after"] == "Asia/Tokyo"
