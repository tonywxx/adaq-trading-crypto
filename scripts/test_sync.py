#!/usr/bin/env python3
"""守护 transpiler 与 Rust 引擎之间的契约,防止静默漂移。

Guard the contract between the transpiler (gen_adapters.py) and the Rust engine
(generic.rs) against silent drift.

两项检查 / Two checks:
  A. routing_candidates() 必须含全部必经候选键。这些键直接来自 src/generic.rs
     的 find_first 候选数组;若解析格式变更导致解析结果缩水,这些键会消失,
     测试即失败(而不是静默少发端点)。
     routing_candidates() must contain every must-have candidate key. These come
     straight from the find_first candidate arrays in src/generic.rs; if the parse
     format changes and the result shrinks, these keys vanish and the test fails
     (instead of silently under-emitting endpoints).
  B. Python key_matches 必须与 src/generic.rs 的 key_matches 在同一组 fixtures
     上逐条一致。跨语言无法单一来源,故用 Rust 单测(generic.rs::key_matches_word_boundary)
     同一组用例做一致性守护。
     Python key_matches must match src/generic.rs::key_matches on the same fixtures,
     byte for byte. Cross-language code cannot share one source, so we reuse the
     exact cases from the Rust unit test (generic.rs::key_matches_word_boundary).

无外部依赖(不 import ccxt),可独立运行: `python3 scripts/test_sync.py`。
No third-party deps (does not import ccxt); run standalone:
    python3 scripts/test_sync.py
失败时以非零码退出,使 CI 立刻变红。
Exits non-zero on failure so CI goes red immediately.
"""

from __future__ import annotations

import os
import sys

SCRIPTS_DIR = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, SCRIPTS_DIR)

from gen_adapters import key_matches, routing_candidates  # noqa: E402

# A. 必经候选键(覆盖各方法,含曾漂移缺失的 system_status / ping)。
#    Must-have candidate keys (covering each method, incl. the once-drifted
#    system_status / ping).
MUST_HAVE = [
    "time", "status", "systemstatus", "system_status", "ping",
    "markets", "exchangeinfo", "instruments", "pairs", "symbols",
    "currencies", "assets", "currencys",
    "ticker", "tickers",
    "ohlcv", "kline", "klines", "candle", "candles", "ohlc",
    "orderbook", "order_book", "depth", "book",
    "trades", "trade",
    "balance", "account", "wallet", "balances",
    "order", "orders",
    "openorders", "open_orders",
    "mytrades", "mytrade",
]

# B. 与 src/generic.rs::key_matches_word_boundary 完全相同的 fixtures。
#    Same fixtures as src/generic.rs::key_matches_word_boundary.
KEY_MATCHES_FIXTURES = [
    ("order", "order", True),
    ("openorders", "openorders", True),
    ("openorders", "orders", False),   # 前置字母,不应误匹配
    ("allorders", "orders", False),
    ("orders", "orders", True),
    ("getticker", "ticker", True),     # get+名词 命名
    ("getorderbook", "orderbook", True),
    ("gettradehistorysummary", "trade", False),  # 中缀嵌入不算匹配
]


def main() -> int:
    # 检查 A:必经候选键齐全。
    # Check A: all must-have candidate keys present.
    cands = routing_candidates()
    missing = [k for k in MUST_HAVE if k not in cands]
    assert not missing, f"routing_candidates() missing must-have keys: {missing}"
    # 顺便核对解析数量级(若格式变更导致缩水,这里也会先报警)。
    # Sanity-check the parse magnitude (a format change shrinking the set trips here too).
    assert len(cands) >= 20, f"routing_candidates() too small: {len(cands)}"

    # 检查 B:key_matches 跨语言一致。
    # Check B: key_matches agrees across languages.
    mismatches = []
    for key, cand, expected in KEY_MATCHES_FIXTURES:
        got = key_matches(key, cand)
        if got != expected:
            mismatches.append((key, cand, expected, got))
    assert not mismatches, (
        "key_matches drift vs src/generic.rs::key_matches_word_boundary:\n"
        + "\n".join(
            f"  key_matches({k!r}, {c!r}) = {g}, expected {e}"
            for (k, c, e, g) in mismatches
        )
    )

    print(
        f"OK: routing_candidates()={len(cands)} keys; "
        f"key_matches cross-check passed on {len(KEY_MATCHES_FIXTURES)} fixtures."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
