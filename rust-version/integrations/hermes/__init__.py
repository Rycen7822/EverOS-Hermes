"""Hermes memory-provider plugin shim for the Rust EverOS-Hermes runtime.

The Hermes plugin API is Python, so this file stays intentionally tiny: it
registers a MemoryProvider class and delegates all EverOS behavior to the Rust
binary via `everos-hermes-rust provider ...` commands.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import threading
from datetime import datetime, timezone
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


_MAX_REDACTED_TEXT = 500
_DIAGNOSTIC_FIELD_RE = re.compile(
    r"\s+(?:request[_-]?id|trace[_-]?id|span[_-]?id|status|code)=[^\s,;\}\]]+",
    re.IGNORECASE,
)
_SECRET_PATTERNS = [
    re.compile(r"(?i)Authorization\s*:\s*Bearer\s+[^\s,;\]}]+"),
    re.compile(r"(?i)\bBearer\s+[A-Za-z0-9._~+/=\-]+"),
    re.compile(r"\bsk-[A-Za-z0-9._-]{4,}\b"),
    re.compile(
        r'''(?i)(?P<prefix>["']?(?:api[_-]?key|token|access[_-]?token|refresh[_-]?token|password|passwd|secret|authorization|credentials?|private[_-]?key)["']?\s*[:=]\s*")(?P<value>(?:\\.|[^"])*)(?P<suffix>")'''
    ),
    re.compile(
        r'''(?i)(?P<prefix>["']?(?:api[_-]?key|token|access[_-]?token|refresh[_-]?token|password|passwd|secret|authorization|credentials?|private[_-]?key)["']?\s*[:=]\s*')(?P<value>(?:\\.|[^'])*)(?P<suffix>')'''
    ),
    re.compile(
        r'''(?i)(?P<prefix>["']?(?:api[_-]?key|token|access[_-]?token|refresh[_-]?token|password|passwd|secret|authorization|credentials?|private[_-]?key)["']?\s*[:=]\s*)(?P<value>[^\r\n]+)'''
    ),
]

_SENSITIVE_KEY_RE = re.compile(
    r"(?i)(^|[_\-\s.])(api[_-]?key|token|access[_-]?token|refresh[_-]?token|password|passwd|secret|authorization|credentials?|private[_-]?key)([_\-\s.]|$)"
)
_SENSITIVE_ASSIGN_RE = re.compile(
    r"(?i)(?P<prefix>[\"']?(?:api[_-]?key|token|access[_-]?token|refresh[_-]?token|password|passwd|secret|authorization|credentials?|private[_-]?key)[\"']?\s*[:=]\s*)"
)
_EMBEDDED_JSON_STRING_PATTERNS = [
    re.compile(r"(?P<prefix>[\"']?[A-Za-z_][\w.\-]*[\"']?\s*[:=]\s*\")(?P<value>(?:\\.|[^\"])*)\""),
    re.compile(r"(?P<prefix>[\"']?[A-Za-z_][\w.\-]*[\"']?\s*[:=]\s*')(?P<value>(?:\\.|[^'])*)'"),
]


def _shim_is_sensitive_key(key: Any) -> bool:
    return bool(_SENSITIVE_KEY_RE.search(str(key)))


def _shim_scrub_json_value(value: Any) -> Any:
    if isinstance(value, dict):
        return {
            str(key): "[REDACTED]" if _shim_is_sensitive_key(key) else _shim_scrub_json_value(val)
            for key, val in value.items()
        }
    if isinstance(value, list):
        return [_shim_scrub_json_value(item) for item in value]
    if isinstance(value, str):
        return _redact_text(value)
    return value


def _shim_scrub_json_text(value: str) -> str | None:
    stripped = (value or "").strip()
    if not stripped.startswith(("{", "[")):
        return None
    try:
        parsed = json.loads(stripped)
    except Exception:
        return None
    return json.dumps(_shim_scrub_json_value(parsed), ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def _decode_escaped_json_value(value: str) -> str | None:
    try:
        decoded = json.loads(f'"{value}"')
    except Exception:
        return None
    return decoded if isinstance(decoded, str) else None


def _escape_string_value(value: str) -> str:
    encoded = json.dumps(value, ensure_ascii=False)
    return encoded[1:-1]


def _redact_embedded_json_strings(text: str) -> str:
    redacted = text
    for pattern in _EMBEDDED_JSON_STRING_PATTERNS:
        def replace(match: re.Match[str]) -> str:
            decoded = _decode_escaped_json_value(match.group("value"))
            if decoded is None:
                return match.group(0)
            scrubbed = _shim_scrub_json_text(decoded)
            if scrubbed is None:
                return match.group(0)
            suffix = '"' if match.group(0).endswith('"') else "'"
            return f"{match.group('prefix')}{_escape_string_value(scrubbed)}{suffix}"

        redacted = pattern.sub(replace, redacted)
    return redacted


def _find_balanced_end(text: str, start: int) -> int | None:
    pairs = {"{": "}", "[": "]"}
    opener = text[start : start + 1]
    closer = pairs.get(opener)
    if closer is None:
        return None
    depth = 0
    quote: str | None = None
    escape = False
    for pos in range(start, len(text)):
        char = text[pos]
        if quote is not None:
            if escape:
                escape = False
            elif char == "\\":
                escape = True
            elif char == quote:
                quote = None
            continue
        if char in {"'", '"'}:
            quote = char
        elif char == opener:
            depth += 1
        elif char == closer:
            depth -= 1
            if depth == 0:
                return pos + 1
    return None


def _redact_sensitive_jsonish_assignments(text: str) -> str:
    out: list[str] = []
    cursor = 0
    for match in _SENSITIVE_ASSIGN_RE.finditer(text):
        if match.start() < cursor:
            continue
        value_start = match.end()
        while value_start < len(text) and text[value_start].isspace():
            value_start += 1
        if value_start >= len(text) or text[value_start] not in {"{", "["}:
            continue
        value_end = _find_balanced_end(text, value_start)
        if value_end is None:
            continue
        out.append(text[cursor : match.start()])
        out.append(match.group("prefix"))
        out.append("[REDACTED]")
        cursor = value_end
    if not out:
        return text
    out.append(text[cursor:])
    return "".join(out)



def _redact_text(text: str) -> str:
    redacted = _redact_embedded_json_strings(text or "")
    redacted = _redact_sensitive_jsonish_assignments(redacted)
    for pattern in _SECRET_PATTERNS[:3]:
        redacted = pattern.sub("[REDACTED]", redacted)

    def replace_secret(match: re.Match[str]) -> str:
        suffix = match.groupdict().get("suffix")
        if suffix is not None:
            return f"{match.group('prefix')}[REDACTED]{suffix}"
        value = match.groupdict().get("value") or ""
        diagnostic_suffix = "".join(match.group(0) for match in _DIAGNOSTIC_FIELD_RE.finditer(value))
        return f"{match.group('prefix')}[REDACTED]{diagnostic_suffix}"

    for pattern in _SECRET_PATTERNS[3:]:
        redacted = pattern.sub(replace_secret, redacted)
    if len(redacted) > _MAX_REDACTED_TEXT:
        return redacted[:_MAX_REDACTED_TEXT] + "...[truncated]"
    return redacted


def _payload_text(payload: dict[str, Any] | None) -> str | None:
    if payload is None:
        return None
    return json.dumps(payload, ensure_ascii=False, separators=(",", ":"))


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
        self._monitors: list[threading.Thread] = []

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
        self._run(
            ["provider", "save-config", "--hermes-home", hermes_home, "--payload-stdin"],
            timeout=10,
            payload={"values": values},
        )

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
        return self._run(
            ["provider", "system-prompt", "--payload-stdin"],
            timeout=10,
            default="",
            payload={"state": self._state},
        )

    def prefetch(self, query: str, *, session_id: str = "") -> str:
        payload = {"state": self._state, "query": query, "session_id_override": session_id}
        return self._run(["provider", "prefetch", "--payload-stdin"], timeout=15, default="", payload=payload)

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
        payload = {"state": self._state, "args": args or {}}
        try:
            return self._run(
                ["provider", "tool-call", "--tool-name", tool_name, "--payload-stdin"],
                timeout=70,
                payload=payload,
            )
        except Exception as exc:
            return json.dumps({"error": _redact_text(f"EverOS Rust provider failed: {exc}")}, ensure_ascii=False)

    def sync_turn(self, user_content: str, assistant_content: str, *, session_id: str = "") -> None:
        self._spawn(
            ["provider", "sync-turn", "--payload-stdin"],
            payload={
                "state": self._state,
                "user_content": user_content,
                "assistant_content": assistant_content,
                "session_id_override": session_id,
            },
        )

    def on_memory_write(self, action: str, target: str, content: str, metadata: dict[str, Any] | None = None) -> None:
        self._spawn(
            ["provider", "on-memory-write", "--payload-stdin"],
            payload={
                "state": self._state,
                "action": action,
                "target": target,
                "content": content,
                "metadata": metadata,
            },
        )

    def on_session_end(self, messages: list[dict[str, Any]]) -> None:
        self._run(
            ["provider", "on-session-end", "--payload-stdin"],
            timeout=20,
            default="",
            payload={"state": self._state, "messages": messages or []},
        )

    def on_pre_compress(self, messages: list[dict[str, Any]]) -> str:
        return self._run(
            ["provider", "on-pre-compress", "--payload-stdin"],
            timeout=20,
            default="",
            payload={"state": self._state, "messages": messages or []},
        )

    def on_delegation(self, task: str, result: str, *, child_session_id: str = "", **kwargs: Any) -> None:
        self._spawn(
            ["provider", "on-delegation", "--payload-stdin"],
            payload={
                "state": self._state,
                "task": task,
                "result": result,
                "child_session_id": child_session_id,
            },
        )

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
        monitors = list(self._monitors)
        self._monitors = []
        for monitor in monitors:
            monitor.join(timeout=1)

    def _state_json(self) -> str:
        return json.dumps(self._state, ensure_ascii=False, separators=(",", ":"))

    def _run(
        self,
        args: list[str],
        *,
        timeout: float,
        default: str | None = None,
        payload: dict[str, Any] | None = None,
    ) -> str:
        binary = self._binary_path()
        if not binary:
            if default is not None:
                return default
            raise RuntimeError("everos-hermes-rust binary not found; set EVEROS_HERMES_RUST_BIN")
        proc = subprocess.run(
            [binary, *args],
            input=_payload_text(payload),
            text=True,
            capture_output=True,
            timeout=timeout,
            check=False,
        )
        if proc.returncode != 0:
            if default is not None:
                return default
            raise RuntimeError(_redact_text((proc.stderr or proc.stdout or f"exit {proc.returncode}").strip()))
        return proc.stdout.strip()

    def _spawn(self, args: list[str], *, payload: dict[str, Any] | None = None) -> None:
        binary = self._binary_path()
        if not binary:
            return None
        try:
            child = subprocess.Popen(
                [binary, *args],
                text=True,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
        except Exception as exc:
            self._record_background_failure(args, 127, str(exc))
            return None
        self._children = [proc for proc in self._children if proc.poll() is None]
        self._children.append(child)
        thread = threading.Thread(
            target=self._monitor_child,
            args=(child, args, _payload_text(payload)),
            daemon=True,
        )
        self._monitors.append(thread)
        thread.start()
        return None

    def _monitor_child(self, child: subprocess.Popen[str], args: list[str], payload_text: str | None) -> None:
        try:
            stdout, stderr = child.communicate(input=payload_text, timeout=120)
        except subprocess.TimeoutExpired:
            child.kill()
            self._record_background_failure(args, -9, "background provider command timed out")
            return
        except Exception as exc:
            self._record_background_failure(args, 126, str(exc))
            return
        if child.returncode:
            self._record_background_failure(args, child.returncode, stderr or stdout or "")

    def _record_background_failure(self, args: list[str], returncode: int | None, detail: str) -> None:
        hermes_home = Path(str(self._state.get("hermes_home") or Path.home() / ".hermes"))
        try:
            hermes_home.mkdir(parents=True, exist_ok=True)
            log_path = hermes_home / "everos.log"
            timestamp = datetime.now(timezone.utc).isoformat(timespec="seconds")
            command = " ".join(args[:2]) if args else "provider"
            line = (
                f"{timestamp} background provider command failed "
                f"command={command!r} returncode={returncode}: {_redact_text(detail)}\n"
            )
            flags = os.O_WRONLY | os.O_CREAT | os.O_APPEND
            fd = os.open(log_path, flags, 0o600)
            try:
                os.fchmod(fd, 0o600)
                with os.fdopen(fd, "a", encoding="utf-8") as handle:
                    handle.write(line)
            except Exception:
                os.close(fd)
                raise
        except Exception:
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
SKILL_DESCRIPTION = "Use proactively when complex or iterative work may produce durable EverOS/Hermes memory: recall, save, verify, clean, compress, or migrate reusable workflows, debugging lessons, tool/API quirks, and agent cases without saving noisy task logs."
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
