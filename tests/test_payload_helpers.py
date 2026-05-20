from __future__ import annotations


def test_provider_and_mcp_share_tool_payload_helpers():
    from everos_hermes import mcp_server, provider
    from everos_hermes.tool_payloads import flush_result_payload, save_result_payload, timeout_payload

    assert provider._flush_result_payload is flush_result_payload
    assert provider._save_result_payload is save_result_payload
    assert provider._timeout_payload is timeout_payload
    assert mcp_server._flush_result_payload is flush_result_payload
    assert mcp_server._save_result_payload is save_result_payload
    assert mcp_server._timeout_payload is timeout_payload


def test_tool_payload_helpers_preserve_provider_shape():
    from everos_hermes.tool_payloads import flush_result_payload, save_result_payload

    payload = save_result_payload(
        result={"data": {"status": "queued", "task_id": "task-1"}},
        user_id="u1",
        session_id="s1",
        scope="agent",
        flush_requested=True,
        flush_result={"data": {"status": "success", "request_id": "req-1"}},
    )
    assert payload == {
        "saved": True,
        "message_queued": True,
        "extraction_requested": True,
        "searchable": None,
        "scope": "agent",
        "user_id": "u1",
        "session_id": "s1",
        "status": "queued",
        "task_id": "task-1",
        "flush": {"ok": True, "status": "success", "request_id": "req-1"},
    }
    assert flush_result_payload({"data": {"status": "success"}}, attempt_count=2) == {"ok": True, "attempt_count": 2, "status": "success"}
