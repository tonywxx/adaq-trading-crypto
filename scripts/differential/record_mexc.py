#!/usr/bin/env python3
"""mexc 差分录制:raw + ccxt 解析基准。

用真实 ccxt 解析【同一份 raw】生成基准:
    tests/fixtures/raw/mexc/<method>.json          — 直接 HTTP 录原始响应
    tests/fixtures/ccxt_parsed/mexc/<method>.json  — ccxt parse_* 解析结果

用法:
    .venv/bin/python scripts/differential/record_mexc.py
"""

import json
import os
import re
import sys
import urllib.request

BASE = "https://api.mexc.com/api/v3"


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

    ex = ccxt.mexc({"enableRateLimit": True})
    ex.load_markets()
    raw_dir = os.path.join("tests", "fixtures", "raw", "mexc")
    parsed_dir = os.path.join("tests", "fixtures", "ccxt_parsed", "mexc")

    info = get_json(f"{BASE}/exchangeInfo")
    symbols = [s for s in info["symbols"] if s["status"] == "1"][:10]
    write(os.path.join(raw_dir, "markets.json"), {"method": "fetch_markets", "raw": symbols})
    parsed = []
    for s in symbols:
        m = ex.markets_by_id[s["symbol"]][0]
        parsed.append({k: m.get(k) for k in ["id", "symbol", "base", "quote", "active", "precision"]})
    write(os.path.join(parsed_dir, "markets.json"), {"method": "fetch_markets", "parsed": normalize_key(parsed)})

    sym = "BTCUSDT"
    spot_market = ex.markets_by_id[sym][0]
    ticker = get_json(f"{BASE}/ticker/24hr?symbol={sym}")
    write(os.path.join(raw_dir, "ticker.json"), {"method": "fetch_ticker", "raw": ticker})
    parsed = ex.parse_ticker(ticker, spot_market)
    write(os.path.join(parsed_dir, "ticker.json"), {"method": "fetch_ticker", "parsed": normalize_key(parsed)})

    book = get_json(f"{BASE}/depth?symbol={sym}&limit=10")
    write(os.path.join(raw_dir, "order_book.json"), {"method": "fetch_order_book", "raw": book})
    parsed = ex.parse_order_book(book, "BTC/USDT")
    write(os.path.join(parsed_dir, "order_book.json"), {"method": "fetch_order_book", "parsed": normalize_key(parsed)})

    trades = get_json(f"{BASE}/trades?symbol={sym}&limit=5")
    write(os.path.join(raw_dir, "trades.json"), {"method": "fetch_trades", "raw": trades})
    parsed = [ex.parse_trade(t, spot_market) for t in trades]
    write(os.path.join(parsed_dir, "trades.json"), {"method": "fetch_trades", "parsed": normalize_key(parsed)})

    klines = get_json(f"{BASE}/klines?symbol={sym}&interval=1m&limit=5")
    write(os.path.join(raw_dir, "ohlcv.json"), {"method": "fetch_ohlcv", "raw": klines})
    parsed = [ex.parse_ohlcv(k) for k in klines]
    write(os.path.join(parsed_dir, "ohlcv.json"), {"method": "fetch_ohlcv", "parsed": normalize_key(parsed)})
    return 0


if __name__ == "__main__":
    sys.exit(main())
