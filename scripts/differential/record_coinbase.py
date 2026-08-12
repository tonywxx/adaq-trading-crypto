#!/usr/bin/env python3
"""coinbase 差分录制:raw + ccxt 解析基准。

用真实 ccxt 解析【同一份 raw】生成基准:
    tests/fixtures/raw/coinbase/<method>.json          — 直接 HTTP 录原始响应
    tests/fixtures/ccxt_parsed/coinbase/<method>.json  — ccxt parse_* 解析结果

用法:
    .venv/bin/python scripts/differential/record_coinbase.py
"""

import json
import os
import re
import sys
import urllib.parse
import urllib.request

BASE = "https://api.coinbase.com/api/v3"


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

    ex = ccxt.coinbase({"enableRateLimit": True})
    ex.load_markets()
    raw_dir = os.path.join("tests", "fixtures", "raw", "coinbase")
    parsed_dir = os.path.join("tests", "fixtures", "ccxt_parsed", "coinbase")

    # markets:products 前 10(现货)
    products = get_json(f"{BASE}/brokerage/market/products")["products"][:10]
    write(os.path.join(raw_dir, "markets.json"), {"method": "fetch_markets", "raw": products})
    parsed = [ex.parse_spot_market(p, None) for p in products]
    write(os.path.join(parsed_dir, "markets.json"), {"method": "fetch_markets", "parsed": normalize_key(parsed)})

    pid = "BTC-USDT"
    ticker = get_json(f"{BASE}/brokerage/market/products/{pid}/ticker?limit=1")
    write(os.path.join(raw_dir, "ticker.json"), {"method": "fetch_ticker", "raw": ticker})
    # 复刻 fetch_ticker_v3:trades[0] → parse_ticker,bid/ask 取 best_bid/best_ask
    first = ticker["trades"][0]
    parsed = ex.parse_ticker(first)
    parsed["bid"] = ex.safe_number(ticker, "best_bid")
    parsed["ask"] = ex.safe_number(ticker, "best_ask")
    write(os.path.join(parsed_dir, "ticker.json"), {"method": "fetch_ticker", "parsed": normalize_key(parsed)})

    book = get_json(f"{BASE}/brokerage/market/product_book?product_id={pid}&limit=10")
    write(os.path.join(raw_dir, "order_book.json"), {"method": "fetch_order_book", "raw": book})
    data = book["pricebook"]
    parsed = ex.parse_order_book(data, "BTC/USDT", None, "bids", "asks", "price", "size")
    write(os.path.join(parsed_dir, "order_book.json"), {"method": "fetch_order_book", "parsed": normalize_key(parsed)})

    trades_resp = get_json(f"{BASE}/brokerage/market/products/{pid}/ticker?limit=5")
    trades = trades_resp["trades"]
    write(os.path.join(raw_dir, "trades.json"), {"method": "fetch_trades", "raw": trades})
    parsed = [ex.parse_trade(t) for t in trades]
    write(os.path.join(parsed_dir, "trades.json"), {"method": "fetch_trades", "parsed": normalize_key(parsed)})

    candles = get_json(f"{BASE}/brokerage/market/products/{pid}/candles?granularity=ONE_MINUTE&start={int(__import__('time').time()) - 300}&end={int(__import__('time').time())}")
    write(os.path.join(raw_dir, "ohlcv.json"), {"method": "fetch_ohlcv", "raw": candles["candles"]})
    parsed = [ex.parse_ohlcv(c) for c in candles["candles"]]
    write(os.path.join(parsed_dir, "ohlcv.json"), {"method": "fetch_ohlcv", "parsed": normalize_key(parsed)})
    return 0


if __name__ == "__main__":
    sys.exit(main())
