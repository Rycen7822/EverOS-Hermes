from __future__ import annotations

import json

from everos_hermes.trajectory import build_agent_trajectory_messages


def test_builds_user_assistant_tool_chain_with_tool_calls():
    messages = [
        {"role": "user", "content": "Find papers", "timestamp": 1000},
        {
            "role": "assistant",
            "content": "",
            "tool_calls": [
                {"id": "call-1", "type": "function", "function": {"name": "search", "arguments": '{"q":"paper"}'}}
            ],
            "timestamp": 1001,
        },
        {"role": "tool", "tool_call_id": "call-1", "content": "paper results", "timestamp": 1002},
        {"role": "assistant", "content": "Found results", "timestamp": 1003},
    ]

    result = build_agent_trajectory_messages(messages, session_id="sess-1", source="session_end", now_ms=1_700_000_000_000)

    assert [m["role"] for m in result.messages] == ["user", "assistant", "tool", "assistant"]
    assert result.messages[1]["content"] == "[Assistant requested tool calls]"
    assert result.messages[1]["tool_calls"][0]["id"] == "call-1"
    assert result.messages[2]["tool_call_id"] == "call-1"
    assert result.input_count == 4
    assert result.output_count == 4
    assert result.dropped_count == 0
    assert result.source == "session_end"


def test_drops_tool_without_tool_call_id_and_warns():
    result = build_agent_trajectory_messages(
        [
            {"role": "user", "content": "run"},
            {"role": "tool", "content": "orphan output"},
        ],
        session_id="sess-1",
        source="session_end",
        now_ms=1_700_000_000_000,
    )

    assert [m["role"] for m in result.messages] == ["user"]
    assert result.dropped_count == 1
    assert any("tool_call_id" in warning for warning in result.warnings)


def test_assistant_tool_call_without_content_gets_placeholder():
    result = build_agent_trajectory_messages(
        [{"role": "assistant", "content": None, "tool_calls": [{"id": "call-1"}]}],
        session_id="sess-1",
        source="session_end",
        now_ms=1_700_000_000_000,
    )

    assert result.messages[0]["content"] == "[Assistant requested tool calls]"


def test_redacts_secret_patterns_and_strips_everos_context():
    result = build_agent_trajectory_messages(
        [
            {
                "role": "assistant",
                "content": "Authorization: Bearer fake token=secret password=hunter2 sk-testplaceholder <everos-context>do not leak</everos-context>",
                "tool_calls": [{"id": "call-1", "args": "api_key=secret-value"}],
            }
        ],
        session_id="sess-1",
        source="session_end",
        now_ms=1_700_000_000_000,
    )

    rendered = json.dumps(result.messages, ensure_ascii=False)
    assert "fake" not in rendered
    assert "hunter2" not in rendered
    assert "sk-testplaceholder" not in rendered
    assert "do not leak" not in rendered
    assert "secret-value" not in rendered
    assert "[REDACTED]" in rendered
    assert "<everos-context>" not in rendered
    assert "<memory-context>" not in rendered


def test_deterministic_message_id_is_stable():
    base = [
        {"role": "user", "content": "same", "timestamp": 1700000000},
        {"role": "user", "content": "same", "timestamp": 1700000000},
    ]

    first = build_agent_trajectory_messages(base, session_id="sess-1", source="session_end", now_ms=1_700_000_000_000)
    second = build_agent_trajectory_messages(base, session_id="sess-1", source="pre_compress", now_ms=1_800_000_000_000)
    changed_content = build_agent_trajectory_messages(
        [{"role": "user", "content": "changed", "timestamp": 1700000000}],
        session_id="sess-1",
        source="session_end",
        now_ms=1_700_000_000_000,
    )
    changed_timestamp = build_agent_trajectory_messages(
        [{"role": "user", "content": "same", "timestamp": 1700000001}],
        session_id="sess-1",
        source="session_end",
        now_ms=1_700_000_000_000,
    )

    assert [m["message_id"] for m in first.messages] == [m["message_id"] for m in second.messages]
    assert first.messages[0]["message_id"] != first.messages[1]["message_id"]
    assert first.messages[0]["message_id"] != changed_content.messages[0]["message_id"]
    assert first.messages[0]["message_id"] != changed_timestamp.messages[0]["message_id"]
    assert first.fingerprint == second.fingerprint


def test_payload_budget_keeps_recent_task_chain():
    messages = [
        {"role": "user", "content": "old user " + "x" * 80},
        {"role": "assistant", "content": "old assistant " + "y" * 80},
        {"role": "user", "content": "recent task"},
        {"role": "assistant", "content": "recent answer"},
        {"role": "tool", "tool_call_id": "call-2", "content": "recent tool"},
    ]

    result = build_agent_trajectory_messages(
        messages,
        session_id="sess-1",
        source="session_end",
        now_ms=1_700_000_000_000,
        max_payload_chars=120,
    )

    rendered = json.dumps(result.messages)
    assert "old user" not in rendered
    assert "old assistant" not in rendered
    assert [m["content"] for m in result.messages] == ["recent task", "recent answer", "recent tool"]
    assert result.dropped_count == 2


def test_timestamp_normalization_accepts_ms_seconds_and_missing():
    result = build_agent_trajectory_messages(
        [
            {"role": "user", "content": "ms", "timestamp": 1_700_000_000_123},
            {"role": "user", "content": "seconds", "timestamp": 1_700_000_000},
            {"role": "user", "content": "missing"},
        ],
        session_id="sess-1",
        source="session_end",
        now_ms=1_800_000_000_000,
    )

    assert [m["timestamp"] for m in result.messages] == [
        1_700_000_000_123,
        1_700_000_000_000,
        1_800_000_000_002,
    ]
