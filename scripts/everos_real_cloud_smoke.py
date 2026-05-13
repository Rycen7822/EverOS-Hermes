#!/usr/bin/env python3
"""Real EverOS Cloud smoke test for EverOS-Hermes agent visibility.

This script drives an everos-hermes-rust binary through MCP stdio. It writes only to an
explicit non-default user_id/session_id and performs session-scoped structured cleanup.
It never prints API keys.
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import time
from pathlib import Path
from typing import Any

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))
from everos_agent_visibility_smoke import MCPClient  # noqa: E402

DEFAULT_USER_ID = "hermes_mcp_stress_main_20260512_125809"
ALLOWED_VISIBILITY = {"unchecked", "not_visible", "partial", "visible"}


def call(client: MCPClient, name: str, arguments: dict[str, Any]) -> tuple[dict[str, Any], dict[str, Any]]:
    result, parsed = client.call_tool(name, arguments)
    if result.get("isError"):
        raise RuntimeError(f"MCP tool {name} returned error: {json.dumps(parsed, ensure_ascii=False)}")
    if isinstance(parsed, dict) and parsed.get("error"):
        raise RuntimeError(f"MCP tool {name} returned error payload: {json.dumps(parsed, ensure_ascii=False)}")
    return result, parsed


def assert_true(failures: list[str], name: str, condition: bool, detail: str = "") -> None:
    if not condition:
        failures.append(f"{name}: {detail}" if detail else name)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--user-id", default=DEFAULT_USER_ID, help="Must not be hermes_default")
    parser.add_argument("--session-id", default="", help="Optional explicit smoke session_id")
    parser.add_argument("--output", required=True)
    parser.add_argument("--base-url", default="", help="Optional EVEROS_BASE_URL override")
    parser.add_argument("--timeout", type=float, default=30.0)
    args = parser.parse_args()

    if args.user_id == "hermes_default":
        raise SystemExit("Refusing to run real Cloud smoke against hermes_default")

    session_id = args.session_id or f"eh_real_cloud_smoke_{time.strftime('%Y%m%d_%H%M%S')}_{os.getpid()}"
    marker = f"EverOS-Hermes real cloud agent visibility smoke marker {session_id}"

    env = os.environ.copy()
    env["EVEROS_USER_ID"] = args.user_id
    if args.base_url:
        env["EVEROS_BASE_URL"] = args.base_url
    env.setdefault("EVEROS_TIMEOUT", str(args.timeout))

    client = MCPClient(str(Path(args.binary).expanduser()), env)
    failures: list[str] = []
    cleanup_payload: dict[str, Any] | None = None
    save_payload: dict[str, Any] | None = None
    verify_payload: dict[str, Any] | None = None
    post_cleanup_agent_memory: dict[str, Any] | None = None
    try:
        init = client.request(
            "initialize",
            {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "everos-real-cloud-smoke", "version": "0"}},
        )
        assert_true(failures, "initialize", init.get("result", {}).get("serverInfo", {}).get("name") == "everos_mcp", json.dumps(init, ensure_ascii=False))

        tools = client.request("tools/list", {})
        tool_count = len(tools.get("result", {}).get("tools", []))
        assert_true(failures, "tools/list has thirteen tools", tool_count == 13, f"tool_count={tool_count}")

        _, save_payload = call(
            client,
            "everos_save_and_verify",
            {
                "user_id": args.user_id,
                "session_id": session_id,
                "scope": "agent",
                "content": marker,
                "verification_query": marker,
                "flush": True,
                "top_k": 3,
                "timeout": args.timeout,
            },
        )
        visibility = (save_payload.get("agent_visibility") or {}) if isinstance(save_payload, dict) else {}
        status = visibility.get("agent_visibility_status")
        assert_true(failures, "save_and_verify ok", bool(save_payload.get("ok")), json.dumps(save_payload, ensure_ascii=False))
        assert_true(failures, "agent visibility status present", status in ALLOWED_VISIBILITY, json.dumps(save_payload, ensure_ascii=False))
        assert_true(failures, "agent raw queued flag present", "agent_raw_queued" in visibility, json.dumps(save_payload, ensure_ascii=False))
        assert_true(failures, "agent flush field present", "agent_flush" in visibility, json.dumps(save_payload, ensure_ascii=False))

        _, verify_payload = call(
            client,
            "everos_verify_session_ingest",
            {
                "user_id": args.user_id,
                "session_id": session_id,
                "scope": "agent",
                "verification_queries": [marker],
                "top_k": 3,
                "timeout": args.timeout,
            },
        )
        verify_visibility = (verify_payload.get("agent_visibility") or {}) if isinstance(verify_payload, dict) else {}
        verify_status = verify_visibility.get("agent_visibility_status")
        assert_true(failures, "verify_session_ingest visibility status present", verify_status in ALLOWED_VISIBILITY, json.dumps(verify_payload, ensure_ascii=False))

        _, cleanup_payload = call(
            client,
            "everos_delete_memories",
            {
                "user_id": args.user_id,
                "session_id": session_id,
                "confirm": True,
                "confirm_scope_text": f"delete user_id={args.user_id} session_id={session_id}",
            },
        )
        assert_true(failures, "cleanup payload returned", isinstance(cleanup_payload, dict), json.dumps(cleanup_payload, ensure_ascii=False))

        # Structured cleanup probe only; raw residual is a known Cloud limitation and is not used as a failure gate.
        _, post_cleanup_agent_memory = call(
            client,
            "everos_search_memories",
            {
                "user_id": args.user_id,
                "session_id": session_id,
                "query": marker,
                "memory_types": ["agent_memory"],
                "method": "hybrid",
                "top_k": 3,
                "timeout": args.timeout,
            },
        )

        summary = {
            "ok": not failures,
            "user_id": args.user_id,
            "session_id": session_id,
            "marker": marker,
            "tool_count": tool_count,
            "save_status": save_payload.get("status") if isinstance(save_payload, dict) else None,
            "agent_visibility_status": status,
            "verify_status": verify_payload.get("status") if isinstance(verify_payload, dict) else None,
            "verify_agent_visibility_status": verify_status,
            "cleanup_payload": cleanup_payload,
            "post_cleanup_agent_memory_search": post_cleanup_agent_memory,
            "failures": failures,
            "notes": [
                "session-scoped cleanup was requested for the non-default user_id",
                "raw_message residuals are not used as a failure gate because Cloud delete may leave raw residuals",
                "API keys are not included in this summary",
            ],
        }
        Path(args.output).parent.mkdir(parents=True, exist_ok=True)
        Path(args.output).write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({
            "ok": summary["ok"],
            "user_id": args.user_id,
            "session_id": session_id,
            "agent_visibility_status": status,
            "verify_agent_visibility_status": verify_status,
            "failures": failures,
        }, ensure_ascii=False, indent=2))
        return 0 if not failures else 1
    finally:
        client.close()


if __name__ == "__main__":
    raise SystemExit(main())
