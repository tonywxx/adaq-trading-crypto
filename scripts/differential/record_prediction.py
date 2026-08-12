#!/usr/bin/env python3
"""预测市场差分录制:kalshi / polymarket 的 raw + ccxt 解析基准。

注意:ccxt 的预测市场适配器是 async-only(ccxt.prediction),脚本整体用 asyncio。
与 record_raw.py / parse_compare.py 同思路(ADR-0010「同一输入」差分):
1. 用真实 ccxt fetch_markets 确定目标 outcome symbol;
2. 直接 HTTP 录制公共端点原始响应 → tests/fixtures/raw/<exchange>/;
3. 用 ccxt 公开 parse_* 解析【同一份 raw】→ tests/fixtures/ccxt_parsed/<exchange>/,
   Rust 侧解析器解析同一 raw 后与之逐字段比对。

用法:
    .venv/bin/python scripts/differential/record_prediction.py --exchange kalshi
    .venv/bin/python scripts/differential/record_prediction.py --exchange polymarket
"""

import argparse
import asyncio
import json
import os
import re
import sys
import time
import urllib.parse
import urllib.request

GAMMA = "https://gamma-api.polymarket.com"
CLOB = "https://clob.polymarket.com"
DATA = "https://data-api.polymarket.com"
KALSHI = "https://external-api.kalshi.com/trade-api/v2"


def snake(s: str) -> str:
    s = re.sub(r"(?<=[a-z0-9])(?=[A-Z])", "_", s)
    return s.lower()


def normalize_key(v):
    if isinstance(v, dict):
        return {snake(k): normalize_key(x) for k, x in v.items()}
    if isinstance(v, list):
        return [normalize_key(x) for x in v]
    return v


def get_json(url: str) -> dict:
    req = urllib.request.Request(url, headers={
        "User-Agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36",
        "Accept": "application/json",
    })
    with urllib.request.urlopen(req, timeout=20) as resp:
        return json.load(resp)


def write(dest: str, payload: dict) -> None:
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    with open(dest, "w", encoding="utf-8") as f:
        json.dump(payload, f, indent=2, ensure_ascii=False)
    print(f"wrote {dest}")


async def record_kalshi(out_dir: str) -> str:
    from ccxt.prediction import kalshi as kalshi_cls

    ex = kalshi_cls({"enableRateLimit": True})
    await ex.load_markets()
    markets = list(ex.markets.values())
    target = markets[0]
    outcome = target["outcomes"][0]["outcome"]
    outcome_obj = ex.outcome(outcome)
    ticker = outcome_obj["info"]["ticker"]
    series = outcome_obj["info"]["seriesTicker"]
    print(f"kalshi target outcome: {outcome} (ticker={ticker}, series={series})")

    raw_dir = os.path.join(out_dir, "raw", "kalshi")
    parsed_dir = os.path.join(out_dir, "ccxt_parsed", "kalshi")

    markets_resp = get_json(f"{KALSHI}/markets?limit=10")
    raw_markets = markets_resp["markets"]
    write(os.path.join(raw_dir, "markets.json"), {"method": "fetch_markets", "raw": raw_markets})
    parsed = [ex.parse_market(m) for m in raw_markets]
    write(os.path.join(parsed_dir, "markets.json"), {"method": "fetch_markets", "parsed": normalize_key(parsed)})

    raw_market = get_json(f"{KALSHI}/markets/{ticker}")["market"]
    write(os.path.join(raw_dir, "ticker.json"), {"method": "fetch_ticker", "raw": raw_market, "outcome": outcome})
    parsed = ex.parse_prediction_ticker(raw_market, outcome_obj)
    write(os.path.join(parsed_dir, "ticker.json"), {"method": "fetch_ticker", "outcome": outcome, "parsed": normalize_key(parsed)})

    book = get_json(f"{KALSHI}/markets/{ticker}/orderbook")
    write(os.path.join(raw_dir, "order_book.json"), {"method": "fetch_order_book", "raw": book, "outcome": outcome})
    b = book.get("orderbook_fp", book)
    raw_yes = b.get("yes_dollars", [])
    raw_no = b.get("no_dollars", [])
    is_no = outcome_obj["label"] == "NO"
    if is_no:
        bids, asks = raw_no, [[str(1 - float(p)), s] for p, s in raw_yes]
    else:
        bids, asks = raw_yes, [[str(1 - float(p)), s] for p, s in raw_no]
    book_dict = {
        "bids": sorted(bids, key=lambda x: float(x[0]), reverse=True),
        "asks": sorted(asks, key=lambda x: float(x[0])),
    }
    parsed = ex.parse_order_book(book_dict, outcome, None, "bids", "asks", 0, 1)
    write(os.path.join(parsed_dir, "order_book.json"), {"method": "fetch_order_book", "outcome": outcome, "parsed": normalize_key(parsed)})

    trades_resp = get_json(f"{KALSHI}/markets/trades?ticker={ticker}&limit=5")
    raw_trades = trades_resp.get("trades", [])
    write(os.path.join(raw_dir, "trades.json"), {"method": "fetch_trades", "raw": raw_trades, "outcome": outcome})
    parsed = [ex.parse_prediction_trade(t, outcome_obj) for t in raw_trades]
    write(os.path.join(parsed_dir, "trades.json"), {"method": "fetch_trades", "outcome": outcome, "parsed": normalize_key(parsed)})

    now = int(time.time())
    url = f"{KALSHI}/series/{series}/markets/{ticker}/candlesticks?period_interval=1&start_ts={now - 300}&end_ts={now}"
    candles = get_json(url).get("candlesticks", [])
    raw_candles = [c for c in candles if c.get("price", {}).get("open_dollars") is not None]
    write(os.path.join(raw_dir, "ohlcv.json"), {"method": "fetch_ohlcv", "raw": raw_candles, "outcome": outcome})
    ex.options["ohlcvCandleDurationSeconds"] = 60
    parsed = [ex.parse_ohlcv(c) for c in raw_candles]
    write(os.path.join(parsed_dir, "ohlcv.json"), {"method": "fetch_ohlcv", "outcome": outcome, "parsed": normalize_key(parsed)})
    await ex.close()
    return outcome


async def record_polymarket(out_dir: str) -> str:
    from ccxt.prediction import polymarket as polymarket_cls

    ex = polymarket_cls({"enableRateLimit": True})
    await ex.load_markets()
    markets = list(ex.markets.values())
    target = markets[0]
    outcome = target["outcomes"][0]["outcome"]
    outcome_obj = ex.outcome(outcome)
    token_id = outcome_obj["outcomeId"]
    condition_id = outcome_obj["info"]["conditionId"]
    print(f"polymarket target outcome: {outcome} (token={token_id[:16]}..., condition={condition_id[:16]}...)")

    raw_dir = os.path.join(out_dir, "raw", "polymarket")
    parsed_dir = os.path.join(out_dir, "ccxt_parsed", "polymarket")

    events = get_json(f"{GAMMA}/events?limit=3&status=active")
    write(os.path.join(raw_dir, "markets.json"), {"method": "fetch_markets", "raw": events})
    parsed = []
    for ev in events:
        parsed.extend(ex.parse_event_to_markets(ev))
    write(os.path.join(parsed_dir, "markets.json"), {"method": "fetch_markets", "parsed": normalize_key(parsed)})

    mid = get_json(f"{CLOB}/midpoint?token_id={token_id}")
    book = get_json(f"{CLOB}/book?token_id={token_id}")
    last_trade = get_json(f"{CLOB}/last-trade-price?token_id={token_id}")
    ticker_input = {"midpoint": mid, "book": book, "lastTrade": last_trade}
    write(os.path.join(raw_dir, "ticker.json"), {"method": "fetch_ticker", "raw": ticker_input, "outcome": outcome})
    parsed = ex.parse_prediction_ticker(ticker_input, outcome_obj)
    write(os.path.join(parsed_dir, "ticker.json"), {"method": "fetch_ticker", "outcome": outcome, "parsed": normalize_key(parsed)})

    write(os.path.join(raw_dir, "order_book.json"), {"method": "fetch_order_book", "raw": book, "outcome": outcome})
    ts = int(book.get("timestamp", 0)) or None
    parsed = ex.parse_order_book(book, outcome, ts, "bids", "asks", "price", "size")
    write(os.path.join(parsed_dir, "order_book.json"), {"method": "fetch_order_book", "outcome": outcome, "parsed": normalize_key(parsed)})

    trades_resp = get_json(f"{DATA}/trades?market={condition_id}&limit=100")
    raw_trades = [t for t in trades_resp if t.get("asset") == token_id][:5]
    write(os.path.join(raw_dir, "trades.json"), {"method": "fetch_trades", "raw": raw_trades, "outcome": outcome})
    parsed = [ex.parse_prediction_trade(t, outcome_obj) for t in raw_trades]
    write(os.path.join(parsed_dir, "trades.json"), {"method": "fetch_trades", "outcome": outcome, "parsed": normalize_key(parsed)})
    await ex.close()
    return outcome


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--exchange", default="kalshi", choices=["kalshi", "polymarket"])
    args = ap.parse_args()
    out_dir = os.path.join("tests", "fixtures")
    if args.exchange == "kalshi":
        asyncio.run(record_kalshi(out_dir))
    else:
        asyncio.run(record_polymarket(out_dir))
    return 0


if __name__ == "__main__":
    sys.exit(main())
