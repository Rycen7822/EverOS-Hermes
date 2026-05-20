from __future__ import annotations

import json


def test_endpoint_whitelist_excludes_group_sender_storage():
    from everos_hermes.schemas import CLOUD_V1_ENDPOINTS, OUT_OF_SCOPE_ENDPOINTS

    paths = set(CLOUD_V1_ENDPOINTS.values())
    assert paths == {
        "/api/v1/memories",
        "/api/v1/memories/agent",
        "/api/v1/memories/flush",
        "/api/v1/memories/agent/flush",
        "/api/v1/memories/get",
        "/api/v1/memories/search",
        "/api/v1/memories/delete",
        "/api/v1/tasks/{task_id}",
        "/api/v1/settings",
    }
    rendered = json.dumps({"in": list(paths), "out": OUT_OF_SCOPE_ENDPOINTS})
    assert "/api/v0" not in rendered
    assert "/api/v1/memories/group" not in paths
    assert "/api/v1/groups" in OUT_OF_SCOPE_ENDPOINTS
    assert "/api/v1/senders" in OUT_OF_SCOPE_ENDPOINTS
    assert "/api/v1/object/sign" in OUT_OF_SCOPE_ENDPOINTS


def test_contract_document_covers_scope_and_blacklist():
    from everos_hermes.schemas import CLOUD_V1_ENDPOINTS

    text = open("docs/everos_cloud_v1_contract.md", encoding="utf-8").read()
    for path in CLOUD_V1_ENDPOINTS.values():
        assert path.replace("/api/v1/tasks/", "/api/v1/tasks/") in text
    for term in ["PUT /api/v1/settings", "group", "multimodal", "out of scope"]:
        assert term in text.lower() if term.islower() else term in text
