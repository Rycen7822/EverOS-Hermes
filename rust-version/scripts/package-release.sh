#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "$ROOT/.." && pwd)"
cd "$ROOT"

VERSION="${VERSION:-$(python3 - <<'PY'
from pathlib import Path
import re
text = Path('Cargo.toml').read_text(encoding='utf-8')
match = re.search(r'^version\s*=\s*"([^"]+)"', text, re.M)
if not match:
    raise SystemExit('version not found in Cargo.toml')
print(match.group(1))
PY
)}"
TARGET="${TARGET:-$(rustc -vV | awk '/^host: / {print $2}') }"
TARGET="${TARGET// /}"
PKG_NAME="everos-hermes-rust-${VERSION}-${TARGET}"
DIST_DIR="$ROOT/dist"
STAGE="$DIST_DIR/$PKG_NAME"
ARCHIVE="$DIST_DIR/$PKG_NAME.tar.gz"
SHA_FILE="$ARCHIVE.sha256"
RUST_PLUGIN_PREFIX="rust-version/integrations/hermes"

copy_tracked_prefix() {
  # Stage release inputs via git ls-files so ignored/untracked files never enter the archive.
  local prefix="$1"
  local dest="$2"
  local copied=0
  while IFS= read -r -d '' rel; do
    local src="$REPO_ROOT/$rel"
    local relative="${rel#$prefix/}"
    mkdir -p "$dest/$(dirname "$relative")"
    install -m 0644 "$src" "$dest/$relative"
    copied=1
  done < <(git -C "$REPO_ROOT" ls-files -z -- "$prefix")
  if [[ "$copied" -ne 1 ]]; then
    printf 'No tracked files found for %s\n' "$prefix" >&2
    exit 1
  fi
}

check_no_untracked_sensitive_files() {
  local status rel base
  while IFS= read -r line; do
    status="${line:0:2}"
    rel="${line:3}"
    [[ "$status" == "??" || "$status" == "!!" ]] || continue
    base="$(basename "$rel")"
    case "$base:$rel" in
      .env:*|*.env:*|*.pem:*|*.key:*|*.p12:*|*.pfx:*|*secret*|*token*|*credential*)
        printf 'Refusing to package untracked or ignored sensitive file: %s\n' "$rel" >&2
        exit 1
        ;;
    esac
  done < <(git -C "$REPO_ROOT" status --porcelain --ignored --untracked-files=all -- "$RUST_PLUGIN_PREFIX")
}

check_no_untracked_sensitive_files
cargo build --release --target "$TARGET" --bin everos-hermes-rust

rm -rf "$STAGE"
mkdir -p "$STAGE/bin" "$STAGE/integrations"
install -m 0755 "$ROOT/target/$TARGET/release/everos-hermes-rust" "$STAGE/bin/everos-hermes-rust"
copy_tracked_prefix "$RUST_PLUGIN_PREFIX" "$STAGE/integrations/hermes"
find "$STAGE" \( -type d -name '__pycache__' -o -type f \( -name '*.pyc' -o -name '*.pyo' \) \) -prune -exec rm -rf {} +
install -m 0644 "$ROOT/README.md" "$STAGE/README.md"

cat > "$STAGE/INSTALL.md" <<EOF
# EverOS-Hermes Rust prebuilt package

Version: $VERSION
Target: $TARGET

## Contents

- bin/everos-hermes-rust — Rust provider helper and stdio compatibility binary
- integrations/hermes — single Hermes plugin directory with provider shim, tools, and thin bundled skill plus references
- README.md — Rust runtime documentation

## Quick install

\`\`\`bash
INSTALL_DIR="\$HOME/.local/share/everos-hermes"
HERMES_HOME="\${HERMES_HOME:-\$HOME/.hermes}"
mkdir -p "\$INSTALL_DIR" "\$HERMES_HOME/plugins"
cp -R . "\$INSTALL_DIR/"
rm -rf "\$HERMES_HOME/plugins/everos"
cp -R "\$INSTALL_DIR/integrations/hermes" "\$HERMES_HOME/plugins/everos"
"\$INSTALL_DIR/bin/everos-hermes-rust" --help
\`\`\`

Set secrets in \`\${HERMES_HOME:-\$HOME/.hermes}/.env\`, not in committed config. Use an absolute path for \`EVEROS_HERMES_RUST_BIN\`; this package's dotenv parser does not expand \`~\`, \`\$HOME\`, or \`\$INSTALL_DIR\` inside values.

\`\`\`bash
EVEROS_API_KEY=your_everos_api_key
EVEROS_USER_ID=hermes_default
EVEROS_HERMES_RUST_BIN=/home/you/.local/share/everos-hermes/bin/everos-hermes-rust
\`\`\`

Enable both plugin roles:

\`\`\`bash
hermes plugins enable everos
hermes config set memory.provider everos
\`\`\`

Load the bundled runbook with \`/skill everos:everos-memory-curation\`; its entry \`SKILL.md\` is a thin router that points to \`references/*.md\` for detailed guidance. Restart Hermes CLI/WebUI/gateway after changing plugin, provider, or secret configuration.
EOF

find "$STAGE" -type d -exec chmod 0755 {} +
find "$STAGE" -type f -exec chmod 0644 {} +
chmod 0755 "$STAGE/bin/everos-hermes-rust"

tar -C "$DIST_DIR" \
  --sort=name \
  --owner=0 \
  --group=0 \
  --numeric-owner \
  --mtime='@0' \
  -czf "$ARCHIVE" "$PKG_NAME"
(
  cd "$DIST_DIR"
  sha256sum "$(basename "$ARCHIVE")" > "$(basename "$SHA_FILE")"
)

printf '%s\n' "$ARCHIVE"
printf '%s\n' "$SHA_FILE"
