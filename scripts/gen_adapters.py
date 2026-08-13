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

# 通用引擎路由候选键的唯一来源:解析 src/generic.rs 中所有 `.find_first(&[...])`
# 的候选数组(小写、去重)。
# The single source of truth for routing candidate keys: parse every
# `.find_first(&[...])` candidate array out of src/generic.rs (lowercased, deduped).
# 此前该列表在 Rust(generic.rs 的 find_first 候选)与 Python(本文件 CANDIDATES)
# 双份手工同步,存在静默漂移风险(generic.rs 新增候选但 Python 漏改 → 生成器
# 静默不发出对应端点 → 该方法在生成适配器上永远路由不到)。改为由 generic.rs
# 直接解析后,漂移在结构上不可能发生。
# Previously this list was hand-synced between Rust (generic.rs find_first candidates)
# and Python (this CANDIDATES literal), a silent-drift risk: adding a Rust candidate
# without updating Python meant the generator silently skipped emitting that endpoint,
# so the method could never route on generated adapters. Deriving it from generic.rs
# makes such drift structurally impossible.
GENERIC_RS = os.path.join(ROOT, "src", "generic.rs")


def routing_candidates() -> "set[str]":
    """解析 src/generic.rs 中全部 find_first 候选数组,作为路由候选键唯一来源。

    Parse every find_first candidate array in src/generic.rs; this is the sole
    source of routing candidate keys.

    解析守卫:若 find_first 调用形态变更导致解析为空/过少,应立刻失败,而非
    静默少发端点(那会表现为难以诊断的运行时路由缺失)。
    Parser guard: a broken parse must fail loudly, never silently under-emit
    (which would surface as hard-to-diagnose runtime routing gaps).
    """
    with open(GENERIC_RS, encoding="utf-8") as fh:
        src = fh.read()
    pat = re.compile(r"\.find_first\(\s*&\[([^\]]*)\]")
    cands: "set[str]" = set()
    for m in pat.finditer(src):
        for lit in re.findall(r'"([^"]*)"', m.group(1)):
            cands.add(lit.lower())
    assert len(cands) >= 20, f"routing_candidates parsed too few keys: {sorted(cands)}"
    return cands


# 通用引擎路由候选键(单一来源:由 src/generic.rs 的 find_first 候选解析得到)。
# Routing candidate keys (single source: derived from find_first candidates in src/generic.rs).
CANDIDATES = routing_candidates()


def key_matches(k: str, c: str) -> bool:
    # 必须与 src/generic.rs 的 key_matches 保持逐字节一致(契约:二者用同一套
    # 词边界/动词前缀规则决定端点 key 是否命中候选)。跨语言无法单一来源,故由
    # scripts/test_sync.py 用 Rust 单测同一组 fixtures 做一致性守护。
    # Must stay byte-for-byte equivalent to key_matches in src/generic.rs (the
    # contract: both decide endpoint-key hits via the same word-boundary / verb-prefix
    # rules). Cross-language code cannot share a single source, so scripts/test_sync.py
    # guards equivalence using the same fixtures as the Rust unit tests.
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


def fmt_list(indent: str, prefix: str, items: list[str], suffix: str) -> list[str]:
    """Emit a rustfmt-stable bracketed list of Rust atoms (default config).

    Mirrors rustfmt's array rules (verified empirically against rustfmt 1.97
    and 1.85; the construct only appears as a struct-field value at indent 4
    in this codebase):
      * single line iff items+separators total (real_total) <= array_width (60);
      * else, if every element is short (len <= short_array_element_width_
        threshold = 10): Mixed — packed lines, wrapping before a non-last
        item once the running line would exceed the nested width (91 here);
        the last item always joins the current line (rustfmt allows the final
        line to exceed the budget rather than leave a dangling item);
      * else Vertical: one item per line with trailing commas.
    """
    if not items:
        return [f"{indent}{prefix}[]" + suffix]
    real_total = sum(len(it) for it in items) + (len(items) - 1) * 2
    if real_total <= 60:
        return [f"{indent}{prefix}[{', '.join(items)}]{suffix}"]
    if all(len(it) <= 10 for it in items):
        # Mixed: pack as many items per line as fit the budget; never break
        # before the last item.
        out = [f"{indent}{prefix}["]
        item_indent = indent + "    "
        line_len = 0
        line_items = []
        for i, it in enumerate(items):
            tw = len(it) + 1  # item + separator `,`
            last = i == len(items) - 1
            if line_len > 0 and not last and line_len + 1 + tw > 91:
                out.append(item_indent + ", ".join(line_items) + ",")
                line_items = []
                line_len = 0
            elif line_len > 0:
                line_len += 1  # space between items
            line_items.append(it)
            line_len += tw
        if line_items:
            out.append(item_indent + ", ".join(line_items) + ",")
        out.append(f"{indent}]{suffix}")
        return out
    # Vertical: one element per line, trailing comma.
    out = [f"{indent}{prefix}["]
    for it in items:
        out.append(f"{indent}    {it},")
    out.append(f"{indent}]{suffix}")
    return out


def fmt_endpoint(indent: str, base: str, verb: str, key: str, pth: str, auth: bool) -> list[str]:
    """Emits one `Endpoint { ... }` struct literal, always field-per-line.

    rustfmt never collapses struct literals (verified empirically: even a
    short one that fits in 100 cols is forced to one-field-per-line), so
    always emitting the multi-line shape is the stable fixpoint.
    """
    return [
        f"{indent}Endpoint {{",
        f"{indent}    base: {rust_str(base)},",
        f"{indent}    verb: {rust_str(verb)},",
        f"{indent}    key: {rust_str(key)},",
        f"{indent}    path: {rust_str(pth)},",
        f"{indent}    auth: {str(auth).lower()},",
        f"{indent}}},",
    ]


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
    lines.extend(fmt_list("    ", "has: &", [rust_str(h) for h in spec["has"]], ","))
    lines.append("    endpoints: &[")
    for base, verb, key, pth, auth in spec["endpoints"]:
        lines.extend(fmt_endpoint("        ", base, verb, key, pth, auth))
    lines.append("    ],")
    lines.append(f"    taker: {spec['taker']!r},")
    lines.append(f"    maker: {spec['maker']!r},")
    lines.extend(fmt_list("    ", "timeframes: &", [rust_str(t) for t in spec["timeframes"]], ","))
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
    # Prefer a pip-installed ccxt as the canonical source (pinned to 4.5.73 in
    # CI). The repo-local bundled copy at ccxt/python (gitignored) is only a
    # fallback for offline use. Appending — not inserting at index 0 — keeps the
    # PyPI package authoritative so generated output stays reproducible across
    # machines (see .github/workflows/ci.yml). The bundled copy and PyPI 4.5.73
    # differ in describe() output, so the pinned PyPI version must win.
    sys.path.append(CCXT_PY)
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

    # 聚合模块(按模块名排序:rustfmt reorder_modules 会按字典序重排 `pub mod`,
    # 生成端先排好可保证 `cargo fmt --check` 稳定)。
    agg = []
    agg.append("//! 转译生成的交易所适配器聚合模块(ADR-0016)。")
    agg.append("//! 由 `scripts/gen_adapters.py` 从 ccxt 4.5.73 `describe()` 自动生成;")
    agg.append("//! 不要手改 —— 重新运行脚本即可重建。")
    agg.append("#![allow(clippy::too_many_arguments)]")
    agg.append("")
    for mod, _ in sorted(generated, key=lambda m: m[0]):
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
