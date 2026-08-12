#!/usr/bin/env python3
"""录制真实 ccxt 统一输出到 tests/fixtures/recorded/<exchange>/<method>.json。

仅用公共端点(无需 API key),对齐 ADR-0010「录制 fixtures 为主」的差分基建:
- `normalized`:ccxt 统一结构的输出;key 递归转 snake_case,与 Rust 端统一
  结构字段名对齐,便于直接反序列化。
- 回放:离线 JSON 供契约/形状测试使用;CI 定期 live 差分另行脚本。

用法:
    python3 scripts/differential/record.py --exchange binance --symbol BTC/USDT
"""

import argparse
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
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--exchange", default="binance")
    ap.add_argument("--symbol", default="BTC/USDT")
    ap.add_argument(
        "--methods", default="ticker,ohlcv,order_book,trades,markets"
    )
    args = ap.parse_args()

    import ccxt  # 延迟导入:setup.sh 负责安装

    exchange_cls = getattr(ccxt, args.exchange, None)
    if exchange_cls is None:
        print(f"error: ccxt has no exchange {args.exchange}", file=sys.stderr)
        return 2
    ex = exchange_cls({"enableRateLimit": True})

    out_dir = os.path.join("tests", "fixtures", "recorded", args.exchange)
    os.makedirs(out_dir, exist_ok=True)

    for m in args.methods.split(","):
        fn = getattr(ex, f"fetch_{m}", None)
        if fn is None:
            print(f"skip: fetch_{m} not supported by {args.exchange}")
            continue
        if m == "markets":
            all_markets = fn()
            wanted = [x for x in all_markets if x["symbol"] == args.symbol][:1]
            others = [x for x in all_markets if x["symbol"] != args.symbol][:9]
            data = wanted + others  # 保证目标 symbol 在 fixtures 中
        elif m == "ohlcv":
            data = fn(args.symbol, "1m", limit=5)
        elif m == "order_book":
            data = fn(args.symbol, limit=10)
        elif m == "trades":
            data = fn(args.symbol, limit=5)
        else:
            data = fn(args.symbol)

        payload = {
            "exchange": args.exchange,
            "symbol": args.symbol,
            "method": f"fetch_{m}",
            "normalized": normalize_key(data),
        }
        path = os.path.join(out_dir, f"{m}.json")
        with open(path, "w", encoding="utf-8") as f:
            json.dump(payload, f, indent=2, ensure_ascii=False)
        print(f"wrote {path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
