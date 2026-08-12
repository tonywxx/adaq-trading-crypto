#!/usr/bin/env python3
"""Transpile ccxt 4.5.73 `describe()` into AdaQ Rust adapters (ADR-0005/0013/0016).

Reads the vendored ccxt (ccxt/python) `describe()` for every sync exchange and
emits, under ``src/adapters/generated/``, one Rust module per exchange that
embeds an ``ApiSpec`` (pure &'static data) and forwards the common REST methods
to the ``GenericExchange`` engine via ``impl_generated_adapter!``.

Only endpoints whose key matches the generic engine's routing candidates are
emitted (the engine can only route those), which keeps each adapter small and
reduces spurious matches. The full ``has`` capability map is still emitted for
contract/visibility.

The 10 hand-written crypto adapters (binance/okx/bybit/kraken/coinbase/bitget/
gate/mexc/htx/kucoin) are skipped — they carry precise, differential-tested
implementations. The prediction markets kalshi/polymarket/manifold are
hand-written; the long-tail prediction markets (limitless/myriad/opinion) are
transpiled from the ccxt.prediction namespace alongside the CEX/DEX set.
"""

from __future__ import annotations

import json
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CCXT_PY = os.path.join(ROOT, "ccxt", "python")
GEN_DIR = os.path.join(ROOT, "src", "adapters", "generated")
AGG_FILE = os.path.join(ROOT, "src", "adapters", "generated.rs")

# 精选手写集:跳过(已有精确 + 差分测试实现)。
HANDWRITTEN = {
    "binance", "okx", "bybit", "kraken", "coinbase", "bitget",
    "gate", "mexc", "htx", "kucoin",
}
# 预测市场(若出现在 ccxt 集中,归类为 Prediction)。
PREDICTION = {
    "kalshi", "polymarket", "manifold", "augur", "metaculus", "limitless",
    "myriad", "opinion", "brainstorm", "zeitgeist", "olympiad", "omen", "poly",
}
# 已在其他处覆盖、禁止从 ccxt.prediction 命名空间重复生成的预测市场。
# - binance/hyperliquid:已在同步集(CEX/DEX)手写覆盖。
# - kalshi/polymarket:手写预测市场适配器(精确实作)。
PREDICTION_SKIP = {"binance", "hyperliquid", "kalshi", "polymarket"}

VERBS = {"get", "post", "put", "delete", "patch", "head", "options"}

# 通用引擎路由候选键(须与 src/generic.rs 中 find_first 的候选保持一致)。
CANDIDATES = [
    "time", "status", "systemstatus",
    "markets", "exchangeinfo", "instruments", "pairs", "symbols",
    "currencies", "assets", "currencys",
    "ticker", "tickers",
    "ohlcv", "kline", "klines", "candle", "candles", "ohlc",
    "orderbook", "order_book", "orderbook", "depth", "book",
    "trades", "trade",
    "balance", "account", "wallet", "balances",
    "order", "orders",
    "openorders", "open_orders", "openorders", "orders",
    "mytrades", "mytrade", "mytrades", "trades",
]


def key_matches(k: str, c: str) -> bool:
    # 去掉 {模板} 占位,按非字母数字切分为 token,逐 token 匹配:
    # 1) token==c; 2) c 为干净词边界; 3) token 以已知动词前缀起头其后接 c。
    PREFIXES = ("get", "fetch", "list", "query", "public")
    stripped = re.sub(r"\{[^}]*\}", "", k)
    tokens = [t for t in re.split(r"[^a-z0-9]", stripped.lower()) if t]
    for tok in tokens:
        if tok == c:
            return True
        idx = tok.find(c)
        if idx >= 0:
            before = tok[:idx]
            after = tok[idx + len(c):]
            if any(ch.isalnum() for ch in before):
                # 仍可能是 前缀+c(如 get+ticker)
                if any(before == p for p in PREFIXES) and not any(ch.isalpha() for ch in after):
                    return True
                continue
            if any(ch.isalpha() for ch in after):
                continue
            return True
    return False


def endpoint_relevant(key: str) -> bool:
    return any(key_matches(key, c) for c in CANDIDATES)


def first_str(node):
    if isinstance(node, str):
        return node
    if isinstance(node, dict):
        for v in node.values():
            r = first_str(v)
            if r:
                return r
    return None


def resolve_base(urls_api, segments):
    node = urls_api
    for seg in segments:
        if isinstance(node, dict) and seg in node:
            node = node[seg]
        else:
            break
    return first_str(node)


def walk_api(node, path):
    out = []
    if isinstance(node, dict):
        keys = set(node.keys())
        if keys and keys <= VERBS:
            for verb, entries in node.items():
                if isinstance(entries, dict):
                    for key, pth in entries.items():
                        # ccxt 路径解析:
                        #  - 值为字符串:显式路径(旧版 / 显式覆盖)。
                        #  - 值为 {cost:N} 字典 / 1 / True:路径即端点名(key 本身)。
                        #    ccxt 4.5.x 大量采用 `1` 作为占位,路径由 key 给出
                        #    (可能含 {模板},由引擎 fill_path 处理)。
                        if isinstance(pth, str):
                            path_val = pth
                        else:
                            path_val = key
                        out.append((path, verb.upper(), key, path_val))
            return out
        for k, v in node.items():
            out.extend(walk_api(v, path + [k]))
    return out


def rust_str(s: str) -> str:
    # ensure_ascii=False:非 ASCII 直接以 UTF-8 写入 Rust 源(Rust 源文件即 UTF-8,
    # 合法),避免 json 默认产生的 \uXXXX 转义(Rust 需要 \u{XXXX} 才是合法转义)。
    # json.dumps 仍会正确转义 `"` 与 `\`,与 Rust 字符串字面量兼容。
    return json.dumps(s, ensure_ascii=False)


def sanitize_mod(name: str) -> str:
    s = re.sub(r"[^a-z0-9]", "_", name.lower())
    if not s or not s[0].isalpha():
        s = "x" + s
    return s


def struct_name(name: str) -> str:
    parts = re.sub(r"[^a-z0-9]", "_", name.lower()).split("_")
    s = "".join(p[:1].upper() + p[1:] for p in parts if p)
    if not s or not s[0].isalpha():
        s = "X" + s
    return s


def classify(desc) -> str:
    if desc.get("dex"):
        return "Dex"
    if desc.get("id") in PREDICTION:
        return "Prediction"
    return "Cex"


def to_float(x):
    """安全转 float;容忍 None / 空 / 列表 / 字典(分层费率取首值)/ 字符串。"""
    if x is None or x == "":
        return 0.0
    if isinstance(x, bool):
        return 1.0 if x else 0.0
    if isinstance(x, (int, float)):
        return float(x)
    if isinstance(x, str):
        try:
            return float(x)
        except ValueError:
            return 0.0
    if isinstance(x, list):
        for item in x:
            f = to_float(item)
            if f != 0.0:
                return f
        return 0.0
    if isinstance(x, dict):
        for item in x.values():
            f = to_float(item)
            if f != 0.0:
                return f
        return 0.0
    return 0.0


def extract(src, ex_id: str):
    ex = getattr(src, ex_id)()
    d = ex.describe()
    urls_api = d.get("urls", {}).get("api", {}) or {}
    if not isinstance(urls_api, dict):
        urls_api = {}
    raw_endpoints = walk_api(d.get("api", {}) or {}, [])
    endpoints = []
    for path, verb, key, pth in raw_endpoints:
        if not endpoint_relevant(key):
            continue
        base = resolve_base(urls_api, path)
        if not base:
            continue
        auth = not any(
            seg.lower().find("public") >= 0
            or seg.lower().find("data") >= 0
            or seg.lower().find("open") >= 0
            for seg in path
        )
        endpoints.append((base, verb, key, pth, auth))
    if not endpoints:
        return None  # 无通用可达端点,跳过(引擎无法路由)
    has = []
    for k, v in (d.get("has", {}) or {}).items():
        if v is True or (isinstance(v, str) and v.lower() not in ("", "false")):
            has.append(k)
    fees = (d.get("fees", {}) or {}).get("trading", {}) or {}
    taker = to_float(fees.get("taker"))
    maker = to_float(fees.get("maker"))
    timeframes = list((d.get("timeframes", {}) or {}).keys())
    version = d.get("version")
    version = "" if version is None else str(version)
    rate_limit = int(to_float(d.get("rateLimit")))
    return {
        "id": d.get("id") or ex_id,
        "name": d.get("name") or ex_id,
        "version": version,
        "rate_limit_ms": rate_limit,
        "has": sorted(has),
        "endpoints": endpoints,
        "taker": taker,
        "maker": maker,
        "timeframes": timeframes,
        "kind": classify(d),
    }


def render(spec) -> str:
    lines = []
    lines.append(f"//! {spec['name']} (`{spec['id']}`) 转译适配器 — 由 `scripts/gen_adapters.py`")
    lines.append("//! 从 ccxt 4.5.73 `describe()` 自动生成(best-effort 批量补齐)。")
    lines.append("//! 保留 MIT 声明见仓库 `NOTICE`;精确性由精选手写集保证。")
    lines.append(f"//! 类别: {spec['kind']}。")
    lines.append("")
    lines.append("#![allow(clippy::too_many_arguments)]")
    lines.append("")
    lines.append("use crate::generic::{ApiSpec, Endpoint, MarketKind};")
    lines.append("")
    lines.append("/// ccxt `describe()` 静态快照(描述驱动引擎的输入)。")
    lines.append("pub static SPEC: ApiSpec = ApiSpec {")
    lines.append(f"    id: {rust_str(spec['id'])},")
    lines.append(f"    name: {rust_str(spec['name'])},")
    lines.append(f"    version: {rust_str(spec['version'])},")
    lines.append(f"    rate_limit_ms: {spec['rate_limit_ms']},")
    has = ", ".join(rust_str(h) for h in spec["has"]) or ""
    lines.append(f"    has: &[{has}],")
    ep_lines = []
    for base, verb, key, pth, auth in spec["endpoints"]:
        ep_lines.append(
            f"        Endpoint {{ base: {rust_str(base)}, verb: {rust_str(verb)}, "
            f"key: {rust_str(key)}, path: {rust_str(pth)}, auth: {str(auth).lower()} }},"
        )
    lines.append("    endpoints: &[")
    lines.extend(ep_lines)
    lines.append("    ],")
    lines.append(f"    taker: {spec['taker']!r},")
    lines.append(f"    maker: {spec['maker']!r},")
    tf = ", ".join(rust_str(t) for t in spec["timeframes"]) or ""
    lines.append(f"    timeframes: &[{tf}],")
    lines.append(f"    kind: MarketKind::{spec['kind']},")
    lines.append("};")
    lines.append("")
    lines.append(f"crate::impl_generated_adapter!({struct_name(spec['id'])}, &SPEC);")
    lines.append("")
    return "\n".join(lines)


def edit_cargo(generated_ids: list[str], full: bool):
    """幂等重写 [features]:移除已有 generated 组与生成 feature 行,重新插入。"""
    cargo = os.path.join(ROOT, "Cargo.toml")
    text = open(cargo).read()
    lines = text.splitlines()
    gid_set = set(generated_ids)
    out = []
    i = 0
    n = len(lines)
    in_features = False
    while i < n:
        line = lines[i]
        if line.strip() == "[features]":
            in_features = True
            out.append(line)
            i += 1
            continue
        if in_features and line.startswith("["):
            in_features = False
        if in_features:
            if line.strip().startswith("generated"):
                depth = 0
                while i < n:
                    depth += lines[i].count("[")
                    depth -= lines[i].count("]")
                    i += 1
                    if depth <= 0:
                        break
                continue
            m = re.match(r"^\s*([A-Za-z0-9_]+)\s*=\s*\[\]\s*$", line)
            if m and m.group(1) in gid_set:
                i += 1
                continue
            out.append(line)
            i += 1
            continue
        out.append(line)
        i += 1

    final = []
    inserted = False
    for line in out:
        final.append(line)
        if not inserted and line.strip() == "manifold = []":
            for gid in generated_ids:
                final.append(f"{gid} = []")
            final.append("generated = [")
            for gid in generated_ids:
                final.append(f'    "{gid}",')
            final.append("]")
            inserted = True
    if full:
        final2 = []
        in_full = False
        for line in final:
            if line.strip().startswith("full = ["):
                in_full = True
                final2.append(line)
                final2.append('    "generated",')
                continue
            if in_full:
                # 跳过已存在的 generated 副本,保证幂等(每次全量运行只保留一个)。
                if line.strip().strip('",').strip() == "generated":
                    continue
                if line.strip() == "]":
                    in_full = False
            final2.append(line)
        final = final2
    open(cargo, "w").write("\n".join(final) + "\n")


def main():
    sys.path.insert(0, CCXT_PY)
    import ccxt  # noqa: E402

    only = None
    for a in sys.argv[1:]:
        if a.startswith("--only="):
            only = a.split("=", 1)[1]

    ids = ccxt.exchanges
    try:
        import ccxt.prediction as pred_mod  # noqa: E402
        pred_ids_all = list(pred_mod.exchanges)
    except Exception:  # noqa: BLE001
        pred_mod = None
        pred_ids_all = []
    pred_ids = [e for e in pred_ids_all if e not in PREDICTION_SKIP]

    if only:
        if only in ids:
            ids = [only]
            pred_target = None
        elif only in pred_ids:
            ids = []
            pred_target = only
        else:
            ids = []
            pred_target = None
    else:
        pred_target = "__all__"

    os.makedirs(GEN_DIR, exist_ok=True)
    generated = []  # (modname, spec)
    skipped_hw = []
    skipped_none = []
    counts = {"Cex": 0, "Dex": 0, "Prediction": 0}

    for ex_id in ids:
        if ex_id in HANDWRITTEN:
            skipped_hw.append(ex_id)
            continue
        try:
            spec = extract(ccxt, ex_id)
        except Exception as e:  # noqa: BLE001
            print(f"  skip {ex_id}: describe error {e!r}", file=sys.stderr)
            skipped_none.append(ex_id)
            continue
        if spec is None:
            skipped_none.append(ex_id)
            continue
        mod = sanitize_mod(spec["id"])
        path = os.path.join(GEN_DIR, f"{mod}.rs")
        open(path, "w").write(render(spec))
        generated.append((mod, spec))
        counts[spec["kind"]] += 1

    # 预测市场命名空间(ccxt.prediction):补齐 limitless/myriad/opinion 等长尾。
    if pred_target is not None and pred_mod is not None:
        targets = pred_ids if pred_target == "__all__" else [pred_target]
        for ex_id in targets:
            try:
                spec = extract(pred_mod, ex_id)
            except Exception as e:  # noqa: BLE001
                print(f"  skip {ex_id}: describe error {e!r}", file=sys.stderr)
                skipped_none.append(ex_id)
                continue
            if spec is None:
                skipped_none.append(ex_id)
                continue
            mod = sanitize_mod(spec["id"])
            path = os.path.join(GEN_DIR, f"{mod}.rs")
            open(path, "w").write(render(spec))
            generated.append((mod, spec))
            counts[spec["kind"]] += 1

    # 聚合模块
    agg = []
    agg.append("//! 转译生成的交易所适配器聚合模块(ADR-0016)。")
    agg.append("//! 由 `scripts/gen_adapters.py` 从 ccxt 4.5.73 `describe()` 自动生成;")
    agg.append("//! 不要手改 —— 重新运行脚本即可重建。")
    agg.append("#![allow(clippy::too_many_arguments)]")
    agg.append("")
    for mod, _ in generated:
        agg.append(f'#[cfg(feature = "{mod}")]')
        agg.append(f"pub mod {mod};")
    agg.append("")
    open(AGG_FILE, "w").write("\n".join(agg))

    if only:
        # 验证模式:只加单个 feature(不动 full/generated 组)
        edit_cargo([m for m, _ in generated], full=False)
    else:
        edit_cargo([m for m, _ in generated], full=True)

    print(f"generated: {len(generated)} (Cex={counts['Cex']} Dex={counts['Dex']} "
          f"Prediction={counts['Prediction']})")
    print(f"skipped hand-written: {len(skipped_hw)}")
    print(f"skipped (no generic endpoint / error): {len(skipped_none)} -> {skipped_none}")


if __name__ == "__main__":
    main()
