"""EverOS integration for Hermes Agent.

This package provides:
- a small EverOS Cloud v1 REST client,
- a stdio MCP compatibility server exposing EverOS memory tools,
- a Hermes MemoryProvider implementation,
- support code used by the single Hermes plugin under integrations/hermes.
"""

from .client import DEFAULT_BASE_URL, EverOSClient, EverOSError
from .provider import EverOSMemoryProvider

__all__ = ["DEFAULT_BASE_URL", "EverOSClient", "EverOSError", "EverOSMemoryProvider"]
