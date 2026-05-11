from __future__ import annotations

import os
import re
from pathlib import Path
from typing import Mapping

_ENV_KEY_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")


def hermes_home(explicit: str | os.PathLike[str] | None = None) -> Path:
    """Return the Hermes home used for secret lookup.

    Priority follows Hermes conventions: an explicit path from the provider,
    then HERMES_HOME for profiles/tests, then ~/.hermes.
    """
    if explicit:
        return Path(explicit).expanduser()
    return Path(os.environ.get("HERMES_HOME") or (Path.home() / ".hermes")).expanduser()


def hermes_dotenv_path(explicit_home: str | os.PathLike[str] | None = None) -> Path:
    return hermes_home(explicit_home) / ".env"


def read_dotenv(path: str | os.PathLike[str]) -> dict[str, str]:
    """Parse a small dotenv file without adding a python-dotenv dependency.

    Supported syntax is intentionally conservative: KEY=value, optional
    leading ``export``, blank lines/comments, and single/double quoted values.
    Invalid lines are ignored rather than raising, because missing secrets are
    reported by the client/provider availability checks.
    """
    dotenv = Path(path).expanduser()
    if not dotenv.exists() or not dotenv.is_file():
        return {}

    values: dict[str, str] = {}
    try:
        lines = dotenv.read_text(encoding="utf-8").splitlines()
    except OSError:
        return {}

    for raw_line in lines:
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("export "):
            line = line[len("export ") :].lstrip()
        if "=" not in line:
            continue
        key, value = line.split("=", 1)
        key = key.strip()
        if not _ENV_KEY_RE.match(key):
            continue
        values[key] = _parse_dotenv_value(value)
    return values


def hermes_dotenv_values(explicit_home: str | os.PathLike[str] | None = None) -> dict[str, str]:
    return read_dotenv(hermes_dotenv_path(explicit_home))


def get_env(name: str, default: str = "", *, hermes_home_path: str | os.PathLike[str] | None = None, dotenv: Mapping[str, str] | None = None) -> str:
    """Return an env var, falling back to $HERMES_HOME/.env or ~/.hermes/.env.

    Real process environment wins so users can temporarily override a value
    without editing the dotenv file. This function never prints or persists
    secret values.
    """
    current = os.environ.get(name)
    if current is not None and current.strip():
        return current.strip()
    values = dict(dotenv) if dotenv is not None else hermes_dotenv_values(hermes_home_path)
    value = values.get(name)
    if value is not None and value.strip():
        return value.strip()
    return str(default).strip()


def _parse_dotenv_value(raw: str) -> str:
    value = raw.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in {"'", '"'}:
        value = value[1:-1]
        if raw.strip().startswith('"'):
            value = value.encode("utf-8").decode("unicode_escape")
        return value

    # Strip inline comments only when introduced after whitespace, preserving
    # values that legitimately contain '#'.
    for marker in (" #", "\t#"):
        idx = value.find(marker)
        if idx != -1:
            value = value[:idx].rstrip()
            break
    return value
