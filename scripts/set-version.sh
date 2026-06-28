#!/usr/bin/env bash
set -e

# 获取脚本所在的目录，从而指向同级目录的 set-version.mjs
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
node "$SCRIPT_DIR/set-version.mjs" "$@"
