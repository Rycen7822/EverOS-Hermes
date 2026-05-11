import json
import urllib.error

import pytest


class FakeHTTPResponse:
    def __init__(self, payload: dict, status: int = 200):
        self.payload = payload
        self.status = status

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        return False

    def read(self):
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
        client.get_memories(user_id="", memory_type="profile")

    assert "422" in str(exc.value)
    assert "InvalidParameter" in str(exc.value)
    assert "user_id: Field required" in str(exc.value)
