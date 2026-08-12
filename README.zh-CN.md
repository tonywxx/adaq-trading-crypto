# adaq-trading-crypto

[![Crates.io](https://img.shields.io/crates/v/adaq-trading-crypto.svg)](https://crates.io/crates/adaq-trading-crypto)
[![docs.rs](https://docs.rs/adaq-trading-crypto/badge.svg)](https://docs.rs/adaq-trading-crypto)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue.svg)](https://blog.rust-lang.org)

面向 **加密货币与预测市场** 的统一交易接口，使用 Rust 编写，服务于
[AdaQ](https://github.com/tonywxx/adaq-trading-crypto) 量化交易平台。

本库的 API 面与统一数据结构与
[ccxt](https://github.com/ccxt/ccxt) 对齐：相同的 `snake_case` 方法名
（`fetch_ohlcv`、`create_order` …）、相同的字段命名，并以类型化结构体承载原始交易所响应（保存于
`info` 字段，作为兜底通道）。

> 📗 English documentation: [README.md](./README.md).

## 特性

- **ccxt 对齐的统一 REST API**：通过 `Exchange` trait 提供，`snake_case` 方法名与字段名，代码可近乎逐行地从 ccxt 移植。
- **109 个适配器**：14 个手写（10 个精选加密货币交易所 + Hyperliquid + 3 个预测市场），以及 95 个由 ccxt `describe()` 经 `scripts/gen_adapters.py` 与描述驱动的通用引擎（`generic.rs`）批量生成的适配器。
- **类型化、serde 容错的数据结构**：`Market`、`Ticker`、`OrderBook`、`OHLCV`、`Order`、`Trade`、`Position`、`Balance` …，并通过 `info` 保留原始响应以保证完整性。
- **精确十进制运算**：默认使用 `rust_decimal`，另提供 `Precise` 模块复刻 ccxt 的 `Precise` 语义，以及 `decimal_to_precision`（ccxt `decimalToPrecision` 的移植）。
- **可选的同步封装**（`sync` feature）：无需自行管理异步运行时。
- **可选的实时 WebSocket 层**（`realtime` feature）：8 个 `watch_*` 通道。
- **预测市场支持**：Kalshi、Polymarket（EIP-712 订单签名）、Manifold（原生），以及通过生成集提供的 Limitless / Myriad / Opinion。
- **Apache-2.0** 许可；源自 ccxt 的解析逻辑保留其 MIT 声明（见 `NOTICE`）。

## 安装

```toml
[dependencies]
adaq-trading-crypto = { version = "1", features = ["binance", "okx"] }
```

仅启用你需要的交易所（每个交易所对应一个 Cargo feature），并视情况启用 `realtime` / `sync`。
默认 feature 为 `binance`、`okx`、`kalshi`、`polymarket`。使用 `full` feature 可编译全部内容。

## 快速开始

### 异步

```rust
use adaq_trading_crypto::{Config, Exchange};
use adaq_trading_crypto::adapters::Binance;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut cfg = Config::new();
    cfg.api_key = Some("YOUR_KEY".into());
    cfg.secret  = Some("YOUR_SECRET".into());

    let ex = Binance::new(cfg)?;
    let ticker = ex.fetch_ticker("BTC/USDT", Default::default()).await?;
    println!("last = {}", ticker.last.unwrap());
    Ok(())
}
```

### 同步（无需管理运行时）

```rust
use adaq_trading_crypto::sync::BlockingExchange;
use adaq_trading_crypto::{Config, Exchange};
use adaq_trading_crypto::adapters::Binance;

let ex = BlockingExchange::new(Binance::new(Config::new())?)?;
let ticker = ex.fetch_ticker("BTC/USDT", Default::default())?;
println!("last = {}", ticker.last.unwrap());
```

### 预测市场（Polymarket）

```rust
use adaq_trading_crypto::adapters::Polymarket;
use adaq_trading_crypto::{Config, Exchange};

let ex = Polymarket::new(Config::new())?;
let markets = ex.fetch_markets(Default::default()).await?;
```

## 支持的交易所

### 手写适配器（14 个）

| Feature       | 交易所          | 范围                                                                 |
| ------------- | --------------- | -------------------------------------------------------------------- |
| `binance`     | Binance         | REST 公开 + 私有，WS 8 通道                                          |
| `okx`         | OKX             | REST 公开 + 私有，WS 8 通道                                          |
| `bybit`       | Bybit           | REST 公开 + 私有，WS 8 通道                                          |
| `kraken`      | Kraken          | REST 公开 + 私有，WS 8 通道（无 `positions` 通道）                   |
| `coinbase`    | Coinbase        | REST 公开 + 私有（v3 Advanced Trade）                               |
| `bitget`      | Bitget          | REST 公开 + 私有                                                     |
| `gate`        | Gate.io         | REST 公开 + 私有                                                     |
| `mexc`        | MEXC            | REST 公开 + 私有                                                     |
| `htx`         | HTX (Huobi)     | REST 公开 + 私有                                                     |
| `kucoin`      | KuCoin          | REST 公开 + 私有                                                     |
| `hyperliquid` | Hyperliquid     | DEX：公开行情数据（markets / tickers / ohlcv / order_book）          |
| `kalshi`      | Kalshi          | REST 完整（含下单），WS 私有通道                                     |
| `polymarket`  | Polymarket      | REST + EIP-712 订单签名，WS 5 通道                                   |
| `manifold`    | Manifold        | 原生适配器（无 ccxt 参考）：markets / ticker / trades                |

### 生成适配器（95 个）

其余交易所位于 `generated` feature 分组下，每个由各自的 Cargo feature 控制（如 `alpaca`、
`apex`、`deribit`、`krakenfutures` …）。它们由 `scripts/gen_adapters.py` 基于 ccxt `describe()`
生成，并共享同一个描述驱动的引擎（`generic.rs`）。它们以尽力而为的方式覆盖常用 REST 面
（markets / tickers / ohlcv / order_book / trades / balance / orders / create_order /
cancel_order / …），采用尽力而为的字段解析。需要交易所特定签名的方法保持 `NotSupported`，并交由精选集处理。
这也包含来自 `ccxt.prediction` 命名空间的三个长尾预测市场——**Limitless / Myriad / Opinion**——
走同一套流水线，而 Kalshi / Polymarket / Manifold 仍保持手写。

## Feature 开关

| Feature           | 作用                                                                 |
| ----------------- | -------------------------------------------------------------------- |
| 按交易所          | 编译进对应适配器（如 `binance`、`kraken`、`polymarket` …）           |
| `realtime`        | 启用 WebSocket `watch_*` 方法（见下方 8 个通道）                      |
| `sync`            | 异步 API 的阻塞封装（`sync::BlockingExchange`）                      |
| `prediction`      | `kalshi` + `polymarket` + `manifold`（手写预测市场）                 |
| `full`            | 全部交易所（精选 + 生成）+ `realtime`                                |
| _默认_            | `binance`、`okx`、`kalshi`、`polymarket`                            |

## 架构

适配器遵循 **HttpCore + 四接缝** 模型（ADR-0013）：

- **`HttpCore`** —— 一个与交易所无关的深层模块，负责 HTTP 请求骨架、市集缓存、客户端分页/过滤、安全字段提取，以及 `Precise` 运算。
- **四接缝** —— 每个适配器只需填写：`describe`（端点路径/参数）、`sign`（签名算法）、`handle_errors`（错误码映射），以及字段映射（`parse` 覆写）。生成适配器只填 `describe` 接缝，零改动复用 `HttpCore`。

预测市场的特有逻辑（结果上下文、合成 ticker、RSA-PSS / EIP-712 / ECDSA 签名）保留在适配器侧，不进入核心。

`types` 中的统一数据结构使用与 ccxt 对齐的 `snake_case` 字段名，并通过 `info` 保留原始响应以保证完整性。

## 实时（WebSocket）

启用 `realtime` 后，可使用 `Realtime` trait 上的 8 个 `watch_*` 通道：
`watch_ticker`、`watch_order_book`、`watch_trades`、`watch_ohlcv`、`watch_balance`、
`watch_orders`、`watch_my_trades`、`watch_positions`（见 ADR-0009）。`watch_*` 层复用 REST 的
`parse_*` 解析器（ADR-0015），并共享增量订单簿引擎。`sync` feature 仅覆盖 REST；WebSocket 层需要异步。

## 数值精度

价格与数量使用精确十进制运算（默认 `rust_decimal`）。公开的 `precise` 模块复刻 ccxt 的 `Precise`
语义，`decimal_to_precision(value, mode, precision, rounding, padding)` 复刻 ccxt 的
`decimalToPrecision`（小数位 / 有效数字 / tick size；round / truncate / up / down）。

## 错误处理

统一的 `Error` 封装了镜像 ccxt 异常树的 `ErrorKind`，携带上下文、可重试性，以及 `From` 转换，便于使用 `?` 进行符合人体工学的错误传播。

## 测试与质量

- **契约测试** —— 统一方法面在构建期从 `Exchange` / `Realtime` trait 提取（见 `build.rs`）并断言完整性；这是发布门槛。
- **差分测试** —— 解析器与来自相同原始响应的真实 ccxt 输出逐字段比对（离线、CI 中确定性 fixtures）。
- `clippy -D warnings`、`rustfmt`、MSRV **1.85**（edition **2024**）、`cargo nextest`。

## 开发

从本地 ccxt 检出重新生成 95 个转译适配器：

```bash
python3 scripts/gen_adapters.py --ccxt /path/to/ccxt
```

生成器基于 ccxt `describe()` 在 `src/adapters/generated/` 下产出按交易所划分的模块；运行时位于
`src/generic.rs`。

## 许可证

Apache-2.0。源自 ccxt 的解析逻辑保留其 MIT 声明 —— 见 `NOTICE`。

---

📗 English documentation: [README.md](./README.md).
