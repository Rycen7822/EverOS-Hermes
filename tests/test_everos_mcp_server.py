import asyncio
import json

def test_mcp_search_tool_calls_client_with_defaults(monkeypatch, tmp_path):
    from everos_hermes import mcp_server

    captured = {}

    class FakeClient:
        def search_memories(self, **kwargs):
            captured.update(kwargs)
            return {"data": {"episodes": [{"id": "ep1", "summary": "Coffee preference"}]}}

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.delenv("EVEROS_USER_ID", raising=False)
    monkeypatch.setenv("HERMES_HOME", str(tmp_path))
    (tmp_path / ".env").write_text("EVEROS_USER_ID=u1\n", encoding="utf-8")
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

def test_mcp_save_memory_supports_agent_scope_tool_role_and_unchecked_visibility(monkeypatch):
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
        tool_call_id="tool-call-1",
        session_id="sess-1",
        flush=True,
    ))
    add_raw = asyncio.run(mcp_server.everos_add_memories(
        messages=[{"role": "assistant", "timestamp": 1711900000000, "content": "agent note", "message_id": "msg-agent-1"}],
        scope="agent",
        session_id="sess-agent",
        flush=False,
    ))

    result = json.loads(raw)
    add = json.loads(add_raw)
    assert result["scope"] == "agent"
    assert result["flush"]["status"] == "success"
    assert result["agent_visibility"]["agent_visibility_status"] == "unchecked"
    assert add["agent_visibility"]["agent_visibility_status"] == "unchecked"
    assert calls[0][1]["scope"] == "agent"
    assert calls[0][1]["messages"][0]["role"] == "tool"
    assert calls[0][1]["messages"][0]["tool_call_id"] == "tool-call-1"
    assert calls[1][1] == {"user_id": "u1", "session_id": "sess-1", "scope": "agent", "timeout": None}
    assert calls[2][1]["scope"] == "agent"
    assert calls[2][1]["messages"][0]["message_id"] == "msg-agent-1"

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
