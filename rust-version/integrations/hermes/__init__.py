"""Hermes memory-provider plugin shim for the Rust EverOS-Hermes runtime.

The Hermes plugin API is Python, so this file stays intentionally tiny: it
registers a MemoryProvider class and delegates all EverOS behavior to the Rust
binary via `everos-hermes-rust provider ...` commands.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import threading
from pathlib import Path
from typing import Any, Callable

try:
    from agent.memory_provider import MemoryProvider
except Exception:  # pragma: no cover - used outside Hermes during smoke tests
    from abc import ABC, abstractmethod

    class MemoryProvider(ABC):
        @property
        @abstractmethod
        def name(self) -> str: ...

        @abstractmethod
        def is_available(self) -> bool: ...

        @abstractmethod
        def initialize(self, session_id: str, **kwargs: Any) -> None: ...

        @abstractmethod
        def get_tool_schemas(self) -> list[dict[str, Any]]: ...

        def handle_tool_call(self, tool_name: str, args: dict[str, Any], **kwargs: Any) -> str:
            raise NotImplementedError(tool_name)

        def get_config_schema(self) -> list[dict[str, Any]]:
            return []

        def save_config(self, values: dict[str, Any], hermes_home: str) -> None:
            return None

        def system_prompt_block(self) -> str:
            return ""

        def prefetch(self, query: str, *, session_id: str = "") -> str:
            return ""

        def queue_prefetch(self, query: str, *, session_id: str = "") -> None:
            return None

        def sync_turn(self, user_content: str, assistant_content: str, *, session_id: str = "") -> None:
            return None

        def on_session_end(self, messages: list[dict[str, Any]]) -> None:
            return None

        def on_pre_compress(self, messages: list[dict[str, Any]]) -> str:
            return ""

        def on_memory_write(self, action: str, target: str, content: str, metadata: dict[str, Any] | None = None) -> None:
            return None

        def on_delegation(self, task: str, result: str, *, child_session_id: str = "", **kwargs: Any) -> None:
            return None

        def shutdown(self) -> None:
            return None


class EverOSRustMemoryProvider(MemoryProvider):
    def __init__(self) -> None:
        self._state: dict[str, Any] = {
            "session_id": "",
            "hermes_home": str(Path.home() / ".hermes"),
            "platform": "cli",
            "user_id": "",
            "user_name": "",
            "agent_identity": "default",
            "agent_context": "",
        }
        self._children: list[subprocess.Popen[str]] = []

    @property
    def name(self) -> str:
        return "everos"

    def is_available(self) -> bool:
        binary = self._binary_path()
        if not binary:
            return False
        result = self._run(["provider", "is-available", "--hermes-home", self._state["hermes_home"]], timeout=5)
        try:
            return bool(json.loads(result).get("available"))
        except Exception:
            return False

    def get_config_schema(self) -> list[dict[str, Any]]:
        return [
            {
                "key": "api_key",
                "description": "EverOS API key",
                "secret": True,
                "required": True,
                "env_var": "EVEROS_API_KEY",
                "url": "https://everos.evermind.ai/api-keys",
            },
            {
                "key": "user_id",
                "description": "Default EverOS user_id template (optional; gateway user_id can be used)",
                "required": False,
                "default": "",
                "env_var": "EVEROS_USER_ID",
            },
            {
                "key": "base_url",
                "description": "EverOS API base URL",
                "required": False,
                "default": "https://api.evermind.ai",
                "env_var": "EVEROS_BASE_URL",
            },
            {
                "key": "rust_binary",
                "description": "Optional path to everos-hermes-rust binary",
                "required": False,
                "env_var": "EVEROS_HERMES_RUST_BIN",
            },
        ]

    def save_config(self, values: dict[str, Any], hermes_home: str) -> None:
        self._run(["provider", "save-config", "--hermes-home", hermes_home, "--values-json", json.dumps(values, ensure_ascii=False)], timeout=10)

    def initialize(self, session_id: str, **kwargs: Any) -> None:
        self._state = {
            "session_id": session_id,
            "hermes_home": str(kwargs.get("hermes_home") or Path.home() / ".hermes"),
            "platform": str(kwargs.get("platform") or "cli"),
            "user_id": str(kwargs.get("user_id") or ""),
            "user_name": str(kwargs.get("user_name") or ""),
            "agent_identity": str(kwargs.get("agent_identity") or "default"),
            "agent_context": str(kwargs.get("agent_context") or ""),
        }

    def system_prompt_block(self) -> str:
        return self._run(["provider", "system-prompt", "--state-json", self._state_json()], timeout=10, default="")

    def prefetch(self, query: str, *, session_id: str = "") -> str:
        args = ["provider", "prefetch", "--state-json", self._state_json(), "--query", query]
        if session_id:
            args.extend(["--session-id-override", session_id])
        return self._run(args, timeout=15, default="")

    def queue_prefetch(self, query: str, *, session_id: str = "") -> None:
        return None

    def get_tool_schemas(self) -> list[dict[str, Any]]:
        raw = self._run(["provider", "tool-schemas"], timeout=5, default="[]")
        try:
            parsed = json.loads(raw)
            return parsed if isinstance(parsed, list) else []
        except Exception:
            return []

    def handle_tool_call(self, tool_name: str, args: dict[str, Any], **kwargs: Any) -> str:
        raw_args = json.dumps(args or {}, ensure_ascii=False)
        try:
            return self._run(["provider", "tool-call", "--state-json", self._state_json(), "--tool-name", tool_name, "--args-json", raw_args], timeout=70)
        except Exception as exc:
            return json.dumps({"error": f"EverOS Rust provider failed: {exc}"}, ensure_ascii=False)

    def sync_turn(self, user_content: str, assistant_content: str, *, session_id: str = "") -> None:
        args = [
            "provider",
            "sync-turn",
            "--state-json",
            self._state_json(),
            "--user-content",
            user_content,
            "--assistant-content",
            assistant_content,
        ]
        if session_id:
            args.extend(["--session-id-override", session_id])
        self._spawn(args)

    def on_memory_write(self, action: str, target: str, content: str, metadata: dict[str, Any] | None = None) -> None:
        self._spawn([
            "provider",
            "on-memory-write",
            "--state-json",
            self._state_json(),
            "--action",
            action,
            "--target",
            target,
            "--content",
            content,
            "--metadata-json",
            json.dumps(metadata, ensure_ascii=False) if metadata is not None else "null",
        ])

    def on_session_end(self, messages: list[dict[str, Any]]) -> None:
        self._run([
            "provider",
            "on-session-end",
            "--state-json",
            self._state_json(),
            "--messages-json",
            json.dumps(messages or [], ensure_ascii=False, separators=(",", ":")),
        ], timeout=20, default="")

    def on_pre_compress(self, messages: list[dict[str, Any]]) -> str:
        return self._run([
            "provider",
            "on-pre-compress",
            "--state-json",
            self._state_json(),
            "--messages-json",
            json.dumps(messages or [], ensure_ascii=False, separators=(",", ":")),
        ], timeout=20, default="")

    def on_delegation(self, task: str, result: str, *, child_session_id: str = "", **kwargs: Any) -> None:
        args = [
            "provider",
            "on-delegation",
            "--state-json",
            self._state_json(),
            "--task",
            task,
            "--result",
            result,
        ]
        if child_session_id:
            args.extend(["--child-session-id", child_session_id])
        self._spawn(args)

    def on_session_switch(self, new_session_id: str, *, parent_session_id: str = "", reset: bool = False, **kwargs: Any) -> None:
        if new_session_id:
            self._state["session_id"] = new_session_id

    def shutdown(self) -> None:
        children = list(self._children)
        self._children = []
        for child in children:
            if child.poll() is None:
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    child.kill()

    def _state_json(self) -> str:
        return json.dumps(self._state, ensure_ascii=False, separators=(",", ":"))

    def _run(self, args: list[str], *, timeout: float, default: str | None = None) -> str:
        binary = self._binary_path()
        if not binary:
            if default is not None:
                return default
            raise RuntimeError("everos-hermes-rust binary not found; set EVEROS_HERMES_RUST_BIN")
        proc = subprocess.run([binary, *args], text=True, capture_output=True, timeout=timeout, check=False)
        if proc.returncode != 0:
            if default is not None:
                return default
            raise RuntimeError((proc.stderr or proc.stdout or f"exit {proc.returncode}").strip())
        return proc.stdout.strip()

    def _spawn(self, args: list[str]) -> None:
        binary = self._binary_path()
        if not binary:
            return None
        child = subprocess.Popen([binary, *args], text=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self._children = [proc for proc in self._children if proc.poll() is None]
        self._children.append(child)
        return None

    def _binary_path(self) -> str:
        env_path = os.environ.get("EVEROS_HERMES_RUST_BIN", "").strip()
        if env_path and Path(env_path).is_file():
            return env_path
        here = Path(__file__).resolve()
        candidates = []
        try:
            root = here.parents[2]
            candidates.extend([
                root / "target" / "release" / "everos-hermes-rust",
                root / "target" / "debug" / "everos-hermes-rust",
            ])
        except IndexError:
            pass
        candidates.extend([
            Path.cwd() / "target" / "release" / "everos-hermes-rust",
            Path.cwd() / "target" / "debug" / "everos-hermes-rust",
        ])
        for candidate in candidates:
            if candidate.is_file():
                return str(candidate)
        return shutil.which("everos-hermes-rust") or ""


TOOLSET = "everos"
SKILL_NAME = "everos-memory-curation"
SKILL_DESCRIPTION = "Operate and curate EverOS-Hermes memory safely."
_REQUIRED_ENV = ["EVEROS_API_KEY"]
_PLUGIN_TOOL_SESSION_ID = "everos-rust-plugin-tools"

_tool_provider: EverOSRustMemoryProvider | None = None
_tool_provider_lock = threading.Lock()


def _skill_path() -> Path:
    return Path(__file__).resolve().parent / "resources" / "skills" / SKILL_NAME / "SKILL.md"


def _provider_available() -> bool:
    return EverOSRustMemoryProvider().is_available()


def _get_tool_provider() -> EverOSRustMemoryProvider:
    global _tool_provider
    with _tool_provider_lock:
        if _tool_provider is None:
            _tool_provider = EverOSRustMemoryProvider()
            _tool_provider.initialize(_PLUGIN_TOOL_SESSION_ID)
        return _tool_provider


def _make_tool_handler(tool_name: str) -> Callable[..., str]:
    def _handler(args: dict[str, Any] | None = None, **kwargs: Any) -> str:
        provider = _get_tool_provider()
        return provider.handle_tool_call(tool_name, args or {}, **kwargs)

    return _handler


def _register_standalone_tools(ctx: Any) -> None:
    if not hasattr(ctx, "register_tool"):
        return
    provider = EverOSRustMemoryProvider()
    for schema in provider.get_tool_schemas():
        name = str(schema.get("name") or "").strip()
        if not name:
            continue
        ctx.register_tool(
            name=name,
            toolset=TOOLSET,
            schema=schema,
            handler=_make_tool_handler(name),
            check_fn=_provider_available,
            requires_env=_REQUIRED_ENV,
            description=str(schema.get("description") or ""),
            emoji="🧠",
        )


def _register_bundled_skill(ctx: Any) -> None:
    if not hasattr(ctx, "register_skill"):
        return
    path = _skill_path()
    if path.exists():
        ctx.register_skill(SKILL_NAME, path, SKILL_DESCRIPTION)


def register(ctx: Any) -> None:
    if hasattr(ctx, "register_memory_provider"):
        ctx.register_memory_provider(EverOSRustMemoryProvider())
        return

    _register_standalone_tools(ctx)
    _register_bundled_skill(ctx)
