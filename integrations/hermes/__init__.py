"""Hermes memory-provider plugin entrypoint for EverOS.

Install pattern (same shape as agentmemory's Hermes integration):

1. Install the package so this thin plugin can import everos_hermes:
   pip install -e /home/xu/project/tools/EverOS-Hermes
2. Copy this folder to ~/.hermes/plugins/everos
3. Set memory.provider: everos and EVEROS_API_KEY.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

_repo_src = Path(__file__).resolve().parents[2] / "src"
if _repo_src.is_dir() and str(_repo_src) not in sys.path:
    sys.path.insert(0, str(_repo_src))

try:
    from everos_hermes.provider import EverOSMemoryProvider
except Exception as exc:  # pragma: no cover - only hit in mis-installed Hermes plugin copies
    raise RuntimeError(
        "EverOS Hermes plugin requires the everos-hermes package. "
        "Run: pip install -e /home/xu/project/tools/EverOS-Hermes"
    ) from exc


def register(ctx: Any) -> None:
    ctx.register_memory_provider(EverOSMemoryProvider())
