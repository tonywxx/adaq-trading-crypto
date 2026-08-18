# Changelog / 更新日志

All notable changes to this project are documented here. This file follows a
bilingual format: every release lists an **English** summary followed by a
**中文** summary.

本文件记录本项目的所有重要变更，采用中英双语格式：每个版本先给出 **English**
摘要，再给出 **中文** 摘要。

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

格式参考 [Keep a Changelog](https://keepachangelog.com/)，并遵循
[语义化版本](https://semver.org/lang/zh-CN/)（Semantic Versioning）。

---

## [1.0.8] - 2026-08-17

### English
- **Refactor:** unified signing and rate-limit handling across adapters for
  consistent behavior and less duplicated code.
- **CI:** regenerated `Cargo.toml` feature ordering to match the transpiler
  output, keeping the `transpiler-fresh` job green.

### 中文
- **重构：** 统一各适配器之间的签名与限流处理逻辑，行为更一致，重复代码更少。
- **CI：** 重新生成 `Cargo.toml` 的 feature 排序以匹配转译器输出，确保
  `transpiler-fresh` 任务通过。

---

## [1.0.7] - 2026-08-15

### English
- **Style:** ran `rustfmt` on `build.rs` after the v1.0.6 release fix.

### 中文
- **格式：** 在 v1.0.6 发布修复后对 `build.rs` 执行 `rustfmt` 格式化。

---

## [1.0.6] - 2026-08-15

### English
- **Fix (release):** write `adapter_reg.rs` into `OUT_DIR` so `cargo publish
  --verify` passes.
- **Feature:** refactored the transport layer to support a mock transport,
  enabling offline testing of adapters.

### 中文
- **修复（发布）：** 将 `adapter_reg.rs` 写入 `OUT_DIR`，以使 `cargo publish
  --verify` 通过。
- **新功能：** 重构传输层以支持 mock 传输，便于对适配器进行离线测试。

---

## [1.0.5] - 2026-08-14

### English
- **Refactor:** unified signing primitives to add RSA-PSS and recoverable
  ECDSA support for the Kalshi and Polymarket adapters.
- **Style:** improved formatting of the RSA signature verification code.
- **CI:** regenerated `Cargo.toml` with 4-space feature indentation for the
  `transpiler-fresh` job.

### 中文
- **重构：** 统一签名原语，为 Kalshi 与 Polymarket 适配器加入 RSA-PSS 与可恢复
  ECDSA 支持。
- **格式：** 优化 RSA 签名验证代码的可读性。
- **CI：** 以 4 空格 feature 缩进重新生成 `Cargo.toml`，适配 `transpiler-fresh` 任务。

---

## [1.0.4] - 2026-08-14

### English
- **Refactor:** consolidated signing and parsing into shared deep modules.
- **CI:** regenerated `Cargo.toml` feature ordering for the `transpiler-fresh`
  job.
- **Chore:** removed the `live-diff.yml` workflow and added `graphify-out` to
  `.gitignore`.

### 中文
- **重构：** 将签名与解析逻辑收敛到共享的深层模块中。
- **CI：** 为 `transpiler-fresh` 任务重新生成 `Cargo.toml` 的 feature 排序。
- **杂项：** 移除 `live-diff.yml` 工作流，并将 `graphify-out` 加入 `.gitignore`。

---

## [1.0.3] - 2026-08-13

### English
- **Feature:** promoted 8 exchanges to curated (hand-authored) adapters.
- **Fix (realtime):** made `BinanceWs` hold a Binance REST instance per
  ADR-0015.
- **Docs:** fixed stale `gen_adapters.py` usage and expanded `AGENTS.md`
  guidance.

### 中文
- **新功能：** 将 8 个交易所提升为精选（手写）适配器。
- **修复（实时）：** 按 ADR-0015 让 `BinanceWs` 持有 Binance REST 实例。
- **文档：** 修正 `gen_adapters.py` 的过时用法，并扩充 `AGENTS.md` 指引。

---

## [1.0.2] - 2026-08-12

### English
- **CI:** bumped `actions/setup-python` to v7 (Node 24 runtime) and installed
  the pinned `ccxt==4.5.73` in the freshness job instead of the bundled copy.
- **Refactor:** injected the adapter name into `HttpCore` and updated generated
  adapter endpoints.
- **Chore:** updated `Cargo.lock` dependencies.

### 中文
- **CI：** 将 `actions/setup-python` 升级到 v7（Node 24 运行时），并在 freshness
  任务中安装固定版本 `ccxt==4.5.73`，取代打包副本。
- **重构：** 将适配器名称注入 `HttpCore`，并更新生成适配器的端点。
- **杂项：** 更新 `Cargo.lock` 依赖。

---

## [1.0.1] - 2026-08-12

### English
- **Refactor:** consolidated authentication logic via ADR-0013, introduced a
  shared outcome index, and updated crate dependencies.
- **Refactor:** consolidated utility functions into `httpcore` and unified
  naming conventions across adapters and realtime modules.
- **Style:** applied `rustfmt` to kraken/okx/outcome/polymarket modules.
- **Chore:** upgraded `actions/checkout` from v4 to v5.

### 中文
- **重构：** 通过 ADR-0013 收敛鉴权逻辑，引入共享的 outcome 索引，并更新 crate 依赖。
- **重构：** 将工具函数收敛到 `httpcore`，并统一适配器与实时模块中的命名规范。
- **格式：** 对 kraken/okx/outcome/polymarket 模块执行 `rustfmt` 格式化。
- **杂项：** 将 `actions/checkout` 从 v4 升级到 v5。

---

## [1.0.0] - 2026-08-11

### English
- **Initial release.** A ccxt-compatible, unified trading interface for crypto
  and prediction markets, written in Rust for the AdaQ quant platform.
  - Unified REST `Exchange` trait with `snake_case` ccxt-compatible methods.
  - `HttpCore` standardizes HTTP client logic and market caching across adapters.
  - 100+ exchange adapters via a describe-driven engine (curated + generated).
  - Realtime WebSocket layer (`realtime` feature) with 8 `watch_*` channels for
    okx/bybit/kraken and Binance.
  - Optional synchronous wrapper (`sync` feature).
  - Prediction-market support: Kalshi, Polymarket (EIP-712), Manifold, plus
    Limitless / Myriad / Opinion.
  - Exact decimal arithmetic (`rust_decimal` + `Precise`).
  - Automated GitHub Actions release workflow and `nextest`-based quality gates.

### 中文
- **首发版本。** 为 AdaQ 量化交易平台打造的、兼容 ccxt 的加密货币与预测市场统一交易接口，
  使用 Rust 编写。
  - 统一的 REST `Exchange` trait，提供 `snake_case` 且兼容 ccxt 的方法。
  - `HttpCore` 统一各适配器的 HTTP 客户端逻辑与市场缓存。
  - 通过 describe 驱动引擎提供 100+ 交易所适配器（精选 + 生成）。
  - 实时 WebSocket 层（`realtime` feature），为 okx/bybit/kraken 与 Binance 提供 8 个
    `watch_*` 通道。
  - 可选的同步封装（`sync` feature）。
  - 预测市场支持：Kalshi、Polymarket（EIP-712）、Manifold，以及 Limitless / Myriad /
    Opinion。
  - 精确十进制运算（`rust_decimal` + `Precise`）。
  - 自动化的 GitHub Actions 发布工作流，以及基于 `nextest` 的质量门禁。
