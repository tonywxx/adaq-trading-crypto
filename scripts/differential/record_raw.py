#!/usr/bin/env python3
"""录制交易所【原始】响应到 tests/fixtures/raw/<exchange>/<method>.json。

与 record.py(ccxt 统一输出)配套:差分测试用「原始响应 → 我们的解析器 →
与 ccxt 统一输出逐字段比对」验证解析正确性(ADR-0010)。

仅公共端点,无需 API key。用 urllib,零第三方依赖。

用法:
    python3 scripts/differential/record_raw.py
"""

import json
import os
import sys
import urllib.parse
import urllib.request

BASE = "https://api.binance.com/api/v3"

# method -> (path, query params)
ENDPOINTS = {
    "time": ("/time", {}),
    "ticker": ("/ticker/24hr", {"symbol": "BTCUSDT"}),
    "ohlcv": ("/klines", {"symbol": "BTCUSDT", "interval": "1m", "limit": "5"}),
    "order_book": ("/depth", {"symbol": "BTCUSDT", "limit": "10"}),
    "trades": ("/trades", {"symbol": "BTCUSDT", "limit": "5"}),
}


def main() -> int:
    out_dir = os.path.join("tests", "fixtures", "raw", "binance")
    os.makedirs(out_dir, exist_ok=True)
    for name, (path, params) in ENDPOINTS.items():
        url = BASE + path
        if params:
            url += "?" + urllib.parse.urlencode(params)
        with urllib.request.urlopen(url, timeout=10) as resp:
            data = json.load(resp)
        dest = os.path.join(out_dir, f"{name}.json")
        with open(dest, "w", encoding="utf-8") as f:
            json.dump({"method": f"fetch_{name}", "symbol": "BTC/USDT", "raw": data}, f, indent=2)
        print(f"wrote {dest}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
