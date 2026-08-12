#!/usr/bin/env python3
"""okx 差分录制:raw + ccxt 解析基准(与 record_prediction.py 同思路)。

用真实 ccxt(同步,顶层 ccxt.okx)解析【同一份 raw】生成基准:
    tests/fixtures/raw/okx/<method>.json          — 直接 HTTP 录原始响应
    tests/fixtures/ccxt_parsed/okx/<method>.json  — ccxt parse_* 解析结果

用法:
    .venv/bin/python scripts/differential/record_okx.py
"""

import json
import os
import re
import sys
import urllib.parse
import urllib.request

BASE = "https://www.okx.com/api/v5"


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

    ex = ccxt.okx({"enableRateLimit": True})
    ex.load_markets()
    raw_dir = os.path.join("tests", "fixtures", "raw", "okx")
    parsed_dir = os.path.join("tests", "fixtures", "ccxt_parsed", "okx")

    # markets:instruments 前 10(ccxt 统一格式需 load_markets 的缓存)
    instruments = get_json(f"{BASE}/public/instruments?instType=SPOT")["data"][:10]
    write(os.path.join(raw_dir, "markets.json"), {"method": "fetch_markets", "raw": instruments})
    parsed = [ex.parse_market(i) for i in instruments]
    write(os.path.join(parsed_dir, "markets.json"), {"method": "fetch_markets", "parsed": normalize_key(parsed)})

    inst = "BTC-USDT"
    ticker = get_json(f"{BASE}/market/ticker?instId={inst}")["data"][0]
    write(os.path.join(raw_dir, "ticker.json"), {"method": "fetch_ticker", "raw": ticker})
    parsed = ex.parse_ticker(ticker)
    write(os.path.join(parsed_dir, "ticker.json"), {"method": "fetch_ticker", "parsed": normalize_key(parsed)})

    book = get_json(f"{BASE}/market/books?instId={inst}&sz=10")
    write(os.path.join(raw_dir, "order_book.json"), {"method": "fetch_order_book", "raw": book})
    data = book["data"][0]
    parsed = ex.parse_order_book(data, "BTC/USDT")
    write(os.path.join(parsed_dir, "order_book.json"), {"method": "fetch_order_book", "parsed": normalize_key(parsed)})

    trades = get_json(f"{BASE}/market/trades?instId={inst}&limit=5")["data"]
    write(os.path.join(raw_dir, "trades.json"), {"method": "fetch_trades", "raw": trades})
    parsed = [ex.parse_trade(t) for t in trades]
    write(os.path.join(parsed_dir, "trades.json"), {"method": "fetch_trades", "parsed": normalize_key(parsed)})

    candles = get_json(f"{BASE}/market/candles?instId={inst}&bar=1m&limit=5")["data"]
    write(os.path.join(raw_dir, "ohlcv.json"), {"method": "fetch_ohlcv", "raw": candles})
    parsed = [ex.parse_ohlcv(c) for c in candles]
    write(os.path.join(parsed_dir, "ohlcv.json"), {"method": "fetch_ohlcv", "parsed": normalize_key(parsed)})
    return 0


if __name__ == "__main__":
    sys.exit(main())
