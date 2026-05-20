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


def test_tool_errors_are_redacted_before_returning_to_model(monkeypatch, tmp_path):
    from everos_hermes.provider import EverOSMemoryProvider

    class FakeClient:
        def __init__(self, *args, **kwargs):
            pass

        def search_memories(self, **kwargs):
            raise RuntimeError("backend failed api_key=secret-tool-key Authorization: Bearer secret-token")

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr("everos_hermes.provider.EverOSClient", FakeClient)
    provider = EverOSMemoryProvider()
    provider.initialize(session_id="sess-1", hermes_home=str(tmp_path), platform="cli")

    raw = provider.handle_tool_call("everos_memory_search", {"query": "coffee"})

    result = json.loads(raw)
    assert "secret-tool-key" not in result["error"]
    assert "secret-token" not in result["error"]
    assert "[REDACTED]" in result["error"]


def test_redaction_handles_quoted_and_whitespace_secret_values():
    from everos_hermes.redaction import sanitized_error_message

    secret_words = ["alpha", "beta", "gamma"]
    token_words = ["delta", "epsilon"]
    bearer_token = "abc" + "+def/" + "ghi=~tail"
    quoted_value = "quoted," + "semi;" + "with]delimiters"
    credentials_value = "zeta" + " eta"
    email_secret = "email" + "-secret"
    key_secret = "key" + "-secret"
    client_id_secret = "client" + "-id-secret"
    credentials_blob = json.dumps({"client_email": email_secret, "private_key": key_secret})
    credentials_object = json.dumps({"client_email": email_secret, "client_id": client_id_secret})
    arguments_object = json.dumps({"credentials": {"client_email": email_secret, "client_id": client_id_secret}})
    pretty_payload = "{\n  \"client_email\": \"" + email_secret + "\",\n  \"client_id\": \"" + client_id_secret + "\"\n}"

    rendered = "\n".join(
        [
            sanitized_error_message(
                "backend failed pass" + "word='" + " ".join(secret_words) + "' request_id=req-pass"
            ),
            sanitized_error_message("tok" + "en=" + " ".join(token_words) + " request_id=req-token"),
            sanitized_error_message("Authorization: Bearer " + bearer_token),
            sanitized_error_message("api_" + "key=\"" + quoted_value + "\""),
            sanitized_error_message("creden" + "tials=" + credentials_value),
            sanitized_error_message(
                "creden" + "tials=\"" + credentials_blob.replace('"', '\\"') + "\""
            ),
            sanitized_error_message("creden" + "tials=" + credentials_object + " request_id=req-object"),
            sanitized_error_message("arguments=\"" + arguments_object.replace('"', '\\"') + "\" request_id=req-args"),
            sanitized_error_message("creden" + "tials=" + pretty_payload + " request_id=req-pretty"),
        ]
    )

    for leaked in [
        *secret_words,
        *token_words,
        bearer_token,
        quoted_value,
        credentials_value,
        email_secret,
        key_secret,
        client_id_secret,
    ]:
        assert leaked not in rendered
    assert rendered.count("[REDACTED]") >= 7
    assert "request_id=req-pass" in rendered
    assert "request_id=req-token" in rendered
    assert "request_id=req-object" in rendered
    assert "request_id=req-args" in rendered
    assert "request_id=req-pretty" in rendered

def test_memory_save_tool_preserves_queue_payload_when_flush_has_non_timeout_error(monkeypatch, tmp_path):
    from everos_hermes.provider import EverOSMemoryProvider

    class FakeClient:
        def __init__(self, *args, **kwargs):
            pass

        def add_memories(self, **kwargs):
            return {"data": {"status": "queued", "task_id": "task-queued"}}

        def flush_memories(self, **kwargs):
            raise RuntimeError("flush failed token=provider-secret")

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr("everos_hermes.provider.EverOSClient", FakeClient)
    provider = EverOSMemoryProvider()
    provider.initialize(session_id="sess-1", hermes_home=str(tmp_path), platform="cli")

    result = json.loads(provider.handle_tool_call("everos_memory_save", {"content": "queued before failure", "flush": True}))

    assert result["saved"] is True
    assert result["message_queued"] is True
    assert result["task_id"] == "task-queued"
    assert result["flush"]["ok"] is False
    assert result["flush"]["status"] == "error"
    assert "provider-secret" not in json.dumps(result)


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

    assert context.startswith('<everos-context version="2" source="prefetch">')
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
    assert calls[1] == ("flush", {"user_id": "u1", "session_id": "sess-2", "scope": "personal"})


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
    assert calls == [{"user_id": "u1", "session_id": "sess-2", "scope": "personal", "timeout": 45}]
    assert result["ok"] is False
    assert result["retryable"] is True
    assert result["operation"] == "flush"



def test_provider_tool_schemas_expose_cloud_v1_parameters(monkeypatch, tmp_path):
    from everos_hermes.provider import EverOSMemoryProvider

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    provider = EverOSMemoryProvider()
    provider.initialize(session_id="sess-1", hermes_home=str(tmp_path), platform="cli")
    schemas = {schema["name"]: schema for schema in provider.get_tool_schemas()}

    assert schemas["everos_memory_save"]["parameters"]["properties"]["scope"]["enum"] == ["personal", "agent"]
    assert "tool_call_id" in schemas["everos_memory_save"]["parameters"]["properties"]
    assert "scope" in schemas["everos_memory_flush"]["parameters"]["properties"]
    search_props = schemas["everos_memory_search"]["parameters"]["properties"]
    for name in ["filters", "radius", "top_k", "response_format"]:
        assert name in search_props
    get_props = schemas["everos_memory_get"]["parameters"]["properties"]
    for name in ["filters", "rank_by", "rank_order"]:
        assert name in get_props


def test_sync_turn_capture_agent_memory_false_only_writes_personal(monkeypatch, tmp_path):
    from everos_hermes.provider import EverOSMemoryProvider

    calls = []

    class FakeClient:
        def __init__(self, *args, **kwargs):
            pass

        def add_memories(self, **kwargs):
            calls.append(("add", kwargs))
            return {"data": {"status": "queued"}}

        def flush_memories(self, **kwargs):
            calls.append(("flush", kwargs))
            return {"data": {"status": "success"}}

    (tmp_path / "everos.json").write_text('{"capture_agent_memory": false}\n', encoding="utf-8")
    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr("everos_hermes.provider.EverOSClient", FakeClient)
    provider = EverOSMemoryProvider()
    provider.initialize(session_id="sess-1", hermes_home=str(tmp_path), platform="cli")
    provider.sync_turn("remember I like espresso", "Noted.", session_id="sess-2")
    provider.shutdown()

    assert [call[1].get("scope") for call in calls if call[0] == "add"] == ["personal"]
    assert [call[1].get("scope") for call in calls if call[0] == "flush"] == ["personal"]


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


def test_prefetch_agent_recall_runs_second_agent_memory_search(monkeypatch, tmp_path):
    from everos_hermes.provider import EverOSMemoryProvider

    calls = []

    class FakeClient:
        def __init__(self, *args, **kwargs):
            pass

        def search_memories(self, **kwargs):
            calls.append(kwargs)
            if kwargs["memory_types"] == ["agent_memory"]:
                return {"data": {"agent_cases": [{"task_intent": "debug timeout", "approach": "check task status before retry"}]}}
            return {"data": {"episodes": [{"summary": "User prefers careful verification"}]}}

    (tmp_path / "everos.json").write_text('{"agent_recall": true, "max_context_items": 8}\n', encoding="utf-8")
    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr("everos_hermes.provider.EverOSClient", FakeClient)
    provider = EverOSMemoryProvider()
    provider.initialize(session_id="sess-1", hermes_home=str(tmp_path), platform="cli")

    context = provider.prefetch("debug timeout")

    assert len(calls) == 2
    assert calls[0]["memory_types"] == ["episodic_memory", "profile"]
    assert calls[1]["memory_types"] == ["agent_memory"]
    assert "User prefers careful verification" in context
    assert "check task status before retry" in context


def test_provider_tools_pass_scope_filters_rank_and_timeout(monkeypatch, tmp_path):
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
            return {"data": {"status": "success"}}

        def search_memories(self, **kwargs):
            calls.append(("search", kwargs))
            return {"data": {"episodes": []}}

        def get_memories(self, **kwargs):
            calls.append(("get", kwargs))
            return {"data": {"items": []}}

        def delete_memories(self, **kwargs):
            calls.append(("delete", kwargs))
            return {"ok": True}

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr("everos_hermes.provider.EverOSClient", FakeClient)
    provider = EverOSMemoryProvider()
    provider.initialize(session_id="sess-1", hermes_home=str(tmp_path), platform="cli")

    save = json.loads(provider.handle_tool_call("everos_memory_save", {"content": "retry with timeout", "scope": "agent", "flush": True}))
    provider.handle_tool_call("everos_memory_search", {"query": "retry", "top_k": -1, "filters": {"AND": [{"timestamp": {"gte": 1}}]}, "radius": 0.5, "memory_types": ["agent_memory"], "response_format": "json"})
    provider.handle_tool_call("everos_memory_get", {"memory_type": "agent_case", "rank_by": "timestamp", "rank_order": "asc", "filters": {"AND": [{"timestamp": {"lte": 2}}]}})
    provider.handle_tool_call("everos_memory_flush", {"scope": "agent", "timeout": 45})

    assert save["scope"] == "agent"
    assert calls[0][1]["scope"] == "agent"
    assert calls[0][1]["messages"][0]["role"] == "assistant"
    assert calls[1][1]["scope"] == "agent"
    assert calls[2][1]["top_k"] == -1
    assert calls[2][1]["filters"] == {"AND": [{"timestamp": {"gte": 1}}]}
    assert calls[2][1]["radius"] == 0.5
    assert calls[3][1]["memory_type"] == "agent_case"
    assert calls[3][1]["rank_order"] == "asc"
    assert calls[4][1] == {"user_id": "u1", "session_id": "sess-1", "scope": "agent", "timeout": 45}


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


def test_provider_exposes_and_runs_save_and_verify_workflow(monkeypatch, tmp_path):
    from everos_hermes.provider import EverOSMemoryProvider

    calls = []

    class FakeClient:
        def __init__(self, *args, **kwargs):
            pass

        def add_memories(self, **kwargs):
            calls.append(("add", kwargs))
            return {"data": {"status": "queued", "task_id": "task-save"}}

        def flush_memories(self, **kwargs):
            calls.append(("flush", kwargs))
            return {"data": {"status": "success"}}

        def search_memories(self, **kwargs):
            calls.append(("search", kwargs))
            return {"data": {"episodes": [{"summary": "pytest preference"}]}}

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr("everos_hermes.provider.EverOSClient", FakeClient)
    provider = EverOSMemoryProvider()
    provider.initialize(session_id="sess-1", hermes_home=str(tmp_path), platform="cli")

    schemas = {schema["name"]: schema for schema in provider.get_tool_schemas()}
    assert "everos_memory_save_and_verify" in schemas
    assert "everos_memory_import_and_verify" in schemas
    assert "everos_memory_verify_session" in schemas

    raw = provider.handle_tool_call("everos_memory_save_and_verify", {
        "content": "User prefers pytest.",
        "verification_query": "pytest preference",
        "session_id": "sess-verify",
        "flush": True,
    })

    result = json.loads(raw)
    assert result["ok"] is True
    assert result["status"] == "verified"
    assert result["verification"]["verified"] is True
    assert [call[0] for call in calls] == ["add", "flush", "search"]


def test_provider_import_and_verify_dry_run_does_not_write(monkeypatch, tmp_path):
    from everos_hermes.provider import EverOSMemoryProvider

    class FakeClient:
        def __init__(self, *args, **kwargs):
            pass

        def add_memories(self, **kwargs):  # pragma: no cover - should not be called
            raise AssertionError("dry-run must not write")

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr("everos_hermes.provider.EverOSClient", FakeClient)
    provider = EverOSMemoryProvider()
    provider.initialize(session_id="sess-1", hermes_home=str(tmp_path), platform="cli")

    raw = provider.handle_tool_call("everos_memory_import_and_verify", {
        "messages": [
            {"role": "user", "content": "Alpha", "timestamp": 1},
            {"role": "tool", "content": "missing id", "timestamp": 2},
        ],
        "scope": "agent",
        "dry_run": True,
    })

    result = json.loads(raw)
    assert result["ok"] is True
    assert result["status"] == "dry_run"
    assert result["queued_count"] == 0
    assert any("tool_call_id" in warning for warning in result["warnings"])


def test_provider_verify_session_tool_is_read_only(monkeypatch, tmp_path):
    from everos_hermes.provider import EverOSMemoryProvider

    calls = []

    class FakeClient:
        def __init__(self, *args, **kwargs):
            pass

        def search_memories(self, **kwargs):
            calls.append(kwargs)
            return {"data": {"episodes": []}}

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr("everos_hermes.provider.EverOSClient", FakeClient)
    provider = EverOSMemoryProvider()
    provider.initialize(session_id="sess-1", hermes_home=str(tmp_path), platform="cli")

    raw = provider.handle_tool_call("everos_memory_verify_session", {
        "session_id": "sess-verify",
        "verification_queries": ["missing"],
    })

    result = json.loads(raw)
    assert result["ok"] is True
    assert result["status"] == "not_yet_searchable"
    assert result["verified"] is False
    assert calls[0]["session_id"] == "sess-verify"


def test_provider_agent_save_returns_unchecked_visibility(monkeypatch, tmp_path):
    from everos_hermes.provider import EverOSMemoryProvider

    class FakeClient:
        def __init__(self, *args, **kwargs):
            pass

        def add_memories(self, **kwargs):
            return {"data": {"status": "queued", "task_id": "task-agent"}}

        def flush_memories(self, **kwargs):
            return {"data": {"status": "success", "task_id": "flush-agent"}}

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr("everos_hermes.provider.EverOSClient", FakeClient)
    provider = EverOSMemoryProvider()
    provider.initialize(session_id="sess-1", hermes_home=str(tmp_path), platform="cli")

    result = json.loads(provider.handle_tool_call("everos_memory_save", {"content": "agent note", "scope": "agent", "flush": True}))

    assert result["agent_visibility"]["agent_raw_queued"] is True
    assert result["agent_visibility"]["agent_structured_visible"] is None
    assert result["agent_visibility"]["agent_visibility_status"] == "unchecked"
    assert result["agent_visibility"]["agent_flush"]["status"] == "success"


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


def test_provider_flush_agent_transient_request_failure_retries_once(monkeypatch, tmp_path):
    from everos_hermes.client import EverOSError
    from everos_hermes.provider import EverOSMemoryProvider

    calls = []

    class FakeClient:
        def __init__(self, *args, **kwargs):
            pass

        def flush_memories(self, **kwargs):
            calls.append(kwargs)
            if len(calls) == 1:
                raise EverOSError("EverOS request failed: error sending request")
            return {"data": {"status": "success", "task_id": "flush-agent"}}

    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr("everos_hermes.provider.EverOSClient", FakeClient)
    provider = EverOSMemoryProvider()
    provider.initialize(session_id="sess-1", hermes_home=str(tmp_path), platform="cli")

    result = json.loads(provider.handle_tool_call("everos_memory_flush", {"scope": "agent", "session_id": "sess-agent", "timeout": 45}))

    assert len(calls) == 2
    assert result["flush"]["ok"] is True
    assert result["flush"]["attempt_count"] == 2
    assert result["agent_visibility"]["agent_visibility_status"] == "unchecked"


def test_provider_config_module_exports_legacy_normalizer():
    from everos_hermes import provider
    from everos_hermes.provider_config import _DEFAULT_CONFIG, _normalize_config

    assert provider._normalize_config is _normalize_config
    assert _DEFAULT_CONFIG["base_url"]
    assert _normalize_config({"top_k": "999", "memory_types": "profile,episodic_memory"})["top_k"] == 20



def test_provider_agent_save_visibility_audit_updates_last_status_and_payload(monkeypatch, tmp_path):
    from everos_hermes.provider import EverOSMemoryProvider

    class FakeClient:
        def __init__(self, *args, **kwargs):
            pass

        def add_memories(self, **kwargs):
            return {"data": {"status": "queued", "task_id": "task-agent"}}

        def flush_memories(self, **kwargs):
            return {"data": {"status": "success", "task_id": "flush-agent"}}

        def search_memories(self, **kwargs):
            return {"data": {"agent_memory": []}}

        def get_memories(self, **kwargs):
            return {"data": {"items": []}}

    (tmp_path / "everos.json").write_text('{"agent_visibility_verify_after_flush": true}\n', encoding="utf-8")
    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr("everos_hermes.provider.EverOSClient", FakeClient)
    provider = EverOSMemoryProvider()
    provider.initialize(session_id="sess-1", hermes_home=str(tmp_path), platform="cli")

    result = json.loads(provider.handle_tool_call("everos_memory_save", {"content": "agent note", "scope": "agent", "flush": True}))

    assert result["agent_visibility"]["agent_raw_queued"] is True
    assert result["agent_visibility"]["agent_flush"]["status"] == "success"
    assert result["agent_visibility"]["agent_visibility_status"] == "not_visible"
    assert provider._last_agent_visibility_status["agent_visibility_status"] == "not_visible"
