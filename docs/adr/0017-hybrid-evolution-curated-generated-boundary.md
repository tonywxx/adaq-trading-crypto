# 混合进化模型:curated(手写精品)与 generated(ccxt 转译)双轨边界 + Python 构建环境定位

AdaQ 的交易所覆盖采用**双轨混合进化模型**,而非"完全摆脱 ccxt 自养全部":

- **generated 轨(95 个长尾交易所)**:由 `scripts/gen_adapters.py` 从 ccxt 4.5.73 `describe()` 转译生成,仅覆盖公开行情面;维护责任在 ccxt 上游,团队近乎零成本,跟 ccxt 大版本 regen 即"自动进化"。
- **curated 轨(22 个手写精品)**:团队手写完整 `Exchange` trait 全量交易 API(行情 + 私有/交易/账户),四接缝按 ADR-0013(HttpCore + describe/sign/handle_errors/字段映射);维护责任在团队,但可最大化优化(延迟、私有端点、字段),并可借 ccxt `describe()` 作语义参考(MIT,见 `NOTICE`)。

本次将 8 个交易所(alpaca, aster, binanceus, gemini, hashkey, lighter, myokx, okxus)由 generated **提升(promote)** 为 curated——它们当前已存在于 `src/adapters/generated/`(ccxt 4.5.73 已覆盖),但 generated 只含公开面、无交易能力,故提升为 curated 以补齐完整交易 API。提升后 22 = 14 现有 curated + 8 新增 curated。

**Python 构建环境定位**:Python 仅用于 `gen_adapters.py`(regen generated 轨)与 `test_sync.py`(契约一致性守护),且仅在**偶尔 regen + CI** 时执行;本地 Rust 开发、新增/修改 curated 适配器**全程零 Python**。regen 频率低(跟 ccxt 大版本 / 加新所时),故 Python 摩擦为偶尔负担,采纳"保留 Python 仅用于 regen/CI"方案,不重写转译器到 Rust。

- **Status**: accepted

## Considered Options

- **(a) 完全摆脱 ccxt,自养全部 109 个交易所** — 否决。等于重造 ccxt,维护成本爆炸;长尾交易所 AdaQ 用户可能用但团队不主动维护,自养不经济。
- **(b) 维持这 8 个为 generated,不提升** — 否决。generated 只覆盖公开面、无交易 API(create_order/cancel_order/账户等),无法满足 AdaQ 在核心所上完整交易的需求。
- **(c) 混合模型 + 8 个 promote 为 curated(采纳)** — curated 负责核心所完整交易,generated 负责长尾广度由 ccxt 免费维护;Python 仅 regen/CI。

## Consequences

- **维护面从 ~109 降到 ~22 团队-owned**:95 generated 由 ccxt 上游免费维护;22 curated 由团队维护(14 已稳定,8 新增为未来主要成本)。新增 8 个均可借现有 generated 模块(公开面)+ ccxt `describe()` 作参考,难度可控。
- **双轨语义漂移风险**:curated 与 generated 行为可能分歧,由 `tests/contract.rs`(trait 覆盖契约)+ 差分测试守护;curated 自声明 `IMPLEMENTED` 方法面。
- **promote 是可复用操作**:任何长尾交易所一旦 AdaQ 需要完整交易,即可加入 `HANDWRITTEN` 跳过集 → 重跑生成器 → 删 generated 文件 → 在 `adapters/mod.rs` 注册 → 手写 curated。这是把交易所"升级"为精品的标准路径。
- **alpaca 取 crypto 通道**,不破 `adaq-trading-crypto` 的 crypto 域名假设。
- **lighter 完整交易 = EIP-712 钱包签名**(链上订单),复用 `polymarket` 的 EIP-712 模式;其公开行情面与 CEX 同构。
- **CI 影响**:`transpiler-fresh` 因 `HANDWRITTEN` 变更,重跑后 generated.rs 不再含这 8 个模块;`transpiler-sync`(`test_sync.py`,无 ccxt 依赖)不受影响。

## 8 个 curated 适配器实现清单

**提升(promote)通用机械步骤(8 个共用)**:

1. `scripts/gen_adapters.py` 的 `HANDWRITTEN` 集(当前 10 个)加入 8 个 id:`alpaca, aster, binanceus, gemini, hashkey, lighter, myokx, okxus`。
2. 跑 `python3 scripts/gen_adapters.py` → `generated.rs` 不再含这 8 个的 `pub mod`,且不再 emit `generated/<name>.rs`。
3. 删除孤儿文件:`src/adapters/generated/{alpaca,aster,binanceus,gemini,hashkey,lighter,myokx,okxus}.rs`(已由步骤 2 停止生成,但仍留在磁盘)。
4. `src/adapters/mod.rs` 为每个所加 `#[cfg(feature = "<name>")] pub mod <name>;` + `pub use <name>::{ID as <NAME>_ID, <Name>};`(对齐现有 14 个 curated 的注册写法)。
5. 新建 `src/adapters/<name>.rs`(curated):从现有 `generated/<name>.rs` 迁移公开面,补齐私有/交易面,实现完整 `Exchange` trait;按 ADR-0013 四接缝(HttpCore + describe/sign/handle_errors/字段映射)编写。
6. `Cargo.toml` feature 已有这 8 个(`alpaca/aster/binanceus/gemini/hashkey/lighter/myokx/okxus`),**保留不动**;`full` 聚合若含这些 feature 保持不变。
7. 门禁:`cargo fmt --all && cargo clippy --all-features -- -D warnings && cargo nextest run --all-features` 全绿;`python3 scripts/test_sync.py` 仍通过;本地模拟 `transpiler-fresh`(`gen_adapters.py` + `git diff --exit-code`)应干净。

**逐所签名策略与注意**(base URL / 是否 HMAC 以官方文档为准,以下为方案族):

| ID | 类型 | curated 签名方案(需补) | 复用模式 | 参考来源 | 提升注意 |
|---|---|---|---|---|---|
| `alpaca` | CEX(美股券商的 crypto 通道) | Alpaca v2:API Key ID + Secret 头(部分需 HMAC-SHA256,核对) | 独立(无 CEX 同款) | 现有 generated(公开面)+ Alpaca 官方文档 | crypto 通道;trading / market-data base URL 分离 |
| `aster` | CEX(Binance 派生) | HMAC-SHA256(apiKey + signature + timestamp) | `binance` | 现有 generated + ccxt `describe()` | base URL 核对(如 `https://api.asterdex.com`) |
| `binanceus` | CEX(美国) | HMAC-SHA256(同 binance) | `binance` | 现有 generated + ccxt `describe()` | base `https://api.binance.us` |
| `gemini` | CEX | HMAC-SHA384(base64 payload)+ `X-GEMINI-*` 头 | 独立(异于 binance) | 现有 generated + ccxt `describe()` | 签名算法不同于 binance,需独立实现 |
| `hashkey` | CEX(香港) | HMAC-SHA256(apiKey + secret) | `binance`-ish | 现有 generated + ccxt `describe()` | base `https://api.hashkey.com` |
| `lighter` | DEX(永续) | EIP-712 钱包签名(链上订单) | `polymarket` | 现有 generated(公开面)+ polymarket EIP-712 | 公开面同 CEX;交易需钱包签名,复用 polymarket 模式 |
| `myokx` | CEX(OKX 实体变体) | HMAC-SHA256(apiKey + passphrase + timestamp + signature) | `okx` | 现有 generated + ccxt `describe()` | base URL / 实体端点核对 |
| `okxus` | CEX(OKX 美国) | 同 `myokx` | `okx` | 现有 generated + ccxt `describe()` | base URL / 实体端点核对 |

**备注**:`binanceusdm`(币安美国合约)仍留 generated,不在本次提升范围。`manifold` 为原生(curated)且不来自 ccxt,不受影响。
