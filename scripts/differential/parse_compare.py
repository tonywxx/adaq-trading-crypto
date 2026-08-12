#!/usr/bin/env python3
"""用真实 ccxt 解析【同一份】原始 fixture,生成差分比对基准。

流程:raw fixture(record_raw.py 录制)→ ccxt 的 parse_* → 存为
tests/fixtures/ccxt_parsed/<exchange>/<method>.json;Rust 侧用我们的
解析器解析同一份 raw,再与这里的输出逐字段比对(ADR-0010 差分值比对)。

用法:
    .venv/bin/python scripts/differential/parse_compare.py
"""

import json
import os
import re
import sys


def snake(s: str) -> str:
    s = re.sub(r"(?<=[a-z0-9])(?=[A-Z])", "_", s)
    return s.lower()


def normalize_key(v):
    if isinstance(v, dict):
        return {snake(k): normalize_key(x) for k, x in v.items()}
    if isinstance(v, list):
        return [normalize_key(x) for x in v]
    return v


def main() -> int:
    import ccxt

    ex = ccxt.binance({"enableRateLimit": True})
    ex.load_markets()
    market = ex.market("BTC/USDT")

    base = os.path.join("tests", "fixtures", "raw", "binance")
    out = os.path.join("tests", "fixtures", "ccxt_parsed", "binance")
    os.makedirs(out, exist_ok=True)

    cases = {
        "ticker": lambda raw: ex.parse_ticker(raw),
        "ohlcv": lambda raw: [ex.parse_ohlcv(row) for row in raw],
        "order_book": lambda raw: ex.parse_order_book(raw, "BTC/USDT"),
        "trades": lambda raw: [ex.parse_trade(t, market) for t in raw],
    }
    for name, fn in cases.items():
        with open(os.path.join(base, f"{name}.json"), encoding="utf-8") as f:
            raw = json.load(f)["raw"]
        try:
            parsed = fn(raw)
        except Exception as e:  # noqa: BLE001
            print(f"error {name}: {e}", file=sys.stderr)
            return 1
        dest = os.path.join(out, f"{name}.json")
        with open(dest, "w", encoding="utf-8") as f:
            json.dump(
                {"method": f"fetch_{name}", "symbol": "BTC/USDT", "parsed": normalize_key(parsed)},
                f,
                indent=2,
                ensure_ascii=False,
            )
        print(f"wrote {dest}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
