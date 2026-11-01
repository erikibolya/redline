#!/usr/bin/env bash
# Build the store ZIP using only Python's standard library.
set -euo pipefail
cd "$(dirname "$0")/.."
python3 scripts/package.py
