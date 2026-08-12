# 实时/解析合一:watch_* 复用 REST parse_*(realtime 持有 REST 适配器实例)

架构评审候选 3(2026-08-12):实时适配器自建的 parse_* 与 REST 适配器并行漂移,同一概念的解析逻辑跨两个模块,修复一处漏一处。决策:WS 消息一律复用 REST 适配器的 parse_* —— 每个 realtime 适配器持有 `Arc<crate::adapters::X>` 实例(kalshi/polymarket 原本已持有,okx/bybit/binance/kraken 新增),dispatch 闭包捕获 Arc 并调用 `rest.parse_*(raw)`。前提是**字段形状兼容**:okx(字段名完全一致)、kalshi order、bybit ticker/order 已合一;形状不同的交易所按两种方式处理——(a) REST parse 加**纯加法 fallback**(WS 专属字段,如 bybit `time||timestamp||ts` 字符串/数字兼容、`highPrice24h||high24h`;差分测试证明 REST fixtures 不受影响),(b) 形状本质不同或 REST 无对应(bybit trade 单字母、binance miniTicker 短字段、kraken order descr string/object、polymarket 合成 ticker)保持本地独立。同步修正:okx WS order 时间戳校准为 cTime(REST 语义)、kalshi WS order 获得 order_type=limit、bybit WS symbol 统一为 BTC/USDT 格式、bybit order 状态补 PartiallyFilled→open(与 ccxt 一致,差分测试验证)。realtime 适配器新增 REST 实例后,亦可顺带复用 REST 的 HttpCore 能力(如 binance listenKey 认证),但本轮未扩展。**新增实时适配器一律按此模式:先比对 WS 与 REST 字段形状,能复用则复用(必要时给 REST parse 加 fallback),不能则保持独立并注明。**

- **Status**: accepted
