#!/usr/bin/env python3
"""Fake EverOS Cloud smoke test for EverOS-Hermes agent visibility.

Runs a local HTTP fake server and drives an everos-hermes-rust binary via MCP stdio.
The script intentionally validates queue/flush/structured visibility as separate states.
"""
from __future__ import annotations

import argparse
import json
import os
import select
import socket
import subprocess
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from typing import Any

FORBIDDEN_PATH_PARTS = ("/groups", "/senders", "/object", "/multimodal", "/memories/group")


class SmokeState:
    def __init__(self) -> None:
        self.requests: list[dict[str, Any]] = []
        self.lock = threading.Lock()
        self.fail_next_agent_flush = False
        self.transient_retry_attempts = 0

    def record(self, *, method: str, path: str, headers: dict[str, str], body: Any) -> None:
        redacted = dict(headers)
        for key in list(redacted):
            if key.lower() == "authorization":
                redacted[key] = "Bearer ***"
        with self.lock:
            self.requests.append({"method": method, "path": path, "headers": redacted, "body": body})

    def paths(self) -> list[str]:
        with self.lock:
            return [req["path"] for req in self.requests]

    def snapshot(self) -> list[dict[str, Any]]:
        with self.lock:
            return list(self.requests)


class FakeEverOSHandler(BaseHTTPRequestHandler):
    state: SmokeState

    server_version = "FakeEverOS/agent-visibility-smoke"
    sys_version = ""

    def log_message(self, format: str, *args: Any) -> None:  # noqa: A002 - stdlib signature
        return

    def do_GET(self) -> None:  # noqa: N802 - stdlib hook
        self._handle({})

    def do_PUT(self) -> None:  # noqa: N802 - stdlib hook
        self._handle(self._read_json_body())

    def do_POST(self) -> None:  # noqa: N802 - stdlib hook
        body = self._read_json_body()
        if self.path == "/api/v1/memories/agent/flush" and self.state.fail_next_agent_flush:
            self.state.fail_next_agent_flush = False
            self.state.transient_retry_attempts += 1
            self.state.record(method="POST", path=self.path, headers=dict(self.headers), body=body)
            try:
                self.connection.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            self.connection.close()
            return
        self._handle(body)

    def _read_json_body(self) -> Any:
        length = int(self.headers.get("Content-Length", "0") or "0")
        raw = self.rfile.read(length) if length else b""
        if not raw:
            return {}
        try:
            return json.loads(raw.decode("utf-8"))
        except Exception:
            return {"_raw": raw.decode("utf-8", errors="replace")}

    def _handle(self, body: Any) -> None:
        method = self.command
        path = self.path.split("?", 1)[0]
        self.state.record(method=method, path=path, headers=dict(self.headers), body=body)
        response = self._response_for(method, path, body)
        encoded = json.dumps(response, ensure_ascii=False).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(encoded)

    def _response_for(self, method: str, path: str, body: Any) -> dict[str, Any]:
        if method == "GET" and path == "/api/v1/settings":
            return {"data": {"timezone": "UTC", "llm_custom_setting": {}}}
        if method == "PUT" and path == "/api/v1/settings":
            return {"data": {"status": "updated"}}
        if path in ("/api/v1/memories", "/api/v1/memories/agent"):
            return {"data": {"status": "queued", "task_id": "smoke-task"}}
        if path in ("/api/v1/memories/flush", "/api/v1/memories/agent/flush"):
            return {"data": {"status": "success", "task_id": "smoke-flush"}}
        if path == "/api/v1/memories/search":
            return self._search_response(body if isinstance(body, dict) else {})
        if path == "/api/v1/memories/get":
            return self._get_response(body if isinstance(body, dict) else {})
        if path == "/api/v1/memories/delete":
            return {"ok": True, "status_code": 204, "deleted": True}
        if path.startswith("/api/v1/tasks/"):
            return {"data": {"status": "success"}}
        return {"data": {"status": "ok"}}

    def _case_from_text(self, text: str) -> str:
        lowered = text.lower()
        if "smoke-visible" in lowered:
            return "visible"
        if "smoke-partial" in lowered:
            return "partial"
        return "not_visible"

    def _search_response(self, body: dict[str, Any]) -> dict[str, Any]:
        query = str(body.get("query") or "")
        case = self._case_from_text(query)
        memory_types = body.get("memory_types") or []
        if memory_types == ["agent_memory"]:
            if case in {"visible", "partial"}:
                return {"data": {"agent_memory": [{"id": f"agent-{case}", "content": query}]}}
            return {"data": {"agent_memory": []}}
        return {"data": {"episodes": [{"id": f"episode-{case}", "summary": query or case}]}}

    def _get_response(self, body: dict[str, Any]) -> dict[str, Any]:
        memory_type = str(body.get("memory_type") or "")
        # The workflow does not put query text in get requests. Use request order/body session marker.
        session = str(body.get("filters", {}).get("AND", [{}])[0].get("session_id", "") if isinstance(body.get("filters"), dict) else body.get("session_id", ""))
        case = self._case_from_text(json.dumps(body, ensure_ascii=False) + " " + session)
        if case == "visible" and memory_type == "agent_case":
            return {"data": {"agent_cases": [{"id": "case-visible", "summary": "visible agent case"}]}}
        if case == "visible" and memory_type == "agent_skill":
            return {"data": {"agent_skills": [{"id": "skill-visible", "summary": "visible agent skill"}]}}
        if case == "partial" and memory_type == "agent_case":
            return {"data": {"agent_cases": []}}
        if case == "partial" and memory_type == "agent_skill":
            return {"data": {"agent_skills": []}}
        if memory_type == "agent_case":
            return {"data": {"agent_cases": []}}
        if memory_type == "agent_skill":
            return {"data": {"agent_skills": []}}
        return {"data": {"items": []}}


class MCPClient:
    def __init__(self, binary: str, env: dict[str, str]) -> None:
        self.proc = subprocess.Popen(
            [binary, "mcp"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=env,
            text=False,
        )
        self.next_id = 1

    def close(self) -> None:
        if self.proc.stdin:
            try:
                self.proc.stdin.close()
            except Exception:
                pass
        try:
            self.proc.terminate()
            self.proc.wait(timeout=2)
        except Exception:
            self.proc.kill()
            self.proc.wait(timeout=2)

    def request(self, method: str, params: dict[str, Any] | None = None) -> dict[str, Any]:
        request_id = self.next_id
        self.next_id += 1
        payload = {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params or {}}
        self._write_frame(payload)
        response = self._read_frame(timeout=10)
        if response.get("id") != request_id:
            raise AssertionError(f"unexpected MCP id: {response}")
        return response

    def call_tool(self, name: str, arguments: dict[str, Any]) -> tuple[dict[str, Any], dict[str, Any]]:
        response = self.request("tools/call", {"name": name, "arguments": arguments})
        result = response.get("result") or {}
        text = ((result.get("content") or [{}])[0]).get("text", "")
        parsed = None
        if text:
            try:
                parsed = json.loads(text)
            except Exception:
                parsed = {"_text": text}
        else:
            parsed = {}
        return result, parsed

    def _write_frame(self, payload: dict[str, Any]) -> None:
        assert self.proc.stdin is not None
        body = json.dumps(payload, separators=(",", ":")).encode("utf-8")
        frame = b"Content-Length: " + str(len(body)).encode("ascii") + b"\r\n\r\n" + body
        self.proc.stdin.write(frame)
        self.proc.stdin.flush()

    def _read_frame(self, timeout: float) -> dict[str, Any]:
        assert self.proc.stdout is not None
        fd = self.proc.stdout.fileno()
        if not select.select([fd], [], [], timeout)[0]:
            stderr = self._stderr_tail()
            raise TimeoutError(f"timeout waiting for MCP response; stderr={stderr}")
        first = self.proc.stdout.read(1)
        if not first:
            raise RuntimeError(f"MCP process exited rc={self.proc.poll()} stderr={self._stderr_tail()}")
        raw = bytearray(first)
        if first == b"{":
            while not raw.endswith(b"\n"):
                raw.extend(self.proc.stdout.read(1))
            return json.loads(bytes(raw).rstrip(b"\n"))
        while not raw.endswith(b"\r\n\r\n"):
            raw.extend(self.proc.stdout.read(1))
        header = raw.decode("utf-8", errors="replace")
        length = None
        for line in header.splitlines():
            if line.lower().startswith("content-length:"):
                length = int(line.split(":", 1)[1].strip())
                break
        if length is None:
            raise RuntimeError(f"missing Content-Length header: {header!r}")
        body = self.proc.stdout.read(length)
        return json.loads(body.decode("utf-8"))

    def _stderr_tail(self) -> str:
        if not self.proc.stderr:
            return ""
        fd = self.proc.stderr.fileno()
        chunks: list[bytes] = []
        while select.select([fd], [], [], 0)[0]:
            chunk = os.read(fd, 4096)
            if not chunk:
                break
            chunks.append(chunk)
        return b"".join(chunks)[-4000:].decode("utf-8", errors="replace")


def start_fake_server(state: SmokeState) -> tuple[HTTPServer, str, threading.Thread]:
    class Handler(FakeEverOSHandler):
        pass

    Handler.state = state
    server = HTTPServer(("127.0.0.1", 0), Handler)
    address = server.server_address
    host = str(address[0])
    port = int(address[1])
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server, f"http://{host}:{port}", thread


def assert_true(assertions: list[str], failures: list[str], name: str, condition: bool, detail: str = "") -> None:
    if condition:
        assertions.append(name)
    else:
        failures.append(f"{name}: {detail}" if detail else name)


def call_visibility_case(client: MCPClient, marker: str, session_id: str) -> dict[str, Any]:
    _, parsed = client.call_tool(
        "everos_save_and_verify",
        {
            "content": marker,
            "session_id": session_id,
            "scope": "agent",
            "verification_query": marker,
            "flush": True,
            "top_k": 3,
        },
    )
    return parsed


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--mode", required=True, choices=["build-tree", "installed"])
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    binary = str(Path(args.binary).expanduser())
    state = SmokeState()
    server, base_url, thread = start_fake_server(state)
    env = os.environ.copy()
    env.update(
        {
            "EVEROS_BASE_URL": base_url,
            "EVEROS_USER_ID": "smoke_user",
            "EVEROS_API_KEY": "smoke_key",
            "EVEROS_TIMEOUT": "5",
            "RUST_BACKTRACE": "0",
        }
    )
    client = MCPClient(binary, env)
    assertions: list[str] = []
    failures: list[str] = []
    visibility_cases: dict[str, str] = {}
    tool_count = 0
    try:
        init = client.request(
            "initialize",
            {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "agent-visibility-smoke", "version": "0"}},
        )
        assert_true(assertions, failures, "initialize server", init.get("result", {}).get("serverInfo", {}).get("name") == "everos_mcp", str(init))

        tools = client.request("tools/list", {})
        tool_items = tools.get("result", {}).get("tools", [])
        tool_count = len(tool_items)
        assert_true(assertions, failures, "tools/list returns 13 tools", tool_count == 13, f"tool_count={tool_count}")

        for case, expected in [
            ("smoke-not-visible", "not_visible"),
            ("smoke-partial", "partial"),
            ("smoke-visible", "visible"),
        ]:
            parsed = call_visibility_case(client, case, f"sess-{case}")
            status = parsed.get("agent_visibility", {}).get("agent_visibility_status")
            visibility_cases[case] = str(status)
            assert_true(assertions, failures, f"{case} visibility={expected}", status == expected, json.dumps(parsed, ensure_ascii=False))
            assert_true(assertions, failures, f"{case} has raw queued", "agent_raw_queued" in parsed.get("agent_visibility", {}), json.dumps(parsed, ensure_ascii=False))
            assert_true(assertions, failures, f"{case} has flush", "agent_flush" in parsed.get("agent_visibility", {}), json.dumps(parsed, ensure_ascii=False))
            assert_true(assertions, failures, f"{case} has checks", bool(parsed.get("agent_visibility", {}).get("agent_visibility_checks")), json.dumps(parsed, ensure_ascii=False))

        _, add_parsed = client.call_tool(
            "everos_add_memories",
            {"scope": "agent", "session_id": "sess-add", "messages": [{"role": "assistant", "timestamp": 1711900000000, "content": "add smoke"}]},
        )
        assert_true(
            assertions,
            failures,
            "everos_add_memories agent unchecked",
            add_parsed.get("agent_visibility", {}).get("agent_visibility_status") == "unchecked",
            json.dumps(add_parsed, ensure_ascii=False),
        )

        _, save_parsed = client.call_tool(
            "everos_save_memory",
            {"scope": "agent", "session_id": "sess-save", "content": "save smoke", "flush": False},
        )
        assert_true(
            assertions,
            failures,
            "everos_save_memory agent unchecked",
            save_parsed.get("agent_visibility", {}).get("agent_visibility_status") == "unchecked",
            json.dumps(save_parsed, ensure_ascii=False),
        )

        before_tool_error_count = len(state.snapshot())
        result, parsed = client.call_tool(
            "everos_add_memories",
            {"scope": "agent", "session_id": "sess-tool", "messages": [{"role": "tool", "timestamp": 1711900000000, "content": "missing call id"}]},
        )
        after_tool_error_count = len(state.snapshot())
        is_error = bool(result.get("isError")) or "tool_call_id" in json.dumps(parsed, ensure_ascii=False)
        assert_true(assertions, failures, "role=tool missing tool_call_id fails locally", is_error, json.dumps(result, ensure_ascii=False))
        assert_true(assertions, failures, "role=tool missing tool_call_id sends no HTTP", after_tool_error_count == before_tool_error_count, f"before={before_tool_error_count} after={after_tool_error_count}")

        state.fail_next_agent_flush = True
        _, flush_parsed = client.call_tool("everos_flush_memories", {"scope": "agent", "session_id": "sess-transient", "timeout": 5})
        assert_true(
            assertions,
            failures,
            "flush transient retry attempt_count=2",
            flush_parsed.get("flush", {}).get("attempt_count") == 2,
            json.dumps(flush_parsed, ensure_ascii=False),
        )
        assert_true(
            assertions,
            failures,
            "flush transient returns unchecked visibility",
            flush_parsed.get("agent_visibility", {}).get("agent_visibility_status") == "unchecked",
            json.dumps(flush_parsed, ensure_ascii=False),
        )

        paths_seen = state.paths()
        assert_true(
            assertions,
            failures,
            "no forbidden endpoint paths",
            not any(any(part in path for part in FORBIDDEN_PATH_PARTS) for path in paths_seen),
            json.dumps(paths_seen),
        )
        requests = state.snapshot()
        redacted_ok = all(req.get("headers", {}).get("Authorization") in (None, "Bearer ***") for req in requests)
        assert_true(assertions, failures, "authorization redacted in summary", redacted_ok)

        summary = {
            "mode": args.mode,
            "binary": binary,
            "base_url": base_url,
            "tool_count": tool_count,
            "assertions_passed": len(assertions),
            "assertions_failed": failures,
            "paths_seen": paths_seen,
            "visibility_cases": visibility_cases,
            "transient_retry_attempts": state.transient_retry_attempts,
            "requests": requests,
        }
        Path(args.output).parent.mkdir(parents=True, exist_ok=True)
        Path(args.output).write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        if failures:
            print(json.dumps(summary, ensure_ascii=False, indent=2))
            return 1
        print(json.dumps({k: summary[k] for k in ("mode", "tool_count", "assertions_passed", "visibility_cases", "transient_retry_attempts")}, ensure_ascii=False, indent=2))
        return 0
    finally:
        client.close()
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)


if __name__ == "__main__":
    raise SystemExit(main())
