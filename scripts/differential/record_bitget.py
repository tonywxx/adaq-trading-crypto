#!/usr/bin/env python3
"""bitget 差分录制:raw + ccxt 解析基准。

用真实 ccxt 解析【同一份 raw】生成基准:
    tests/fixtures/raw/bitget/<method>.json          — 直接 HTTP 录原始响应
    tests/fixtures/ccxt_parsed/bitget/<method>.json  — ccxt parse_* 解析结果

用法:
    .venv/bin/python scripts/differential/record_bitget.py
"""

import json
import os
import re
import sys
import urllib.request

BASE = "https://api.bitget.com/api/v2"


def snake(s: str) -> str:
    s = re.sub(r"(?<=[a-z0-9])(?=[A-Z])", "_", s)
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

    ex = ccxt.bitget({"enableRateLimit": True})
    ex.load_markets()
    raw_dir = os.path.join("tests", "fixtures", "raw", "bitget")
    parsed_dir = os.path.join("tests", "fixtures", "ccxt_parsed", "bitget")

    # markets:symbols 前 10(ccxt market 结构从 load_markets 缓存取)
    symbols = get_json(f"{BASE}/spot/public/symbols")["data"][:10]
    write(os.path.join(raw_dir, "markets.json"), {"method": "fetch_markets", "raw": symbols})
    parsed = []
    for s in symbols:
        m = ex.markets_by_id[s["symbol"]][0]
        parsed.append({k: m.get(k) for k in ["id", "symbol", "base", "quote", "active", "precision"]})
    write(os.path.join(parsed_dir, "markets.json"), {"method": "fetch_markets", "parsed": normalize_key(parsed)})

    sym = "BTCUSDT"
    # BTCUSDT 同时存在于 spot/swap → 显式取现货市场,供 symbol 解析
    spot_market = next(m for m in ex.markets_by_id[sym] if m["spot"])
    ticker = get_json(f"{BASE}/spot/market/tickers?symbol={sym}")["data"][0]
    write(os.path.join(raw_dir, "ticker.json"), {"method": "fetch_ticker", "raw": ticker})
    parsed = ex.parse_ticker(ticker, spot_market)
    write(os.path.join(parsed_dir, "ticker.json"), {"method": "fetch_ticker", "parsed": normalize_key(parsed)})

    book = get_json(f"{BASE}/spot/market/orderbook?symbol={sym}&limit=10")
    write(os.path.join(raw_dir, "order_book.json"), {"method": "fetch_order_book", "raw": book})
    data = book["data"]
    parsed = ex.parse_order_book(data, "BTC/USDT")
    write(os.path.join(parsed_dir, "order_book.json"), {"method": "fetch_order_book", "parsed": normalize_key(parsed)})

    fills = get_json(f"{BASE}/spot/market/fills?symbol={sym}&limit=5")["data"]
    write(os.path.join(raw_dir, "trades.json"), {"method": "fetch_trades", "raw": fills})
    parsed = [ex.parse_trade(t, spot_market) for t in fills]
    write(os.path.join(parsed_dir, "trades.json"), {"method": "fetch_trades", "parsed": normalize_key(parsed)})

    candles = get_json(f"{BASE}/spot/market/candles?symbol={sym}&granularity=1min&limit=5")["data"]
    write(os.path.join(raw_dir, "ohlcv.json"), {"method": "fetch_ohlcv", "raw": candles})
    parsed = [ex.parse_ohlcv(c) for c in candles]
    write(os.path.join(parsed_dir, "ohlcv.json"), {"method": "fetch_ohlcv", "parsed": normalize_key(parsed)})
    return 0


if __name__ == "__main__":
    sys.exit(main())
