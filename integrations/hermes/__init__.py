"""Single Hermes plugin entrypoint for EverOS-Hermes.

This directory is intentionally self-contained when copied to
``$HERMES_HOME/plugins/everos``:

- Hermes memory-provider discovery can load it when ``memory.provider: everos``
  is selected.
- Hermes standalone plugin discovery can load it when ``plugins.enabled``
  contains ``everos``; in that mode it registers explicit EverOS tools and a
  bundled read-only operator skill, so users no longer need a separate MCP
  server or a separately installed skill for ordinary Hermes use.
"""

from __future__ import annotations

import sys
import threading
from pathlib import Path
from typing import Any, Callable

_repo_src = Path(__file__).resolve().parents[2] / "src"
if _repo_src.is_dir() and str(_repo_src) not in sys.path:
    sys.path.insert(0, str(_repo_src))

try:
    from everos_hermes.provider import EverOSMemoryProvider
except Exception as exc:  # pragma: no cover - only hit in mis-installed Hermes plugin copies
    raise RuntimeError(
        "EverOS Hermes plugin requires the everos-hermes package. "
        "From the repository, run: python -m pip install -e ."
    ) from exc

TOOLSET = "everos"
SKILL_NAME = "everos-memory-curation"
SKILL_DESCRIPTION = "Use proactively when complex or iterative work may produce durable EverOS/Hermes memory: recall, save, verify, clean, compress, or migrate reusable workflows, debugging lessons, tool/API quirks, and agent cases without saving noisy task logs."
_PLUGIN_TOOL_SESSION_ID = "everos-plugin-tools"
_REQUIRED_ENV = ["EVEROS_API_KEY"]

_tool_provider: EverOSMemoryProvider | None = None
_tool_provider_lock = threading.Lock()


def _skill_path() -> Path:
    return Path(__file__).resolve().parent / "resources" / "skills" / SKILL_NAME / "SKILL.md"


def _provider_available() -> bool:
    return EverOSMemoryProvider().is_available()


def _get_tool_provider() -> EverOSMemoryProvider:
    """Return a lazily initialized provider for standalone plugin tools."""
    global _tool_provider
    with _tool_provider_lock:
        if _tool_provider is None:
            _tool_provider = EverOSMemoryProvider()
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
    schema_provider = EverOSMemoryProvider()
    for schema in schema_provider.get_tool_schemas():
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
    """Register EverOS against whichever Hermes plugin context is loading us.

    Hermes currently has two relevant plugin loaders:

    - ``plugins.memory`` passes a small collector that only supports
      ``register_memory_provider``.  That path must not try to register tools or
      plugin skills.
    - The standalone PluginManager passes ``PluginContext`` with
      ``register_tool`` / ``register_skill`` but no ``register_memory_provider``.
      That path exposes explicit EverOS tools and the bundled curation skill.
    """
    if hasattr(ctx, "register_memory_provider"):
        ctx.register_memory_provider(EverOSMemoryProvider())
        return

    _register_standalone_tools(ctx)
    _register_bundled_skill(ctx)
