"""EverOS integration for Hermes Agent.

This package provides:
- a small EverOS v1 REST client,
- a stdio MCP server exposing EverOS memory tools,
- a Hermes MemoryProvider implementation.
"""

from .client import DEFAULT_BASE_URL, EverOSClient, EverOSError
from .provider import EverOSMemoryProvider

__all__ = ["DEFAULT_BASE_URL", "EverOSClient", "EverOSError", "EverOSMemoryProvider"]
