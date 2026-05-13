#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
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

cargo build --release --bin everos-hermes-rust

rm -rf "$STAGE"
mkdir -p "$STAGE/bin" "$STAGE/integrations"
install -m 0755 "$ROOT/target/release/everos-hermes-rust" "$STAGE/bin/everos-hermes-rust"
cp -R "$ROOT/integrations/hermes" "$STAGE/integrations/hermes"
cp "$ROOT/README.md" "$STAGE/README.md"

cat > "$STAGE/INSTALL.md" <<EOF
# EverOS-Hermes Rust prebuilt package

Version: $VERSION
Target: $TARGET

## Contents

- bin/everos-hermes-rust — Rust provider helper and stdio compatibility binary
- integrations/hermes — single Hermes plugin directory with provider shim, tools, and bundled skill
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

Load the bundled runbook with \`/skill everos:everos-memory-curation\`. Restart Hermes CLI/WebUI/gateway after changing plugin, provider, or secret configuration.
EOF

tar -C "$DIST_DIR" -czf "$ARCHIVE" "$PKG_NAME"
(
  cd "$DIST_DIR"
  sha256sum "$(basename "$ARCHIVE")" > "$(basename "$SHA_FILE")"
)

printf '%s\n' "$ARCHIVE"
printf '%s\n' "$SHA_FILE"
