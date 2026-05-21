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
    )

    assert result["ok"] is True
    assert result["status"] == "dry_run"
    assert any("timestamp" in warning and "epoch millisecond" in warning for warning in result["warnings"])
    assert result["metrics"]["total_messages"] == 2
    assert result["metrics"]["max_content_chars"] == len("BetaBeta")
    assert result["metrics"]["batch_count"] == 1
    assert result["metrics"]["estimated_payload_bytes"] > 0
    assert result["metrics"]["max_batch_payload_bytes"] > 0


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



def test_import_and_verify_preserves_batch_payload_when_flush_has_non_timeout_error():
    class FlushFailClient(AgentVisibilityClient):
        def flush_memories(self, **kwargs):
            self.calls.append(("flush", kwargs))
            raise RuntimeError("flush failed token=import-secret")

    result = import_and_verify(
        client=FlushFailClient(),
        user_id="u1",
        session_id="s1",
        messages=[_message(1), _message(2)],
        batch_size=2,
        flush=True,
    )

    assert result["ok"] is True
    assert result["queued_count"] == 2
    assert result["failed_count"] == 0
    assert result["flush"]["ok"] is False
    assert result["flush"]["status"] == "error"
    assert "import-secret" not in str(result)


def test_save_and_verify_preserves_save_payload_when_verification_has_error():
    class VerifyFailClient(AgentVisibilityClient):
        def search_memories(self, **kwargs):
            self.calls.append(("search", kwargs))
            raise RuntimeError("verify failed token=verify-secret")

    result = save_and_verify(
        client=VerifyFailClient(),
        content="queued before verify failure",
        user_id="u1",
        session_id="s1",
        scope="personal",
        verification_queries=["queued before verify failure"],
        flush=True,
    )

    assert result["ok"] is True
    assert result["status"] == "verification_error"
    assert result["save"]["saved"] is True
    assert result["save"]["message_queued"] is True
    assert result["verification"]["ok"] is False
    assert result["verification"]["status"] == "error"
    assert result["verification"]["verified"] is False
    assert "verify-secret" not in str(result)



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
    checks = visibility["agent_visibility_checks"]
    assert [check["kind"] for check in checks] == ["search", "get", "get"]
    assert sum(1 for name, _ in client.calls if name == "search") == 1



def test_import_fatal_validation_stays_before_write():
    result = import_and_verify(
        client=NoWriteClient(),
        user_id="u1",
        session_id="s1",
        messages=[{"role": "user", "timestamp": "2026-05-13T00:00:00Z", "content": "Alpha"}],
        dry_run=False,
        flush=False,
    )

    assert result["ok"] is False
    assert result["error_code"] == "validation_failed"
    assert result["queued_count"] == 0
    assert any("timestamp" in item for item in result["warnings"])

class RetryFlushClient(AgentVisibilityClient):
    def __init__(self):
        super().__init__()
        self.flush_calls = 0

    def flush_memories(self, **kwargs):
        self.flush_calls += 1
        if self.flush_calls == 1:
            raise EverOSError("EverOS request failed: error sending request")
        return {"data": {"status": "success", "task_id": "flush-ok"}}


def test_workflows_retry_transient_flush_send_error_once():
    cases = [
        (
            lambda client: save_and_verify(
                client=client,
                content="retry flush after save",
                user_id="u1",
                session_id="s1",
                verification_queries=[],
                flush=True,
            ),
            ("save", "flush"),
        ),
        (
            lambda client: import_and_verify(
                client=client,
                user_id="u1",
                session_id="s1",
                messages=[_message(1), _message(2)],
                flush=True,
            ),
            ("flush",),
        ),
    ]

    for run_workflow, path in cases:
        client = RetryFlushClient()
        result = run_workflow(client)
        flush_payload = result
        for key in path:
            flush_payload = flush_payload[key]
        assert client.flush_calls == 2
        assert flush_payload["ok"] is True
        assert flush_payload["status"] == "success"
