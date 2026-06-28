#!/usr/bin/env bash

set -euo pipefail

fail() {
  echo "Package failed: $*" >&2
  exit 1
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

EPOCH="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct 2>/dev/null || echo 0)}"
BUILD_DATE="@$EPOCH"
PUBLISHED_AT="$(TZ=UTC git log -1 --date=format-local:'%Y-%m-%dT%H:%M:%SZ' --format=%cd 2>/dev/null || echo "1970-01-01T00:00:00Z")"
TARGET="x86_64-unknown-linux-musl"
RUST_TARGET="${SECLAB_RUST_TARGET:-$TARGET}"
TARGET_TRIPLE="${SECLAB_TARGET_TRIPLE:-linux-x86_64}"
BUILD_PROFILE="${SECLAB_BUILD_PROFILE:-release}"
BIN_DIR="$ROOT_DIR/target/$RUST_TARGET/$BUILD_PROFILE"
OUT_FILE="$ROOT_DIR/target/seclab-bundle.tar.gz"

[[ -x "$BIN_DIR/seclab" ]] || fail "missing binary: $BIN_DIR/seclab"
[[ -x "$BIN_DIR/seclab-agent" ]] || fail "missing binary: $BIN_DIR/seclab-agent"

SECLAB_VERSION=""
SECLAB_TOML="$ROOT_DIR/Cargo.toml"
if [[ -f "$SECLAB_TOML" ]]; then
  SECLAB_VERSION="$(grep -E '^version[[:space:]]*=' "$SECLAB_TOML" | head -n 1 | awk -F '\"' '{print $2}')"
fi
if [[ -z "$SECLAB_VERSION" ]]; then
  fail "failed to read seclab version"
fi

BUNDLE_DIR_NAME="seclab-$SECLAB_VERSION"
[[ -x "$ROOT_DIR/deploy/install.sh" ]] || fail "missing deploy/install.sh"

STAGING_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$STAGING_DIR"
}
trap cleanup EXIT

mkdir -p "$STAGING_DIR/$BUNDLE_DIR_NAME/templates"

cp "$ROOT_DIR/deploy/install.sh" "$STAGING_DIR/$BUNDLE_DIR_NAME/"

if [[ -x "$BIN_DIR/slctl" ]]; then
  cp "$BIN_DIR/slctl" "$STAGING_DIR/$BUNDLE_DIR_NAME/slctl"
elif [[ -x "$ROOT_DIR/deploy/slctl" ]]; then
  cp "$ROOT_DIR/deploy/slctl" "$STAGING_DIR/$BUNDLE_DIR_NAME/"
fi

TEMPLATE_SRC="$ROOT_DIR/deploy/templates"
[[ -d "$TEMPLATE_SRC" ]] || fail "missing templates dir: $TEMPLATE_SRC"
cp -R "$TEMPLATE_SRC/." "$STAGING_DIR/$BUNDLE_DIR_NAME/templates/"

mkdir -p "$(dirname "$OUT_FILE")"

sign_file() {
  local file="$1"
  local sig="$file.sig"

  local key_source="${SECLAB_SIGNING_PRIVATE_KEY:-}"
  if [[ -z "$key_source" && "$BUILD_PROFILE" == "fast-release" ]]; then
    echo "Skipping signature for $file in fast-release mode (no key provided)"
    return 0
  fi

  command -v minisign &>/dev/null || fail "minisign is required for signing, but it is not installed."
  [[ -n "$key_source" ]] || fail "SECLAB_SIGNING_PRIVATE_KEY is required"

  # Expand leading tilde ~ to $HOME
  local resolved_source="${key_source/#\~/$HOME}"
  local key_file=""
  local temp_key=""

  if [[ -f "$resolved_source" ]]; then
    key_file="$resolved_source"
  elif [[ "$key_source" =~ ^untrusted[[:space:]]+comment:[[:space:]]+minisign[[:space:]]+(encrypted[[:space:]]+)?secret[[:space:]]+key ]]; then
    temp_key="$(mktemp)"
    printf '%s\n' "$key_source" > "$temp_key"
    key_file="$temp_key"
  else
    fail "SECLAB_SIGNING_PRIVATE_KEY is neither a valid file path nor a valid private key content."
  fi

  local err_msg
  if ! err_msg=$(printf '%s\n' "${SECLAB_SIGNING_PRIVATE_KEY_PASSWORD:-}" | \
    minisign -S -s "$key_file" -m "$file" -x "$sig" -q 2>&1); then
    fail "minisign signing failed: $err_msg"
  fi

  if [[ -n "$temp_key" ]]; then
    rm -f "$temp_key"
  fi
}

sha_file() {
  local file="$1"
  local name
  name="$(basename "$file")"
  sha256sum "$file" | awk -v n="$name" '{print $1 "  " n}' > "$file.sha256"
}

COMPONENT_DIR="$STAGING_DIR/$BUNDLE_DIR_NAME/components"
mkdir -p "$COMPONENT_DIR/controller" "$COMPONENT_DIR/agent"
cp "$BIN_DIR/seclab" "$COMPONENT_DIR/controller/seclab"
cp "$BIN_DIR/seclab-agent" "$COMPONENT_DIR/agent/seclab-agent"

CONTROLLER_PACKAGE="$STAGING_DIR/$BUNDLE_DIR_NAME/seclab-${TARGET_TRIPLE}.tar.gz"
AGENT_PACKAGE="$STAGING_DIR/$BUNDLE_DIR_NAME/seclab-agent-${TARGET_TRIPLE}.tar.gz"
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="$BUILD_DATE" \
  -C "$COMPONENT_DIR/controller" -czf "$CONTROLLER_PACKAGE" seclab
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="$BUILD_DATE" \
  -C "$COMPONENT_DIR/agent" -czf "$AGENT_PACKAGE" seclab-agent
sha_file "$CONTROLLER_PACKAGE"
sha_file "$AGENT_PACKAGE"
sign_file "$CONTROLLER_PACKAGE"
sign_file "$AGENT_PACKAGE"
rm -rf "$COMPONENT_DIR"

# Auto derive release channel
CHANNEL="stable"
if [[ "$SECLAB_VERSION" == *-* ]]; then
  CHANNEL="prerelease"
fi
RELEASE_CHANNEL="${SECLAB_RELEASE_CHANNEL:-$CHANNEL}"

cat <<EOF > "$STAGING_DIR/$BUNDLE_DIR_NAME/release.json"
{
  "version": "${SECLAB_VERSION}",
  "channel": "${RELEASE_CHANNEL}",
  "targetTriple": "${TARGET_TRIPLE}",
  "publishedAt": "${PUBLISHED_AT}"
}
EOF

OUT_FILE="$(dirname "$OUT_FILE")/seclab-${SECLAB_VERSION}-${TARGET_TRIPLE}.tar.gz"
tar \
  --sort=name \
  --owner=0 \
  --group=0 \
  --numeric-owner \
  --mtime="$BUILD_DATE" \
  -C "$STAGING_DIR" \
  -czf "$OUT_FILE" \
  "$BUNDLE_DIR_NAME"

echo "package done: $OUT_FILE"
