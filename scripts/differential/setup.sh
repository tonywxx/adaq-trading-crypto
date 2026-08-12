#!/usr/bin/env bash
# 差分测试录制环境:python venv + ccxt(ADR-0010)。
set -euo pipefail
cd "$(dirname "$0")/../.."

PY=${PYTHON:-python3}
if [ ! -d .venv ]; then
  "$PY" -m venv .venv
fi
.venv/bin/pip install --quiet --upgrade pip
.venv/bin/pip install --quiet ccxt
echo "ok: .venv with ccxt ready"
