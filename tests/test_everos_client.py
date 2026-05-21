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

    captured = []

    def fake_urlopen(req, timeout):
        captured.append({
            "url": req.full_url,
            "method": req.get_method(),
            "headers": dict(req.header_items()),
            "body": json.loads(req.data.decode("utf-8")),
            "timeout": timeout,
        })
        return FakeHTTPResponse({"data": {"status": "queued", "task_id": "task-1"}}, status=202)

    monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)
    client = EverOSClient(api_key="sk-test", base_url="https://api.evermind.ai", timeout=7)
    message = {"role": "user", "timestamp": 1711900000000, "content": "I like black coffee."}
    agent_message = {"role": "tool", "timestamp": 1711900000000, "content": "tool output", "tool_call_id": "tool-call-1"}

    result = client.add_memories(user_id="user_001", session_id="session_001", messages=[message], async_mode=True)
    client.add_memories(user_id="user_001", messages=[agent_message], scope="agent")
    client.add_memories(user_id="user_001", messages=[agent_message], agent=True)

    assert result["data"]["task_id"] == "task-1"
    assert captured[0] == {
        "url": "https://api.evermind.ai/api/v1/memories",
        "method": "POST",
        "headers": {
            "Authorization": "Bearer sk-test",
            "Content-type": "application/json",
            "Accept": "application/json",
        },
        "body": {"user_id": "user_001", "session_id": "session_001", "messages": [message], "async_mode": True},
        "timeout": 7,
    }
    assert [call["url"] for call in captured[1:]] == [
        "https://api.evermind.ai/api/v1/memories/agent",
        "https://api.evermind.ai/api/v1/memories/agent",
    ]

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


def test_search_memories_uses_defaults_and_boundary_overrides(monkeypatch):
    from everos_hermes.client import EverOSClient

    captured = []

    def fake_urlopen(req, timeout):
        captured.append({"url": req.full_url, "body": json.loads(req.data.decode("utf-8")), "timeout": timeout})
        return FakeHTTPResponse({"data": {"items": [], "episodes": []}})

    monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)
    client = EverOSClient(api_key="sk-test")

    client.search_memories(query="coffee preference", user_id="user_001", method=" HYBRID ")
    client.search_memories(
        query="all",
        user_id="user_001",
        filters={"AND": [{"timestamp": {"gte": 1700000000000}}]},
        top_k=-1,
        radius=0.5,
        memory_types=["agent_memory"],
        timeout=60,
    )
    client.get_memories(user_id="user_001", memory_type="profile", rank_order=" DESC ")

    assert captured[0]["url"] == "https://api.evermind.ai/api/v1/memories/search"
    assert captured[0]["body"] == {
        "query": "coffee preference",
        "filters": {"user_id": "user_001"},
        "method": "hybrid",
        "memory_types": ["episodic_memory", "profile"],
        "top_k": 5,
        "include_original_data": False,
    }
    assert captured[1]["timeout"] == 60
    assert captured[1]["body"]["top_k"] == -1
    assert captured[1]["body"]["radius"] == 0.5
    assert captured[1]["body"]["memory_types"] == ["agent_memory"]
    assert captured[1]["body"]["filters"]["AND"][0]["timestamp"]["gte"] == 1700000000000
    assert captured[2]["url"] == "https://api.evermind.ai/api/v1/memories/get"
    assert captured[2]["body"]["rank_order"] == "desc"

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
    debug = client.search_memories(query="coffee", user_id="user_001", include_vectors=True)

    rendered = json.dumps(result)
    assert "vector" not in rendered
    assert "embedding" not in rendered
    assert result["data"]["episodes"][0]["summary"] == "Coffee"
    assert debug["data"]["episodes"][0]["vector"] == [0.1, 0.2]


def test_flush_memories_accepts_timeout_and_agent_scope(monkeypatch):
    from everos_hermes.client import EverOSClient

    captured = {}

    def fake_urlopen(req, timeout):
        captured.update({"url": req.full_url, "timeout": timeout, "body": json.loads(req.data.decode("utf-8"))})
        return FakeHTTPResponse({"data": {"status": "extracted"}})

    monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)
    client = EverOSClient(api_key="sk-test", timeout=7)

    client.flush_memories(user_id="user_001", session_id="sess-1", scope="agent", timeout=31)

    assert captured == {"url": "https://api.evermind.ai/api/v1/memories/agent/flush", "timeout": 31, "body": {"user_id": "user_001", "session_id": "sess-1"}}


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



def test_http_response_envelope_contract_cases(monkeypatch):
    from pathlib import Path

    from everos_hermes.client import EverOSClient

    cases = json.loads((Path(__file__).parent / "contracts" / "http_response_envelope_cases.json").read_text(encoding="utf-8"))["cases"]
    for case in cases:
        captured = {}

        def fake_urlopen(req, timeout, *, case=case):
            if getattr(req, "data", None):
                captured["body"] = json.loads(req.data.decode("utf-8"))
            response = case["server_response"]
            return FakeHTTPResponse(response["body"], status=response["status"])

        monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)
        client = EverOSClient(api_key="sk-test")
        if case["operation"] == "request_json":
            request = case["request"]
            actual = client.request_json(request["method"], request["path"])
        elif case["operation"] == "delete_memories":
            actual = client.delete_memories(**case["args"])
            assert captured["body"] == case["expected_request"]["body"]
        else:
            raise AssertionError(f"unsupported response envelope case: {case['operation']}")
        assert actual == case["expected_response"]
