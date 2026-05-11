import json


def test_provider_availability_requires_api_key(monkeypatch, tmp_path):
    from everos_hermes.provider import EverOSMemoryProvider

    monkeypatch.setenv("HERMES_HOME", str(tmp_path))
    monkeypatch.delenv("EVEROS_API_KEY", raising=False)
    assert EverOSMemoryProvider().is_available() is False

    (tmp_path / ".env").write_text("EVEROS_API_KEY=sk-from-dotenv\n", encoding="utf-8")
    assert EverOSMemoryProvider().is_available() is True

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    assert EverOSMemoryProvider().is_available() is True


def test_initialize_prefers_gateway_user_id(monkeypatch, tmp_path):
    from everos_hermes.provider import EverOSMemoryProvider

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.delenv("EVEROS_USER_ID", raising=False)
    provider = EverOSMemoryProvider()
    provider.initialize(
        session_id="sess-1",
        hermes_home=str(tmp_path),
        platform="telegram",
        user_id="tg-42",
        user_name="Xu",
        agent_identity="default",
    )

    assert provider._user_id == "tg-42"
    assert provider._session_id == "sess-1"
    assert provider._write_enabled is True


def test_prefetch_formats_episode_and_profile_context(monkeypatch, tmp_path):
    from everos_hermes.provider import EverOSMemoryProvider

    class FakeClient:
        def __init__(self, *args, **kwargs):
            pass

        def search_memories(self, **kwargs):
            assert kwargs["query"] == "coffee"
            assert kwargs["user_id"] == "u1"
            return {
                "data": {
                    "episodes": [
                        {
                            "id": "ep1",
                            "summary": "User said they prefer strong black Americano.",
                            "subject": "coffee preference",
                            "score": 0.91,
                        }
                    ],
                    "profiles": [
                        {
                            "id": "pr1",
                            "profile_data": {
                                "explicit_info": ["User likes black coffee"],
                                "implicit_traits": ["Prefers concise recommendations"],
                            },
                        }
                    ],
                }
            }

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr("everos_hermes.provider.EverOSClient", FakeClient)
    provider = EverOSMemoryProvider()
    provider.initialize(session_id="sess-1", hermes_home=str(tmp_path), platform="cli")

    context = provider.prefetch("coffee")

    assert "# EverOS Memory" in context
    assert "coffee preference" in context
    assert "strong black Americano" in context
    assert "User likes black coffee" in context


def test_sync_turn_adds_user_and_assistant_then_flushes(monkeypatch, tmp_path):
    from everos_hermes.provider import EverOSMemoryProvider

    calls = []

    class FakeClient:
        def __init__(self, *args, **kwargs):
            pass

        def add_memories(self, **kwargs):
            calls.append(("add", kwargs))
            return {"data": {"status": "queued", "task_id": "task-1"}}

        def flush_memories(self, **kwargs):
            calls.append(("flush", kwargs))
            return {"data": {"status": "extracted"}}

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr("everos_hermes.provider.EverOSClient", FakeClient)

    provider = EverOSMemoryProvider()
    provider.initialize(session_id="sess-1", hermes_home=str(tmp_path), platform="cli")
    provider.sync_turn("remember I like espresso", "Noted.", session_id="sess-2")
    provider.shutdown()

    assert calls[0][0] == "add"
    assert calls[0][1]["user_id"] == "u1"
    assert calls[0][1]["session_id"] == "sess-2"
    assert [m["role"] for m in calls[0][1]["messages"]] == ["user", "assistant"]
    assert calls[1] == ("flush", {"user_id": "u1", "session_id": "sess-2", "agent": False})


def test_memory_save_tool_returns_json_string_and_flushes(monkeypatch, tmp_path):
    from everos_hermes.provider import EverOSMemoryProvider

    calls = []

    class FakeClient:
        def __init__(self, *args, **kwargs):
            pass

        def add_memories(self, **kwargs):
            calls.append(("add", kwargs))
            return {"data": {"status": "queued", "task_id": "task-9"}}

        def flush_memories(self, **kwargs):
            calls.append(("flush", kwargs))
            return {"data": {"status": "extracted"}}

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr("everos_hermes.provider.EverOSClient", FakeClient)

    provider = EverOSMemoryProvider()
    provider.initialize(session_id="sess-1", hermes_home=str(tmp_path), platform="cli")
    raw = provider.handle_tool_call("everos_memory_save", {"content": "User prefers pytest.", "flush": True})

    result = json.loads(raw)
    assert result["saved"] is True
    assert result["message_queued"] is True
    assert result["extraction_requested"] is True
    assert result["flush"]["status"] == "extracted"
    assert result["searchable"] is None
    assert result["task_id"] == "task-9"
    assert calls[0][0] == "add"
    assert calls[1][0] == "flush"


def test_memory_flush_tool_accepts_timeout_and_reports_timeout(monkeypatch, tmp_path):
    from everos_hermes.client import EverOSTimeoutError
    from everos_hermes.provider import EverOSMemoryProvider

    calls = []

    class FakeClient:
        def __init__(self, *args, **kwargs):
            pass

        def flush_memories(self, **kwargs):
            calls.append(kwargs)
            raise EverOSTimeoutError("EverOS request timed out; search before retrying")

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr("everos_hermes.provider.EverOSClient", FakeClient)

    provider = EverOSMemoryProvider()
    provider.initialize(session_id="sess-1", hermes_home=str(tmp_path), platform="cli")
    raw = provider.handle_tool_call("everos_memory_flush", {"session_id": "sess-2", "timeout": 45})

    result = json.loads(raw)
    assert calls == [{"user_id": "u1", "session_id": "sess-2", "agent": False, "timeout": 45}]
    assert result["ok"] is False
    assert result["retryable"] is True
    assert result["operation"] == "flush"
