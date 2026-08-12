#!/usr/bin/env python3
"""kraken 差分录制:raw + ccxt 解析基准(与 record_okx.py 同思路)。"""

import json
import os
import re
import sys
import urllib.request

BASE = "https://api.kraken.com/0/public"


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

    ex = ccxt.kraken({"enableRateLimit": True})
    ex.load_markets()
    raw_dir = os.path.join("tests", "fixtures", "raw", "kraken")
    parsed_dir = os.path.join("tests", "fixtures", "ccxt_parsed", "kraken")

    pair = "XBTUSD"
    market = ex.market("BTC/USD")

    ticker_resp = get_json(f"{BASE}/Ticker?pair={pair}")["result"]
    ticker_key = next(iter(ticker_resp))
    ticker = ticker_resp[ticker_key]
    write(os.path.join(raw_dir, "ticker.json"), {"method": "fetch_ticker", "raw": ticker})
    parsed = ex.parse_ticker(ticker, market)
    write(os.path.join(parsed_dir, "ticker.json"), {"method": "fetch_ticker", "parsed": normalize_key(parsed)})

    depth_resp = get_json(f"{BASE}/Depth?pair={pair}&count=10")["result"]
    depth = depth_resp[next(iter(depth_resp))]
    write(os.path.join(raw_dir, "order_book.json"), {"method": "fetch_order_book", "raw": depth})
    parsed = ex.parse_order_book(depth, "BTC/USD")
    write(os.path.join(parsed_dir, "order_book.json"), {"method": "fetch_order_book", "parsed": normalize_key(parsed)})

    trades_resp = get_json(f"{BASE}/Trades?pair={pair}&count=5")["result"]
    trades = trades_resp[next(iter(trades_resp))]
    write(os.path.join(raw_dir, "trades.json"), {"method": "fetch_trades", "raw": trades})
    parsed = [ex.parse_trade(t, market) for t in trades]
    write(os.path.join(parsed_dir, "trades.json"), {"method": "fetch_trades", "parsed": normalize_key(parsed)})

    ohlc_resp = get_json(f"{BASE}/OHLC?pair={pair}&interval=1")["result"]
    ohlc = ohlc_resp[next(iter(ohlc_resp))][:5]
    write(os.path.join(raw_dir, "ohlcv.json"), {"method": "fetch_ohlcv", "raw": ohlc})
    parsed = [ex.parse_ohlcv(c, market) for c in ohlc]
    write(os.path.join(parsed_dir, "ohlcv.json"), {"method": "fetch_ohlcv", "parsed": normalize_key(parsed)})
    return 0


if __name__ == "__main__":
    sys.exit(main())
