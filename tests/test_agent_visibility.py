from __future__ import annotations

from everos_hermes.client import EverOSError
from everos_hermes.agent_visibility import (
    audit_agent_visibility,
    build_agent_visibility_report,
    workflow_status_from_agent_visibility,
)



def test_build_agent_visibility_report_partial_when_search_hits_but_get_misses():
    report = build_agent_visibility_report(
        agent_raw_queued=True,
        agent_flush=None,
        checks=[
            {"kind": "search", "memory_types": ["agent_memory"], "query": "agent marker", "hit_count": 2, "status": "hit", "latency_ms": 1.0},
            {"kind": "get", "memory_type": "agent_case", "hit_count": 0, "status": "miss", "latency_ms": 1.0},
            {"kind": "get", "memory_type": "agent_skill", "hit_count": 0, "status": "miss", "latency_ms": 1.0},
        ],
    )

    assert report["agent_structured_visible"] is True
    assert report["agent_visibility_status"] == "partial"


def test_workflow_status_from_agent_visibility_keeps_existing_mapping():
    assert workflow_status_from_agent_visibility({"agent_visibility_status": "visible"}, "fallback") == "verified"
    assert workflow_status_from_agent_visibility({"agent_visibility_status": "partial"}, "fallback") == "partially_verified"
    assert workflow_status_from_agent_visibility({"agent_visibility_status": "not_visible"}, "fallback") == "agent_not_visible"
    assert workflow_status_from_agent_visibility({"agent_visibility_status": "error"}, "fallback") == "agent_visibility_error"
    assert workflow_status_from_agent_visibility({"agent_visibility_status": "unchecked"}, "fallback") == "fallback"


def test_audit_agent_visibility_runs_search_and_agent_gets_independently():
    class FakeClient:
        def __init__(self):
            self.calls = []

        def search_memories(self, **kwargs):
            self.calls.append(("search", kwargs))
            raise EverOSError("EverOS request failed: error sending request")

        def get_memories(self, **kwargs):
            self.calls.append(("get", kwargs))
            return {"data": {"items": []}}

    client = FakeClient()
    report = audit_agent_visibility(
        client=client,
        user_id="u1",
        session_id="s1",
        queries=[" agent marker "],
        top_k=5,
        timeout=30,
        get_page_size=20,
    )

    assert [call[0] for call in client.calls] == ["search", "get", "get"]
    assert client.calls[0][1]["query"] == "agent marker"
    assert client.calls[0][1]["memory_types"] == ["agent_memory"]
    assert client.calls[1][1]["memory_type"] == "agent_case"
    assert client.calls[2][1]["memory_type"] == "agent_skill"
    assert report["agent_visibility_checks"][0]["status"] == "error"
    assert report["agent_visibility_checks"][1]["status"] == "miss"
    assert report["agent_visibility_checks"][2]["status"] == "miss"
    assert report["agent_visibility_status"] == "error"



def test_audit_agent_visibility_counts_nested_agent_memory_shapes():
    class FakeClient:
        def search_memories(self, **kwargs):
            return {"data": {"agent_memory": {"cases": [{"id": "c1"}, {"id": "c2"}]}, "agent_cases": [{"id": "c3"}]}}

        def get_memories(self, **kwargs):
            if kwargs["memory_type"] == "agent_case":
                return {"data": {"agent_cases": [{"id": "case-1"}]}}
            return {"data": {"agent_skills": [{"id": "skill-1"}, {"id": "skill-2"}]}}

    report = audit_agent_visibility(
        client=FakeClient(),
        user_id="u1",
        session_id="s1",
        queries=["agent marker"],
        top_k=5,
        timeout=None,
        get_page_size=20,
    )

    checks = report["agent_visibility_checks"]
    assert checks[0]["hit_count"] == 3
    assert checks[1]["hit_count"] == 1
    assert checks[2]["hit_count"] == 2
    assert report["agent_visibility_status"] == "visible"
