import json


def test_provider_agent_visibility_config_normalizes_defaults_and_overrides():
    from everos_hermes.provider import _normalize_config

    defaults = _normalize_config({})
    assert defaults["agent_visibility_verify_after_write"] is False
    assert defaults["agent_visibility_verify_after_flush"] is False
    assert defaults["agent_visibility_queries"] == []
    assert defaults["agent_visibility_top_k"] == 5
    assert defaults["agent_visibility_timeout"] == 30.0
    assert defaults["agent_visibility_get_page_size"] == 20
    assert defaults["agent_visibility_retry_flush_attempts"] == 1

    custom = _normalize_config(
        {
            "agent_visibility_verify_after_write": "true",
            "agent_visibility_verify_after_flush": "yes",
            "agent_visibility_queries": "alpha, beta",
            "agent_visibility_top_k": 99,
            "agent_visibility_timeout": 0,
            "agent_visibility_get_page_size": 200,
            "agent_visibility_retry_flush_attempts": 9,
        }
    )
    assert custom["agent_visibility_verify_after_write"] is True
    assert custom["agent_visibility_verify_after_flush"] is True
    assert custom["agent_visibility_queries"] == ["alpha", "beta"]
    assert custom["agent_visibility_top_k"] == 20
    assert custom["agent_visibility_timeout"] == 1.0
    assert custom["agent_visibility_get_page_size"] == 100
    assert custom["agent_visibility_retry_flush_attempts"] == 5



def test_provider_config_contract_clamps_drift_prone_fields():
    from pathlib import Path

    from everos_hermes.provider import _normalize_config

    contract = json.loads((Path(__file__).parent / "contracts" / "provider_config_contract.json").read_text(encoding="utf-8"))
    fields = contract["fields"]

    defaults = _normalize_config({})
    for key, spec in fields.items():
        assert defaults[key] == spec["default"]

    below_min = _normalize_config({key: 0 for key, spec in fields.items() if spec["min"] > 0})
    for key, spec in fields.items():
        if spec["min"] > 0:
            assert below_min[key] == spec["min"]

    above_max = _normalize_config({key: spec["max"] + 1 for key, spec in fields.items()})
    for key, spec in fields.items():
        assert above_max[key] == spec["max"]


def test_save_config_drops_api_key_and_uses_private_permissions(tmp_path):
    from everos_hermes.provider import EverOSMemoryProvider, _load_config

    provider = EverOSMemoryProvider()
    provider.save_config({"api_key": "secret-config-key", "user_id": "u1", "base_url": "https://example.test"}, str(tmp_path))

    config_path = tmp_path / "everos.json"
    text = config_path.read_text(encoding="utf-8")
    assert "secret-config-key" not in text
    assert "api_key" not in text
    assert _load_config(str(tmp_path))["user_id"] == "u1"
    assert config_path.stat().st_mode & 0o777 == 0o600



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





def test_sync_turn_capture_agent_memory_parallel_writes_agent_and_strips_context(monkeypatch, tmp_path):
    from everos_hermes.provider import EverOSMemoryProvider

    calls = []

    class FakeClient:
        def __init__(self, *args, **kwargs):
            pass

        def add_memories(self, **kwargs):
            calls.append(("add", kwargs))
            return {"data": {"status": "queued", "task_id": f"task-{kwargs['scope']}"}}

        def flush_memories(self, **kwargs):
            calls.append(("flush", kwargs))
            return {"data": {"status": "success"}}

    (tmp_path / "everos.json").write_text(
        '{"capture_agent_memory": true, "agent_capture_mode": "parallel", "agent_flush_after_turn": true}\n',
        encoding="utf-8",
    )
    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr("everos_hermes.provider.EverOSClient", FakeClient)
    provider = EverOSMemoryProvider()
    provider.initialize(session_id="sess-1", hermes_home=str(tmp_path), platform="cli")
    provider.sync_turn(
        "please fix the MCP timeout",
        "<everos-context>old memory must not be recaptured</everos-context>Fixed by adding timeout handling.",
        session_id="sess-2",
    )
    provider.shutdown()

    add_scopes = [call[1]["scope"] for call in calls if call[0] == "add"]
    flush_scopes = [call[1]["scope"] for call in calls if call[0] == "flush"]
    assert add_scopes == ["personal", "agent"]
    assert flush_scopes == ["personal", "agent"]
    agent_messages = [call[1]["messages"] for call in calls if call[0] == "add" and call[1]["scope"] == "agent"][0]
    rendered = json.dumps(agent_messages)
    assert "Task request: please fix the MCP timeout" in rendered
    assert "old memory must not be recaptured" not in rendered
    assert provider._last_agent_write_status["ok"] is True




def test_provider_tools_pass_scope_flush_timeout_and_visibility(monkeypatch, tmp_path):
    from everos_hermes.provider import EverOSMemoryProvider

    calls = []

    class FakeClient:
        def __init__(self, *args, **kwargs):
            pass

        def add_memories(self, **kwargs):
            calls.append(("add", kwargs))
            return {"data": {"status": "queued", "task_id": "task-agent"}}

        def flush_memories(self, **kwargs):
            calls.append(("flush", kwargs))
            return {"data": {"status": "success", "task_id": "flush-agent"}}

        def search_memories(self, **kwargs):
            calls.append(("search", kwargs))
            return {"data": {"agent_memory": []}}

        def get_memories(self, **kwargs):
            calls.append(("get", kwargs))
            return {"data": {"items": []}}

    (tmp_path / "everos.json").write_text('{"agent_visibility_verify_after_flush": true}\n', encoding="utf-8")
    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr("everos_hermes.provider.EverOSClient", FakeClient)
    provider = EverOSMemoryProvider()
    provider.initialize(session_id="sess-1", hermes_home=str(tmp_path), platform="cli")

    save = json.loads(provider.handle_tool_call("everos_memory_save", {"content": "retry with timeout", "scope": "agent", "flush": True}))
    provider.handle_tool_call("everos_memory_flush", {"scope": "agent", "timeout": 45})

    assert save["scope"] == "agent"
    assert save["agent_visibility"]["agent_visibility_status"] == "not_visible"
    assert provider._last_agent_visibility_status["agent_visibility_status"] == "not_visible"
    assert calls[0][1]["scope"] == "agent"
    assert calls[0][1]["messages"][0]["role"] == "assistant"
    assert calls[-1][1] == {"user_id": "u1", "session_id": "sess-1", "scope": "agent", "timeout": 45}

def test_provider_background_error_records_redacted_log_and_status(monkeypatch, tmp_path):
    from everos_hermes.provider import EverOSMemoryProvider

    class FakeClient:
        def __init__(self, *args, **kwargs):
            pass

        def add_memories(self, **kwargs):
            raise RuntimeError("boom secret=sk-test content should not be logged")

    (tmp_path / "everos.json").write_text('{"capture_agent_memory": true}\n', encoding="utf-8")
    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr("everos_hermes.provider.EverOSClient", FakeClient)
    provider = EverOSMemoryProvider()
    provider.initialize(session_id="sess-1", hermes_home=str(tmp_path), platform="cli")
    provider.sync_turn("please remember failure", "failed once", session_id="sess-2")
    provider.shutdown()

    assert provider._last_write_status["ok"] is False
    log_path = tmp_path / "everos.log"
    log_text = log_path.read_text(encoding="utf-8")
    assert log_path.stat().st_mode & 0o777 == 0o600
    assert "sync_turn.personal" in log_text
    assert "sk-test" not in log_text
    assert "please remember failure" not in log_text




def test_provider_sync_turn_records_agent_visibility_gap_when_enabled(monkeypatch, tmp_path):
    from everos_hermes.provider import EverOSMemoryProvider

    class FakeClient:
        def __init__(self, *args, **kwargs):
            pass

        def add_memories(self, **kwargs):
            return {"data": {"status": "queued", "task_id": f"task-{kwargs['scope']}"}}

        def flush_memories(self, **kwargs):
            return {"data": {"status": "success", "task_id": f"flush-{kwargs['scope']}"}}

        def search_memories(self, **kwargs):
            return {"data": {"agent_memory": []}}

        def get_memories(self, **kwargs):
            return {"data": {"items": []}}

    (tmp_path / "everos.json").write_text(
        '{"capture_agent_memory": true, "agent_summary_after_turn": true, "agent_flush_after_turn": true, "agent_visibility_verify_after_flush": true}\n',
        encoding="utf-8",
    )
    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr("everos_hermes.provider.EverOSClient", FakeClient)
    provider = EverOSMemoryProvider()
    provider.initialize(session_id="sess-1", hermes_home=str(tmp_path), platform="cli")

    provider.sync_turn("please debug agent memories", "queued and flushed agent trajectory", session_id="sess-agent")
    provider.shutdown()

    assert provider._last_agent_visibility_status["agent_visibility_status"] == "not_visible"
    assert provider._last_agent_visibility_status["agent_structured_visible"] is False
    assert [check["kind"] for check in provider._last_agent_visibility_status["agent_visibility_checks"]] == ["search", "search", "get", "get"]
