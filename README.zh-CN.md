# adaq-trading-crypto

[![Crates.io](https://img.shields.io/crates/v/adaq-trading-crypto.svg)](https://crates.io/crates/adaq-trading-crypto)
[![docs.rs](https://docs.rs/adaq-trading-crypto/badge.svg)](https://docs.rs/adaq-trading-crypto)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue.svg)](https://blog.rust-lang.org)

面向 **加密货币与预测市场** 的统一交易接口，使用 Rust 编写，服务于
[AdaQ](https://github.com/tonywxx/adaq) 量化交易平台。

本库的 API 面与统一数据结构 **兼容 ccxt**：相同的 `snake_case` 方法名
（`fetch_ohlcv`、`create_order` …）、相同的字段命名，并以类型化结构体承载原始交易所响应（保存于
`info` 字段，作为兜底通道）。

> 📗 English documentation: [README.md](./README.md).

## 特性

- **`Exchange` trait 提供的统一 REST API**：`snake_case` 方法名与字段名，代码可近乎逐行地从其他兼容 ccxt 的客户端移植。
- **109 个交易所适配器**：22 个精选（手写）覆盖完整接口面，以及 87 个由 ccxt 转译生成的交易所。
- **类型化、serde 容错的数据结构**：`Market`、`Ticker`、`OrderBook`、`OHLCV`、`Order`、`Trade`、`Position`、`Balance` …，并通过 `info` 保留原始响应以保证完整性。
- **精确十进制运算**：默认使用 `rust_decimal`，另提供 `Precise` 模块与 `decimal_to_precision` 辅助函数，语义对齐 ccxt 的 `Precise` / `decimalToPrecision`。
- **可选的同步封装**（`sync` feature）：无需自行管理异步运行时。
- **可选的实时 WebSocket 层**（`realtime` feature）：8 个 `watch_*` 通道。
- **预测市场支持**：Kalshi、Polymarket（EIP-712 订单签名）、Manifold（原生），以及 Limitless / Myriad / Opinion。
- **Apache-2.0** 许可；源自上游的解析逻辑保留其 MIT 声明（见 `NOTICE`）。

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

本库共提供 **109 个交易所** —— 22 个精选适配器提供完整覆盖，以及 87 个由 ccxt 转译生成的
交易所。每个交易所通过各自的 Cargo feature 启用（feature 名称即下表中所示的交易所 id）。

### 精选适配器（22 个）

这些为手写实现，覆盖完整方法面，包含签名与交易所特定的解析。

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
| `manifold`    | Manifold        | 原生适配器：markets / ticker / trades                                |

> **v1.0.3 新增（[ADR-0017](docs/adr/0017-hybrid-evolution-curated-generated-boundary.md)）**：
> 8 个适配器 —— `alpaca`、`aster`、`binanceus`、`gemini`、`hashkey`、`lighter`、`myokx`、`okxus` ——
> 由 generated 提升（promote）为 curated，获得完整 REST 覆盖（公开 + 私有/交易）。精选集现为 **22** 个。

| `alpaca`      | Alpaca          | REST 公开 + 私有（crypto 市场）                                     |
| `aster`       | Aster           | REST 公开 + 私有（DEX）                                             |
| `binanceus`   | Binance US      | REST 公开 + 私有                                                     |
| `gemini`      | Gemini          | REST 公开 + 私有                                                     |
| `hashkey`     | HashKey         | REST 公开 + 私有                                                     |
| `lighter`     | Lighter         | DEX：REST 公开 + 私有（EIP-712 链上订单）                           |
| `myokx`       | OKX             | REST 公开 + 私有                                                     |
| `okxus`       | OKX US          | REST 公开 + 私有                                                     |

### 由 ccxt 转译生成的交易所（87 个）

其余交易所共享同一统一 REST 面，通过各自的 Cargo feature 启用。它们以共用引擎覆盖常用 REST 方法
—— `fetch_markets`、`fetch_ticker`、`fetch_ohlcv`、`fetch_order_book`、`fetch_trades`、
`fetch_balance`、`fetch_orders`、`create_order`、`cancel_order` ……。需要交易所特定签名的方法保持
`NotSupported`。

| Feature            | Feature               | Feature             |
| ------------------ | --------------------- | ------------------- |
| `apex`             | `backpack`            | `bequant`           |
| `bigone`           | `binancecoinm`        | `binanceusdm`       |
| `bingx`            | `bit2c`               | `bitbank`           |
| `bitbns`           | `bitfinex`            | `bitflyer`          |
| `bithumb`          | `bitmex`              | `bitopro`           |
| `bitrue`           | `bitso`               | `bitstamp`          |
| `bitteam`          | `bittrade`            | `bitvavo`           |
| `blockchaincom`    | `blofin`              | `btcbox`            |
| `btcmarkets`       | `btcturk`             | `bullish`           |
| `bybiteu`          | `bydfi`               | `cex`               |
| `coinbaseexchange` | `coinbaseinternational` | `coincheck`         |
| `coinex`           | `coinmate`            | `coinone`           |
| `coinsph`          | `coinspot`            | `cryptocom`         |
| `cryptomus`        | `deepcoin`            | `delta`             |
| `deribit`          | `derive`              | `digifinex`         |
| `dydx`             | `exmo`                | `extended`          |
| `fmfwio`           | `foxbit`              | `gateeu`            |
| `grvt`             | `hibachi`             | `hitbtc`            |
| `hollaex`          | `independentreserve`  | `indodax`           |
| `krakenfutures`    | `kucoinfutures`       | `latoken`           |
| `lbank`            | `luno`                | `mercado`           |
| `modetrade`        | `mudrex`              | `nado`              |
| `ndax`             | `onetrading`          | `p2b`               |
| `pacifica`         | `paradex`             | `paymium`           |
| `phemex`           | `poloniex`            | `tokocrypto`        |
| `toobit`           | `upbit`               | `weex`              |
| `whitebit`         | `woo`                 | `woofipro`          |
| `xt`               | `zaif`                | `zebpay`            |
| `limitless`        | `myriad`              | `opinion`           |

三个长尾预测市场——**Limitless / Myriad / Opinion**——属于此生成集合，而 Kalshi / Polymarket /
Manifold 仍是上方的精选适配器。

## Feature 开关

| Feature           | 作用                                                                 |
| ----------------- | -------------------------------------------------------------------- |
| 按交易所          | 编译进对应适配器（如 `binance`、`kraken`、`polymarket` …）           |
| `realtime`        | 启用 WebSocket `watch_*` 方法（见下方 8 个通道）                      |
| `sync`            | 异步 API 的阻塞封装（`sync::BlockingExchange`）                      |
| `prediction`      | `kalshi` + `polymarket` + `manifold`（精选预测市场）                 |
| `full`            | 全部交易所（精选 + 生成式）+ `realtime`                                |
| _默认_            | `binance`、`okx`、`kalshi`、`polymarket`                            |

## 架构

适配器遵循 **HttpCore + 四接缝** 模型（ADR-0013）：

- **`HttpCore`** —— 一个与交易所无关的深层模块，负责 HTTP 请求骨架、市集缓存、客户端分页/过滤、安全字段提取，以及 `Precise` 运算。它还承载规范化的辅助函数 —— `iso8601_ms`、`parse_ohlcv_standard` —— 以及共享的「错误码 → `ErrorKind`」映射（`ERROR_CODE_MAP`，即 ADR-0013 的 `handle_errors` 接缝）。
- **`signing`** —— 一个共享的深层模块（[`src/signing.rs`](src/signing.rs)，对应 ADR-0013 的 `sign` 接缝），收口所有与交易所无关的 HMAC 原语（SHA-256/384/512，hex 与 base64 两种输出）、凭据抽取（`require_api_key` / `require_secret` / `require_passphrase`），以及无 panic 的请求头装配（`set_header`）。各适配器的 `private_request` 只需保留交易所专属的认证串拼接与请求头名集。
- **四接缝** —— 每个适配器只需填写：`describe`（端点路径/参数）、`sign`（签名算法）、`handle_errors`（错误码映射），以及字段映射（`parse` 覆写）。其余（生成式）交易所适配器只填 `describe` 接缝，零改动复用 `HttpCore`。

> **v1.0.4 新增**：一次无接口面、无交易所变更的整合发布。`sign` 接缝下沉为共享的 `src/signing.rs` 深层模块；`HttpCore` 吸纳了规范化的时间/OHLCV 辅助函数与共享错误码映射；`generic.rs` 中「生成式路径」的 `parse_*` 消费者被拆分为 `src/generic_parse.rs`，以隔离 ADR-0016 的契约锚点。精选适配器中净删约 330 行，行为与差分 fixture 均保持不变。

109 个交易所采用**混合进化模型**（[ADR-0017](docs/adr/0017-hybrid-evolution-curated-generated-boundary.md)）：**22 个精选（curated）**适配器为手写、覆盖完整交易面（团队维护、可最大化优化），**87 个生成式（generated）**适配器由 ccxt `describe()` 转译生成、仅覆盖公开面（由 ccxt 上游维护）。`提升（Promote）`是把任意长尾交易所从 generated 升级为 curated 的可复用操作，一旦需要完整交易即可采用。

预测市场的特有逻辑（结果上下文、合成 ticker、RSA-PSS / EIP-712 / ECDSA 签名）保留在适配器侧，不进入核心。

`types` 中的统一数据结构使用兼容 ccxt 的 `snake_case` 字段名，并通过 `info` 保留原始响应以保证完整性。

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

统一的 `Error` 封装了镜像 ccxt 异常树的 `ErrorKind`，携带上下文、可重试性，以及 `From` 转换，便于使用 `?` 进行符合人体工学的错误传播。交易所错误码现在通过共享的 `ERROR_CODE_MAP`（ADR-0013 的 `handle_errors` 接缝）被分类为更细粒度的 `ErrorKind`：例如 Binance `-1003` → `RateLimitExceeded`（可重试）、`-1121` → `BadSymbol`、`-2015` → `Authentication`；Bybit `10001` → `InvalidOrder`、`10003` → `Authentication`；HTX `invalid-symbol` → `BadSymbol`。未入表的错误码回退为 `ErrorKind::Exchange`，与原有行为一致。

## 测试与质量

- **契约测试** —— 统一方法面在构建期从 `Exchange` / `Realtime` trait 提取（见 `build.rs`）并断言完整性；这是发布门槛。
- **差分测试** —— 解析器与来自相同原始响应的真实 ccxt 输出逐字段比对（离线、CI 中确定性 fixtures）。
- `clippy -D warnings`、`rustfmt`、MSRV **1.85**（edition **2024**）、`cargo nextest`。

## 开发

从 ccxt 的 `describe()` 重新生成其余（生成式）交易所适配器。生成器读取各交易所的 `describe()`，
在 `src/adapters/generated/` 下产出按交易所划分的模块；共用运行时位于 `src/generic.rs`。

`gen_adapters.py` **不接受 `--ccxt` 参数**。它优先使用 pip 安装的 ccxt（固定 `ccxt==4.5.73`），
仅在没有 pip 包时回退到仓库本地的 `ccxt/python` 检出（离线方式）：

```bash
pip install "ccxt==4.5.73"

# 全量重新生成（全部约 87 个转译交易所）：
python3 scripts/gen_adapters.py

# 仅重新生成单个交易所：
python3 scripts/gen_adapters.py --only=<id>     # 例如 --only=binance
```

重新生成后请运行 `cargo fmt --all` 并提交生成层（`src/adapters/generated/`、`src/adapters/generated.rs`、
`Cargo.toml`）—— 否则 CI 的 `transpiler-fresh` 任务会失败。转译器与 Rust 引擎（`src/generic.rs`）的契约由
`python3 scripts/test_sync.py` 守护。

## 许可证

Apache-2.0。源自上游库的解析逻辑保留其 MIT 声明 —— 见 `NOTICE`。

---

📗 English documentation: [README.md](./README.md).
