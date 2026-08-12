#!/usr/bin/env python3
"""bybit 差分录制:raw + ccxt 解析基准(与 record_okx.py 同思路)。"""

import json
import os
import re
import sys
import urllib.request

BASE = "https://api.bybit.com/v5"


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

    ex = ccxt.bybit({"enableRateLimit": True})
    ex.load_markets()
    raw_dir = os.path.join("tests", "fixtures", "raw", "bybit")
    parsed_dir = os.path.join("tests", "fixtures", "ccxt_parsed", "bybit")

    # bybit 的 parse_market 为 NotSupported(ccxt);markets 差分由单元测试覆盖,
    # 此处仅录制 raw 供参考。
    instruments = get_json(f"{BASE}/market/instruments-info?category=spot")["result"]["list"][:10]
    write(os.path.join(raw_dir, "markets.json"), {"method": "fetch_markets", "raw": instruments})

    inst = "BTCUSDT"
    ticker = get_json(f"{BASE}/market/tickers?category=spot&symbol={inst}")["result"]["list"][0]
    write(os.path.join(raw_dir, "ticker.json"), {"method": "fetch_ticker", "raw": ticker})
    parsed = ex.parse_ticker(ticker)
    write(os.path.join(parsed_dir, "ticker.json"), {"method": "fetch_ticker", "parsed": normalize_key(parsed)})

    book = get_json(f"{BASE}/market/orderbook?category=spot&symbol={inst}&limit=10")
    write(os.path.join(raw_dir, "order_book.json"), {"method": "fetch_order_book", "raw": book})
    parsed = ex.parse_order_book(book["result"], "BTC/USDT", None, "b", "a", 0, 1)
    write(os.path.join(parsed_dir, "order_book.json"), {"method": "fetch_order_book", "parsed": normalize_key(parsed)})

    trades = get_json(f"{BASE}/market/recent-trade?category=spot&symbol={inst}&limit=5")["result"]["list"]
    write(os.path.join(raw_dir, "trades.json"), {"method": "fetch_trades", "raw": trades})
    parsed = [ex.parse_trade(t) for t in trades]
    write(os.path.join(parsed_dir, "trades.json"), {"method": "fetch_trades", "parsed": normalize_key(parsed)})

    candles = get_json(f"{BASE}/market/kline?category=spot&symbol={inst}&interval=1&limit=5")["result"]["list"]
    write(os.path.join(raw_dir, "ohlcv.json"), {"method": "fetch_ohlcv", "raw": candles})
    parsed = [ex.parse_ohlcv(c) for c in candles]
    write(os.path.join(parsed_dir, "ohlcv.json"), {"method": "fetch_ohlcv", "parsed": normalize_key(parsed)})
    return 0


if __name__ == "__main__":
    sys.exit(main())
