from __future__ import annotations

from everos_hermes.client import EverOSError
from everos_hermes.workflows import import_and_verify, save_and_verify, verify_session_ingest


class NoWriteClient:
    def add_memories(self, **kwargs):  # pragma: no cover - dry-run/validation must not write
        raise AssertionError("workflow should not call add_memories")


def _message(index: int, *, timestamp=1712052000000, content: str | None = None) -> dict:
    return {
        "role": "user",
        "timestamp": timestamp,
        "content": content or f"message {index}",
    }


def test_import_dry_run_warns_for_non_epoch_ms_timestamp_and_reports_metrics():
    result = import_and_verify(
        client=NoWriteClient(),
        user_id="u1",
        session_id="s1",
        messages=[
            _message(1, timestamp="2026-05-13T00:00:00Z", content="Alpha"),
            _message(2, timestamp=1712052000000, content="BetaBeta"),
        ],
        dry_run=True,
        batch_size=5,
        flush=False,
        workflow="batch_ingest",
    )

    assert result["ok"] is True
    assert result["status"] == "dry_run"
    assert any("timestamp" in warning and "epoch millisecond" in warning for warning in result["warnings"])
    assert result["metrics"]["total_messages"] == 2
    assert result["metrics"]["max_content_chars"] == len("BetaBeta")
    assert result["metrics"]["batch_count"] == 1
    assert result["metrics"]["estimated_payload_bytes"] > 0
    assert result["metrics"]["max_batch_payload_bytes"] > 0


def test_import_rejects_non_epoch_ms_timestamp_before_real_write():
    result = import_and_verify(
        client=NoWriteClient(),
        user_id="u1",
        session_id="s1",
        messages=[_message(1, timestamp="2026-05-13T00:00:00Z")],
        dry_run=False,
        batch_size=5,
        flush=False,
        workflow="batch_ingest",
    )

    assert result["ok"] is False
    assert result["error_code"] == "validation_failed"
    assert result["queued_count"] == 0
    assert result["failed_count"] == 1
    assert any("timestamp" in warning for warning in result["warnings"])


def test_import_splits_cloud_403_batches_until_small_subbatches_queue():
    calls: list[int] = []

    class SplitClient:
        def add_memories(self, **kwargs):
            size = len(kwargs["messages"])
            calls.append(size)
            if size > 5:
                raise EverOSError("EverOS API error 403: Forbidden")
            return {"data": {"status": "queued", "task_id": f"task-{len(calls)}"}}

    result = import_and_verify(
        client=SplitClient(),
        user_id="u1",
        session_id="s1",
        messages=[_message(index) for index in range(12)],
        dry_run=False,
        batch_size=12,
        flush=False,
        workflow="batch_ingest",
    )

    assert result["ok"] is True
    assert result["queued_count"] == 12
    assert result["failed_count"] == 0
    assert result["status"] == "queued"
    assert calls[0] == 12
    assert max(size for size in calls if size <= 5) <= 5
    assert any(batch.get("split_from") is not None for batch in result["batches"])
    assert any("split" in action for action in result["suggested_next_actions"])

class AgentVisibilityClient:
    def __init__(self):
        self.calls = []

    def add_memories(self, **kwargs):
        self.calls.append(("add", kwargs))
        return {"data": {"status": "queued", "task_id": "task-agent"}}

    def flush_memories(self, **kwargs):
        self.calls.append(("flush", kwargs))
        return {"data": {"status": "success", "task_id": "flush-agent"}}

    def search_memories(self, **kwargs):
        self.calls.append(("search", kwargs))
        return {"data": {"agent_memory": []}}

    def get_memories(self, **kwargs):
        self.calls.append(("get", kwargs))
        return {"data": {"items": []}}


def test_save_and_verify_agent_reports_not_visible_separately_from_queue():
    client = AgentVisibilityClient()

    result = save_and_verify(
        client=client,
        content="agent marker",
        user_id="u1",
        session_id="s1",
        scope="agent",
        verification_query="agent marker",
        flush=True,
    )

    assert result["save"]["saved"] is True
    assert result["save"]["scope"] == "agent"
    assert result["agent_visibility"]["agent_raw_queued"] is True
    assert result["agent_visibility"]["agent_flush"]["status"] == "success"
    assert result["agent_visibility"]["agent_structured_visible"] is False
    assert result["agent_visibility"]["agent_visibility_status"] == "not_visible"
    assert result["status"] == "agent_not_visible"
    assert result["verification"]["status"] == "agent_not_visible"


def test_verify_session_ingest_agent_scope_returns_visibility_checks():
    client = AgentVisibilityClient()

    result = verify_session_ingest(
        client=client,
        user_id="u1",
        session_id="s1",
        verification_queries=["agent marker"],
        memory_types=["agent_memory"],
        scope="agent",
        top_k=5,
    )

    assert result["status"] == "agent_not_visible"
    assert result["verified"] is False
    visibility = result["agent_visibility"]
    assert visibility["agent_raw_queued"] is None
    assert visibility["agent_structured_visible"] is False
    assert visibility["agent_visibility_status"] == "not_visible"
    assert [check["kind"] for check in visibility["agent_visibility_checks"]] == ["search", "get", "get"]
    assert visibility["agent_visibility_checks"][0]["memory_types"] == ["agent_memory"]
