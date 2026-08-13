# AGENTS.md

Guidance for CodeBuddy Code (and other coding agents) operating in this repo.

## Agent skills

### Issue tracker

Issues and specs live as GitHub issues (uses the `gh` CLI). See `docs/agents/issue-tracker.md`.

### Triage labels

Five default triage roles with label strings equal to their names. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.
Before any non-trivial work, read `CONTEXT.md` (glossary) and the ADRs touching your area —
especially **ADR-0013** (adapter model) and **ADR-0016** (transpiler). ADRs: `docs/adr/0001..0016`.

## Commands

Toolchain: Rust edition 2024 / **MSRV 1.85**, Python 3.13 for the transpiler. CI runs `cargo
nextest` (not `cargo test`) and fails on any warning (`RUSTFLAGS="-D warnings"`).

```bash
# --- Prereq: pinned ccxt for adapter generation (CI installs this exact version) ---
pip install "ccxt==4.5.73"

# --- Build ---
cargo build --all-targets                       # default features
cargo build --all-targets --all-features        # all exchanges + realtime + sync

# --- Test (nextest) ---
cargo nextest run --all-targets --all-features
cargo nextest run --all-features <substring>    # run a single test / subset (Rust or file name)

# --- Lint & format (CI enforces -D warnings) ---
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check                      # check only
cargo fmt --all                                 # auto-format

# --- MSRV pin check (CI asserts rust-version == "1.85") ---
cargo metadata --no-deps --format-version 1 | jq -e '.packages[0].rust_version == "1.85"'

# --- Regenerate transpiled adapters from ccxt describe() ---
python3 scripts/gen_adapters.py                 # full regen (all ~95 generated exchanges)
python3 scripts/gen_adapters.py --only=<id>     # one exchange, e.g. --only=binance
# NOTE: README's `gen_adapters.py --ccxt /path/to/ccxt` is STALE — current CLI takes
#       no --ccxt flag and only supports `--only=<id>` (or full regen with no args).

# --- Verify the generated layer is fresh (this is exactly what CI `transpiler-fresh` does) ---
python3 scripts/gen_adapters.py && cargo fmt --all && \
  git diff --exit-code -- src/adapters/generated/ src/adapters/generated.rs Cargo.toml

# --- Transpiler <-> engine contract guard (no ccxt needed) ---
python3 scripts/test_sync.py
```

## Architecture

Single crate **`adaq-trading-crypto`** (ccxt-compatible, for the AdaQ quant platform). Every
exchange is a Cargo **feature**; `full` aggregates all + `realtime`; default = `binance`, `okx`,
`kalshi`, `polymarket`. Prediction markets = `prediction` feature (kalshi+polymarket+manifold).

**Unified surface**
- `src/exchange.rs` defines the public `Exchange` (REST) and `Realtime` (WS) traits — the
  ccxt-compatible `snake_case` API. This is the single source of truth for the method surface.
- `build.rs` extracts the trait method lists at compile time → `OUT_DIR/methods.rs`
  (`REST_METHODS` / `WS_METHODS`). `tests/contract.rs` asserts every trait method is covered —
  a **release gate** (ADR-0001). Change the trait, never the list.
- `src/types.rs` holds unified structs (Market / Ticker / OrderBook / OHLCV / Order / Trade /
  Position / Balance) with ccxt field names and the raw response under `info`.

**Adapter model (ADR-0013 / ADR-0016)**
- `src/httpcore.rs` (`HttpCore`) is the exchange-agnostic deep module: HTTP skeleton, market
  caching, client-side pagination/filtering, safe field extraction, `Precise` math.
- Each adapter fills **four seams**: `describe` (endpoint paths/params), `sign`, `handle_errors`,
  and field-mapping `parse` overrides. `HttpCore` is reused unchanged.
- **Curated adapters** (14, hand-authored, full surface) live in `src/adapters/*.rs`
  (binance, okx, bybit, kraken, coinbase, bitget, gate, mexc, htx, kucoin, hyperliquid, kalshi,
  polymarket, manifold). Prediction-market signing (RSA-PSS / EIP-712 / ECDSA) lives in the
  adapter, never the core.
- **Extended adapters** (~95) are *transpiled*, not hand-written: `scripts/gen_adapters.py` reads
  ccxt `describe()` and emits one module per exchange under `src/adapters/generated/`, aggregated
  by `src/adapters/generated.rs`. They fill only the `describe` seam and reuse `HttpCore` via the
  shared engine in `src/generic.rs`.
- `src/generic.rs` is the **transpiler contract anchor**: the `CANDIDATES` routing table and
  `key_matches` are the single source the Python transpiler must mirror exactly.

**Realtime (ADR-0009 / ADR-0015)** — `watch_*` in `src/realtime/` reuses the REST `parse_*`
parsers (realtime holds a REST adapter instance) and shares an incremental order-book engine.
`sync` covers REST only; WS requires async.

**Numbers & errors** — `src/decimal.rs` (`rust_decimal`) + `src/precise.rs` (mirrors ccxt
`Precise`; `decimal_to_precision`). `src/error.rs` wraps `ErrorKind` mirrored from ccxt's
exception tree, with context / retryability / `From` conversions for `?` propagation.

## Cross-language contract (do not break)

- `scripts/gen_adapters.py` ↔ `src/generic.rs`: `CANDIDATES` is derived from `generic.rs` (single
  source); `key_matches` must stay byte-consistent across Python and Rust — guarded by
  `scripts/test_sync.py`.
- Any edit to `generic.rs`, `gen_adapters.py`, or any **new exchange** MUST regenerate and commit
  the generated layer (`src/adapters/generated/`, `src/adapters/generated.rs`, `Cargo.toml`),
  or the CI `transpiler-fresh` job fails.
- The repo's `ccxt/` directory is **gitignored and not committed**. CI installs the pinned PyPI
  `ccxt==4.5.73` (the canonical version); do not assume the bundled `ccxt/` exists in CI.
