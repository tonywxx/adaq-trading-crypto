# 适配器范围:精选集优先 + 框架即插即用,编写模型采用手写为主、转译为后补

v1 不一次性实现 ccxt 全部 103 个适配器,而是:精选加密主流(binance、okx、coinbase、bybit、bitget、kraken、gate、mexc、htx、kucoin 等)+ 预测市场(kalshi、polymarket,来自 ccxt 的 `PredictionExchange` 抽象)+ manifold 作为 ccxt 之外的新增适配器;适配器框架预留统一扩展点,其余交易所按需增量补齐。编写模型:核心 6-8 个交易所手写精做(trait + 宏削减 `safe_*`/`parse_*` 样板),跑通范式后再评估"从 ccxt 单源转译生成 Rust 适配器"作为批量补齐手段。一次性全量(数千人日机械劳动、多数交易所与 AdaQ 无关)与纯转译路线(工程量巨大)均被否决。

- **Status**: accepted
