import json
import urllib.error

import pytest


class FakeHTTPResponse:
    def __init__(self, payload: dict | None, status: int = 200):
        self.payload = payload
        self.status = status

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        return False

    def read(self):
        if self.payload is None:
            return b""
        return json.dumps(self.payload).encode("utf-8")


def test_add_memories_posts_bearer_json(monkeypatch):
    from everos_hermes.client import EverOSClient

    captured = {}

    def fake_urlopen(req, timeout):
        captured["url"] = req.full_url
        captured["method"] = req.get_method()
        captured["headers"] = dict(req.header_items())
        captured["body"] = json.loads(req.data.decode("utf-8"))
        captured["timeout"] = timeout
        return FakeHTTPResponse({"data": {"status": "queued", "task_id": "task-1"}}, status=202)

    monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)
    client = EverOSClient(api_key="sk-test", base_url="https://api.evermind.ai", timeout=7)

    result = client.add_memories(
        user_id="user_001",
        session_id="session_001",
        messages=[{"role": "user", "timestamp": 1711900000000, "content": "I like black coffee."}],
        async_mode=True,
    )

    assert result["data"]["task_id"] == "task-1"
    assert captured == {
        "url": "https://api.evermind.ai/api/v1/memories",
        "method": "POST",
        "headers": {
            "Authorization": "Bearer sk-test",
            "Content-type": "application/json",
            "Accept": "application/json",
        },
        "body": {
            "user_id": "user_001",
            "session_id": "session_001",
            "messages": [
                {"role": "user", "timestamp": 1711900000000, "content": "I like black coffee."}
            ],
            "async_mode": True,
        },
        "timeout": 7,
    }


def test_client_reads_api_key_from_hermes_dotenv_when_env_missing(monkeypatch, tmp_path):
    from everos_hermes.client import EverOSClient

    monkeypatch.delenv("EVEROS_API_KEY", raising=False)
    monkeypatch.delenv("EVEROS_BASE_URL", raising=False)
    monkeypatch.delenv("EVEROS_TIMEOUT", raising=False)
    monkeypatch.setenv("HERMES_HOME", str(tmp_path))
    (tmp_path / ".env").write_text(
        "EVEROS_API_KEY=sk-from-dotenv\n"
        "EVEROS_BASE_URL=https://everos.example.test/\n"
        "EVEROS_TIMEOUT=3\n",
        encoding="utf-8",
    )

    client = EverOSClient()

    assert client.api_key == "sk-from-dotenv"
    assert client.base_url == "https://everos.example.test"
    assert client.timeout == 3.0


def test_search_memories_uses_hybrid_defaults_and_filter(monkeypatch):
    from everos_hermes.client import EverOSClient

    captured = {}

    def fake_urlopen(req, timeout):
        captured["url"] = req.full_url
        captured["body"] = json.loads(req.data.decode("utf-8"))
        return FakeHTTPResponse({"data": {"episodes": []}})

    monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)
    client = EverOSClient(api_key="sk-test")

    client.search_memories(query="coffee preference", user_id="user_001")

    assert captured["url"] == "https://api.evermind.ai/api/v1/memories/search"
    assert captured["body"] == {
        "query": "coffee preference",
        "filters": {"user_id": "user_001"},
        "method": "hybrid",
        "memory_types": ["episodic_memory", "profile"],
        "top_k": 5,
        "include_original_data": False,
    }


def test_search_memories_strips_vectors_from_original_data_by_default(monkeypatch):
    from everos_hermes.client import EverOSClient

    payload = {
        "data": {
            "episodes": [{"id": "ep1", "vector": [0.1, 0.2], "summary": "Coffee"}],
            "original_data": {
                "episodes": {
                    "ep1": {"id": "ep1", "vector": [0.1, 0.2], "nested": {"embedding": [0.3]}}
                }
            },
        }
    }

    def fake_urlopen(req, timeout):
        return FakeHTTPResponse(payload)

    monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)
    client = EverOSClient(api_key="sk-test")

    result = client.search_memories(query="coffee", user_id="user_001", include_original_data=True)

    rendered = json.dumps(result)
    assert "vector" not in rendered
    assert "embedding" not in rendered
    assert result["data"]["episodes"][0]["summary"] == "Coffee"


def test_search_memories_can_keep_vectors_for_debug(monkeypatch):
    from everos_hermes.client import EverOSClient

    payload = {"data": {"episodes": [{"id": "ep1", "vector": [0.1, 0.2]}]}}

    def fake_urlopen(req, timeout):
        return FakeHTTPResponse(payload)

    monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)
    client = EverOSClient(api_key="sk-test")

    result = client.search_memories(query="coffee", user_id="user_001", include_vectors=True)

    assert result["data"]["episodes"][0]["vector"] == [0.1, 0.2]


def test_flush_memories_accepts_per_call_timeout(monkeypatch):
    from everos_hermes.client import EverOSClient

    captured = {}

    def fake_urlopen(req, timeout):
        captured["timeout"] = timeout
        captured["body"] = json.loads(req.data.decode("utf-8"))
        return FakeHTTPResponse({"data": {"status": "extracted"}})

    monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)
    client = EverOSClient(api_key="sk-test", timeout=7)

    client.flush_memories(user_id="user_001", session_id="sess-1", timeout=31)

    assert captured == {"timeout": 31, "body": {"user_id": "user_001", "session_id": "sess-1"}}


def test_timeout_error_is_actionable(monkeypatch):
    from everos_hermes.client import EverOSClient, EverOSTimeoutError

    def fake_urlopen(req, timeout):
        raise TimeoutError()

    monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)
    client = EverOSClient(api_key="sk-test")

    with pytest.raises(EverOSTimeoutError) as exc:
        client.flush_memories(user_id="user_001", session_id="sess-1")

    message = str(exc.value)
    assert "timed out" in message
    assert "search" in message
    assert exc.value.retryable is True


def test_http_error_includes_everos_message(monkeypatch):
    from everos_hermes.client import EverOSClient, EverOSError

    def fake_urlopen(req, timeout):
        payload = json.dumps({"code": "InvalidParameter", "message": "user_id: Field required"}).encode()
        raise urllib.error.HTTPError(req.full_url, 422, "Unprocessable Entity", hdrs=None, fp=FakeHTTPErrorBody(payload))

    class FakeHTTPErrorBody:
        def __init__(self, payload):
            self.payload = payload

        def read(self):
            return self.payload

        def close(self):
            return None

    monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)
    client = EverOSClient(api_key="sk-test")

    with pytest.raises(EverOSError) as exc:
        client.get_memories(user_id="user_001", memory_type="profile")

    assert "422" in str(exc.value)
    assert "InvalidParameter" in str(exc.value)
    assert "user_id: Field required" in str(exc.value)



def test_http_error_redacts_backend_secret_values_and_truncates(monkeypatch):
    from everos_hermes.client import EverOSClient, EverOSError

    bearer_token = "abc" + "+def/" + "ghi=~tail"
    secret_value = "quoted," + "semi;" + "with]delimiters"

    class FakeHTTPErrorBody:
        def __init__(self, payload):
            self.payload = payload

        def read(self):
            return self.payload

        def close(self):
            return None

    def fake_urlopen(req, timeout):
        message = (
            "backend failed api_" + "key=\"" + secret_value + "\" "
            "Authorization: Bearer " + bearer_token + " request_id=req-42 " + ("x" * 1000)
        )
        payload = json.dumps({"code": "BackendFailure", "message": message}).encode()
        raise urllib.error.HTTPError(req.full_url, 500, "Internal Server Error", hdrs=None, fp=FakeHTTPErrorBody(payload))

    monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)
    client = EverOSClient(api_key="sk-" + "test")

    with pytest.raises(EverOSError) as exc:
        client.get_task_status("task-1")

    message = str(exc.value)
    assert secret_value not in message
    assert bearer_token not in message
    assert "[REDACTED]" in message
    assert "request_id=req-42" in message
    assert len(message) < 650


def test_add_memories_supports_scope_agent_and_agent_alias(monkeypatch):
    from everos_hermes.client import EverOSClient

    paths = []

    def fake_urlopen(req, timeout):
        paths.append(req.full_url)
        return FakeHTTPResponse({"data": {"status": "queued"}}, status=202)

    monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)
    client = EverOSClient(api_key="sk-test")
    message = {"role": "tool", "timestamp": 1711900000000, "content": "tool output", "tool_call_id": "tool-call-1"}

    client.add_memories(user_id="user_001", messages=[message], scope="agent")
    client.add_memories(user_id="user_001", messages=[message], agent=True)

    assert paths == [
        "https://api.evermind.ai/api/v1/memories/agent",
        "https://api.evermind.ai/api/v1/memories/agent",
    ]


def test_add_memories_validates_messages_before_request(monkeypatch):
    from everos_hermes.client import EverOSClient

    called = False

    def fake_urlopen(req, timeout):
        nonlocal called
        called = True
        return FakeHTTPResponse({"data": {}}, status=202)

    monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)
    client = EverOSClient(api_key="sk-test")

    with pytest.raises(ValueError, match="1..500"):
        client.add_memories(user_id="user_001", messages=[])
    with pytest.raises(ValueError, match="role"):
        client.add_memories(user_id="user_001", messages=[{"role": "tool", "timestamp": 1, "content": "x"}], scope="personal")
    with pytest.raises(ValueError, match="scope"):
        client.add_memories(user_id="user_001", messages=[{"role": "user", "timestamp": 1, "content": "x"}], scope="personal", agent=True)
    assert called is False


def test_search_memories_allows_top_k_minus_one_filters_radius_and_rejects_bad_type(monkeypatch):
    from everos_hermes.client import EverOSClient

    captured = {}

    def fake_urlopen(req, timeout):
        captured["body"] = json.loads(req.data.decode("utf-8"))
        captured["timeout"] = timeout
        return FakeHTTPResponse({"data": {"episodes": []}})

    monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)
    client = EverOSClient(api_key="sk-test", timeout=7)

    client.search_memories(
        query="all",
        user_id="user_001",
        filters={"AND": [{"timestamp": {"gte": 1700000000000}}]},
        top_k=-1,
        radius=0.5,
        memory_types=["agent_memory"],
        timeout=60,
    )

    assert captured["timeout"] == 60
    assert captured["body"]["top_k"] == -1
    assert captured["body"]["radius"] == 0.5
    assert captured["body"]["memory_types"] == ["agent_memory"]
    assert captured["body"]["filters"] == {
        "user_id": "user_001",
        "AND": [{"timestamp": {"gte": 1700000000000}}],
    }
    with pytest.raises(ValueError, match="memory_types"):
        client.search_memories(query="bad", user_id="user_001", memory_types=["agent_case"])
    with pytest.raises(ValueError, match="user_id"):
        client.search_memories(query="bad")


def test_flush_memories_supports_agent_scope(monkeypatch):
    from everos_hermes.client import EverOSClient

    captured = {}

    def fake_urlopen(req, timeout):
        captured["url"] = req.full_url
        captured["body"] = json.loads(req.data.decode("utf-8"))
        return FakeHTTPResponse({"data": {"status": "extracted"}})

    monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)
    client = EverOSClient(api_key="sk-test")

    client.flush_memories(user_id="user_001", session_id="sess-1", scope="agent")

    assert captured["url"] == "https://api.evermind.ai/api/v1/memories/agent/flush"
    assert captured["body"] == {"user_id": "user_001", "session_id": "sess-1"}


def test_get_memories_validates_type_pagination_and_rank(monkeypatch):
    from everos_hermes.client import EverOSClient

    captured = {}

    def fake_urlopen(req, timeout):
        captured["body"] = json.loads(req.data.decode("utf-8"))
        return FakeHTTPResponse({"data": {"items": []}})

    monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)
    client = EverOSClient(api_key="sk-test")

    client.get_memories(user_id="user_001", memory_type="agent_case", rank_order="DESC")
    assert captured["body"]["memory_type"] == "agent_case"
    assert captured["body"]["rank_order"] == "desc"
    with pytest.raises(ValueError, match="memory_type"):
        client.get_memories(user_id="user_001", memory_type="agent_memory")
    with pytest.raises(ValueError, match="page_size"):
        client.get_memories(user_id="user_001", page_size=101)


def test_delete_memories_strict_modes_and_204_payload(monkeypatch):
    from everos_hermes.client import EverOSClient

    captured = {}

    def fake_urlopen(req, timeout):
        captured["body"] = json.loads(req.data.decode("utf-8"))
        return FakeHTTPResponse(None, status=204)

    monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)
    client = EverOSClient(api_key="sk-test")

    with pytest.raises(ValueError, match="single delete"):
        client.delete_memories(memory_id="mem-1", user_id="user_001")
    with pytest.raises(ValueError, match="explicit user_id"):
        client.delete_memories(session_id="sess-1")

    result = client.delete_memories(memory_id="mem-1")
    assert result == {"ok": True, "status_code": 204, "deleted": True, "mode": "single"}
    assert captured["body"] == {"memory_id": "mem-1"}


def test_update_settings_validates_strict_schema_and_returns_diff(monkeypatch):
    from everos_hermes.client import EverOSClient

    calls = []

    def fake_urlopen(req, timeout):
        calls.append((req.get_method(), req.full_url, None if req.data is None else json.loads(req.data.decode("utf-8"))))
        if req.get_method() == "GET" and len(calls) == 1:
            return FakeHTTPResponse({"data": {"timezone": "UTC", "llm_custom_setting": {}}})
        if req.get_method() == "PUT":
            return FakeHTTPResponse({"data": {"timezone": "Asia/Tokyo", "llm_custom_setting": {}}})
        return FakeHTTPResponse({"data": {"timezone": "Asia/Tokyo", "llm_custom_setting": {}}})

    monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)
    client = EverOSClient(api_key="sk-test")

    result = client.update_settings({"timezone": "Asia/Tokyo"})

    assert calls[1] == ("PUT", "https://api.evermind.ai/api/v1/settings", {"timezone": "Asia/Tokyo"})
    assert result["diff"]["timezone"] == {"before": "UTC", "after": "Asia/Tokyo"}
    with pytest.raises(ValueError, match="Unknown settings"):
        client.update_settings({"extraction_mode": "fast"})



def test_request_json_success_envelope_contract_cases(monkeypatch):
    from pathlib import Path

    from everos_hermes.client import EverOSClient

    cases = json.loads((Path(__file__).parent / "contracts" / "http_response_envelope_cases.json").read_text(encoding="utf-8"))["cases"]

    for case in cases:
        if case["operation"] != "request_json":
            continue

        def fake_urlopen(req, timeout, *, case=case):
            response = case["server_response"]
            return FakeHTTPResponse(response["body"], status=response["status"])

        monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)
        client = EverOSClient(api_key="sk-test")
        request = case["request"]
        assert client.request_json(request["method"], request["path"]) == case["expected_response"]


def test_delete_memories_204_envelope_contract_case(monkeypatch):
    from pathlib import Path

    from everos_hermes.client import EverOSClient

    case = next(
        item
        for item in json.loads((Path(__file__).parent / "contracts" / "http_response_envelope_cases.json").read_text(encoding="utf-8"))["cases"]
        if item["operation"] == "delete_memories"
    )
    captured = {}

    def fake_urlopen(req, timeout):
        captured["body"] = json.loads(req.data.decode("utf-8"))
        response = case["server_response"]
        return FakeHTTPResponse(response["body"], status=response["status"])

    monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)
    client = EverOSClient(api_key="sk-test")

    assert client.delete_memories(**case["args"]) == case["expected_response"]
    assert captured["body"] == case["expected_request"]["body"]


def test_client_param_normalization_contract_cases(monkeypatch):
    from pathlib import Path

    from everos_hermes.client import EverOSClient

    cases = json.loads((Path(__file__).parent / "contracts" / "client_param_normalization_cases.json").read_text(encoding="utf-8"))["cases"]
    for case in cases:
        if not case["surface"].startswith("client."):
            continue
        captured = {}

        def fake_urlopen(req, timeout):
            captured["path"] = "/" + req.full_url.split("/", 3)[3]
            captured["body"] = json.loads(req.data.decode("utf-8"))
            return FakeHTTPResponse({"data": {"items": [], "episodes": []}})

        monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)
        client = EverOSClient(api_key="sk-test")
        if case["surface"] == "client.search":
            client.search_memories(**case["args"])
        elif case["surface"] == "client.get":
            client.get_memories(**case["args"])
        assert captured["path"] == case["expected_request"]["path"]
        for key, value in case["expected_request"]["body_subset"].items():
            assert captured["body"][key] == value


def test_session_filter_requires_exact_non_empty_session_id_operator():
    from everos_hermes.client import EverOSClient

    client = EverOSClient(api_key="sk-test", base_url="http://127.0.0.1:9", timeout=0.1)
    for filters in [
        {"session_id": {}},
        {"session_id": {"eq": ""}},
        {"session_id": {"eq": 123}},
        {"session_id": ""},
        {"AND": [{"session_id": {"eq": 123}}]},
    ]:
        with pytest.raises(ValueError, match="session_id"):
            client.search_memories(query="coffee", user_id="u1", session_id="sess", filters=filters)
