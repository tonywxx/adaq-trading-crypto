#!/usr/bin/env python3
"""htx(Huobi)差分录制:raw + ccxt 解析基准。

用真实 ccxt 解析【同一份 raw】生成基准:
    tests/fixtures/raw/htx/<method>.json          — 直接 HTTP 录原始响应
    tests/fixtures/ccxt_parsed/htx/<method>.json  — ccxt parse_* 解析结果

用法:
    .venv/bin/python scripts/differential/record_htx.py
"""

import json
import os
import re
import sys
import urllib.request

BASE = "https://api.huobi.pro"


def snake(s: str) -> str:
    s = re.sub(r"(?<=[a-z0-9])(?=[A-Z])", "_", s)
    s = s.replace("-", "_")
    return s.lower()


def normalize_key(v):
    if isinstance(v, dict):
        return {snake(k): normalize_key(x) for k, x in v.items()}
    if isinstance(v, list):
        return [normalize_key(x) for x in v]
    return v


def get_json(url: str):
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0", "Accept": "application/json"})
    with urllib.request.urlopen(req, timeout=20) as resp:
        return json.load(resp)


def write(dest: str, payload: dict) -> None:
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    with open(dest, "w", encoding="utf-8") as f:
        json.dump(payload, f, indent=2, ensure_ascii=False)
    print(f"wrote {dest}")


def main() -> int:
    import ccxt

    ex = ccxt.htx({"enableRateLimit": True})
    ex.load_markets()
    raw_dir = os.path.join("tests", "fixtures", "raw", "htx")
    parsed_dir = os.path.join("tests", "fixtures", "ccxt_parsed", "htx")

    symbols = get_json(f"{BASE}/v1/common/symbols")["data"][:10]
    write(os.path.join(raw_dir, "markets.json"), {"method": "fetch_markets", "raw": symbols})
    parsed = []
    for s in symbols:
        m = ex.markets_by_id[s["symbol"]][0]
        parsed.append({k: m.get(k) for k in ["id", "symbol", "base", "quote", "active", "precision"]})
    write(os.path.join(parsed_dir, "markets.json"), {"method": "fetch_markets", "parsed": normalize_key(parsed)})

    sym = "btcusdt"
    spot_market = ex.markets_by_id[sym][0]
    merged = get_json(f"{BASE}/market/detail/merged?symbol={sym}")
    tick = merged["tick"]
    write(os.path.join(raw_dir, "ticker.json"), {"method": "fetch_ticker", "raw": tick})
    parsed = ex.parse_ticker(tick, spot_market)
    write(os.path.join(parsed_dir, "ticker.json"), {"method": "fetch_ticker", "parsed": normalize_key(parsed)})

    depth = get_json(f"{BASE}/market/depth?symbol={sym}&type=step0&depth=10")
    write(os.path.join(raw_dir, "order_book.json"), {"method": "fetch_order_book", "raw": depth})
    tick = depth["tick"]
    parsed = ex.parse_order_book(tick, "BTC/USDT")
    write(os.path.join(parsed_dir, "order_book.json"), {"method": "fetch_order_book", "parsed": normalize_key(parsed)})

    trade = get_json(f"{BASE}/market/trade?symbol={sym}")
    trades = trade["tick"]["data"]
    write(os.path.join(raw_dir, "trades.json"), {"method": "fetch_trades", "raw": trades})
    parsed = [ex.parse_trade(t, spot_market) for t in trades]
    write(os.path.join(parsed_dir, "trades.json"), {"method": "fetch_trades", "parsed": normalize_key(parsed)})

    kline = get_json(f"{BASE}/market/history/kline?symbol={sym}&period=1min&size=5")
    write(os.path.join(raw_dir, "ohlcv.json"), {"method": "fetch_ohlcv", "raw": kline["data"]})
    parsed = [ex.parse_ohlcv(k) for k in kline["data"]]
    write(os.path.join(parsed_dir, "ohlcv.json"), {"method": "fetch_ohlcv", "parsed": normalize_key(parsed)})
    return 0


if __name__ == "__main__":
    sys.exit(main())
