from __future__ import annotations

def _init_provider(monkeypatch, tmp_path, fake_client, config: str = "{}"):
    from everos_hermes.provider import EverOSMemoryProvider

    (tmp_path / "everos.json").write_text(config + "\n", encoding="utf-8")
    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setattr("everos_hermes.provider.EverOSClient", fake_client)
    provider = EverOSMemoryProvider()
    provider.initialize(session_id="sess-1", hermes_home=str(tmp_path), platform="cli")
    return provider


def test_prefetch_uses_assembler_cache_agent_and_session_scoped_raw(monkeypatch, tmp_path):
    calls = []

    class FakeClient:
        def __init__(self, *args, **kwargs):
            pass

        def search_memories(self, **kwargs):
            calls.append(kwargs)
            memory_types = kwargs["memory_types"]
            if memory_types == ["agent_memory"]:
                return {"data": {"agent_cases": [{"id": "case-1", "task_intent": "debug cache", "approach": "reuse cached result", "score": 0.9}]}}
            if memory_types == ["raw_message"]:
                return {"data": {"raw_messages": [{"id": "raw-1", "role": "user", "content": "recent raw clue", "score": 0.8}]}}
            return {
                "data": {
                    "profiles": [{"id": "profile-1", "profile_data": {"explicit_info": ["User verifies every phase"]}, "score": 1.0}],
                    "episodes": [{"id": "episode-1", "subject": "cache", "summary": "Cache should avoid duplicate search", "score": 0.7}],
                }
            }

    provider = _init_provider(
        monkeypatch,
        tmp_path,
        FakeClient,
        '{"agent_recall": true, "include_recent_raw": true, "prefetch_cache_ttl_seconds": 90}',
    )

    context = provider.prefetch("debug cache", session_id="sess-2")
    cached = provider.prefetch("debug cache", session_id="sess-2")

    assert context == cached
    assert context.startswith('<everos-context version="2" source="prefetch">')
    assert "User verifies every phase" in context
    assert "reuse cached result" in context
    assert "recent raw clue" in context
    assert [call["memory_types"] for call in calls] == [["episodic_memory", "profile"], ["agent_memory"], ["raw_message"]]
    assert calls[0]["session_id"] is None
    assert calls[1]["session_id"] is None
    assert calls[2]["session_id"] == "sess-2"
    assert provider._last_recall_status["ok"] is True
    assert provider._last_recall_status["cached"] is True

def test_sync_turn_adds_personal_message_ids_and_respects_agent_summary_flag(monkeypatch, tmp_path):
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

    provider = _init_provider(
        monkeypatch,
        tmp_path,
        FakeClient,
        '{"capture_agent_memory": true, "agent_summary_after_turn": false}',
    )
    provider.sync_turn("remember deterministic ids", "Noted.", session_id="sess-2")
    provider.shutdown()

    add_calls = [kwargs for kind, kwargs in calls if kind == "add"]
    assert [call["scope"] for call in add_calls] == ["personal"]
    messages = add_calls[0]["messages"]
    assert [message["role"] for message in messages] == ["user", "assistant"]
    assert [message["content"] for message in messages] == ["remember deterministic ids", "Noted."]
    assert all(message["message_id"].startswith("eh_") for message in messages)
    assert [kwargs for kind, kwargs in calls if kind == "flush"] == [{"user_id": "u1", "session_id": "sess-2", "scope": "personal"}]


def test_on_pre_compress_captures_agent_trajectory_without_flush_and_session_end_dedupes(monkeypatch, tmp_path):
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

    provider = _init_provider(monkeypatch, tmp_path, FakeClient, '{"capture_agent_memory": true}')
    messages = [
        {"role": "user", "timestamp": 1, "content": "run diagnostics"},
        {"role": "assistant", "timestamp": 2, "content": "", "tool_calls": [{"id": "call-1", "function": {"name": "diagnose", "arguments": "{}"}}]},
        {"role": "tool", "timestamp": 3, "tool_call_id": "call-1", "content": "diagnostics ok"},
        {"role": "assistant", "timestamp": 4, "content": "verified"},
    ]

    summary = provider.on_pre_compress(messages)
    provider.on_session_end(messages)

    agent_adds = [kwargs for kind, kwargs in calls if kind == "add" and kwargs["scope"] == "agent"]
    agent_flushes = [kwargs for kind, kwargs in calls if kind == "flush" and kwargs["scope"] == "agent"]
    assert "EverOS captured 4 agent trajectory messages for session sess-1" in summary
    assert len(agent_adds) == 1
    assert all(message["source"] == "pre_compress" for message in agent_adds[0]["messages"])
    assert agent_flushes == []


def test_on_session_end_writes_agent_tool_trajectory_before_personal_flush(monkeypatch, tmp_path):
    calls = []

    class FakeClient:
        def __init__(self, *args, **kwargs):
            pass

        def add_memories(self, **kwargs):
            calls.append(("add", kwargs))
            return {"data": {"status": "queued"}}

        def flush_memories(self, **kwargs):
            calls.append(("flush", kwargs))
            if kwargs["scope"] == "agent":
                raise RuntimeError("agent flush failed")
            return {"data": {"status": "success"}}

    provider = _init_provider(monkeypatch, tmp_path, FakeClient, '{"capture_agent_memory": true}')
    messages = [
        {"role": "user", "timestamp": 1, "content": "run diagnostics"},
        {"role": "assistant", "timestamp": 2, "content": "", "tool_calls": [{"id": "call-1", "function": {"name": "diagnose"}}]},
        {"role": "tool", "timestamp": 3, "tool_call_id": "call-1", "content": "diagnostics ok"},
    ]

    provider.on_session_end(messages)

    assert [kind for kind, _ in calls] == ["add", "flush", "flush"]
    assert calls[0][1]["scope"] == "agent"
    assert provider._last_agent_write_status["ok"] is True
    assert calls[2][1]["scope"] == "personal"


def test_on_delegation_writes_child_session_id_prefix_and_flushes(monkeypatch, tmp_path):
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

    provider = _init_provider(monkeypatch, tmp_path, FakeClient, '{"capture_agent_memory": true}')

    provider.on_delegation("investigate failing test", "fixed with a regression test", child_session_id="child-42")

    agent_add = [kwargs for kind, kwargs in calls if kind == "add" and kwargs["scope"] == "agent"][0]
    agent_flush = [kwargs for kind, kwargs in calls if kind == "flush" and kwargs["scope"] == "agent"][0]
    assistant_message = agent_add["messages"][1]
    assert assistant_message["content"].startswith("[delegation child_session_id=child-42]")
    assert assistant_message["child_session_id"] == "child-42"
    assert agent_flush == {"user_id": "u1", "session_id": "sess-1", "scope": "agent"}
