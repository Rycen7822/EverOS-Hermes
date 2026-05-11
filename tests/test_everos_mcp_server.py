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
        "method": "hybrid",
        "memory_types": ["episodic_memory", "profile"],
        "top_k": 5,
        "include_original_data": False,
    }
    assert json.loads(raw)["data"]["episodes"][0]["summary"] == "Coffee preference"


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
    assert calls[0][0] == "add"
    assert calls[0][1]["user_id"] == "u1"
    assert calls[0][1]["session_id"] == "sess-1"
    assert calls[0][1]["messages"][0]["role"] == "user"
    assert calls[1] == ("flush", {"user_id": "u1", "session_id": "sess-1", "agent": False})


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
