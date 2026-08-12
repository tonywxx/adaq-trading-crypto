# CONTEXT.md — AdaQ Unified Trading Interface (Rust)

本项目词汇表(glossary)。只收录领域术语,不收录实现细节。随 `/grill-with-docs` 会话逐条敲定。

## 语言(Language)

**统一交易接口 (Unified Trading Interface)**:
本项目交付物:一套与交易所无关的交易/行情 API,覆盖加密市场与预测市场。
_Avoid_: 交易库、SDK

**交易所适配器 (Exchange Adapter)**:
实现统一接口、对接单一交易所的具体实现组件,可独立声明 REST 与实时能力。
_Avoid_: 交易所封装、connector

**统一市场数据结构 (Unified Market Data Structure)**:
跨交易所一致的行情与交易数据结构(Market / Ticker / OrderBook / OHLCV / Order / Trade / Position / Balance 等),字段与 ccxt 对齐。

**预测市场 (Prediction Market)**:
对事件结果进行交易的市集,如 Kalshi、Polymarket。
_Avoid_: 博彩市场

**实时接口 (Realtime / WS)**:
基于 WebSocket 的行情/订单/持仓推送通道,以 watch_* 方法对外,对应 ccxt.pro。
_Avoid_: 长连接、流

**精确数值 (Precise)**:
交易价格与数量使用的精确十进制运算(避免二进制浮点误差),语义对齐 ccxt `Precise`。
_Avoid_: 浮点、decimal 混称

**契约测试 (Contract Test)**:
验证统一 API 方法面与结构字段面完整性的测试形态(发布门槛)。

**差分测试 (Differential Test)**:
与真实 ccxt 参考实现同端点运行、逐字段比对统一结构输出的测试形态(质量手段)。

## 领域对象

**订单 (Order)**:一次下单请求在交易所侧的表示(id、clientOrderId、symbol、side、type、price、amount、filled、remaining、cost、status、fee 等)。
**成交 (Trade)**:一笔已执行的交易记录。
**仓位 (Position)**:合约/杠杆持仓(id、side、contracts、notional、leverage、unrealizedPnl、liquidationPrice 等)。
**资金 (Balance)**:账户可用/占用/总额(free、used、total、debt)。
**行情快照 (Ticker)**:最新行情摘要(high/low/bid/ask/last/vwap/percentage 等)。
**订单簿 (OrderBook)**:买卖盘(bids/asks 按价聚合)。
**OHLCV**:K 线(时间、开、高、低、收、量)。

## 待定术语(讨论中,暂不写入定义)

- 暂无。全部架构级决策已敲定(含 ADR-0016 将"从 ccxt 转译批量补齐适配器"由未来评估项落地为 describe 驱动引擎 + 代码生成器)。

## 决策记录

| ADR | 状态 | 摘要 |
|---|---|---|
| 0001 | accepted | 完整性 = 契约测试(发布门槛)+ 差分测试(质量手段)双轨 |
| 0002 | accepted | 沿用 ccxt snake_case 命名;typed struct + serde 容错 + info 原始响应 |
| 0003 | accepted | async-first(tokio),feature 提供 sync 阻塞包装 |
| 0004 | accepted | 数值:rust_decimal 默认 + public precise 模块复刻 ccxt Precise 语义 |
| 0005 | accepted | 适配器精选集优先 + 框架扩展点;手写为主、转译为后补 |
| 0006 | accepted | v1 即含实时(WS)通道,watch_* 与 REST 共用解析逻辑 |
| 0007 | accepted | 错误体系:ErrorKind 镜像 ccxt 异常树 + 统一 Error 包装(上下文/可重试/From 转换) |
| 0008 | accepted | License:Apache-2.0 + NOTICE 声明 ccxt(MIT)归属 |
| 0009 | accepted | 实时 v1 范围:核心 8 频道 + 共享 OrderBook 增量引擎 + sync 仅 REST |
| 0010 | accepted | 差分测试:录制 fixtures 为主,CI 定期 live 差分兜底 |
| 0011 | accepted | 交付:单 crate + 按交易所 feature + MSRV 1.85 + nextest/clippy/docs.rs |
| 0012 | accepted | 依赖升级:6 个 0.x 大版本一次升到最新(ADR-0012) |
| 0013 | accepted | 适配器模型:HttpCore 深模块 + 四接缝(describe/sign/handle_errors/字段映射);新增交易所一律按此模式,转译只填接缝 |
| 0014 | accepted | 实时层收口:最小收口 + 测试先行(离线重放)+ 心跳/重连进共享层;不做完整 WsHub 框架;先收口后解析合一 |
| 0015 | accepted | 解析合一:watch_* 复用 REST parse_*(realtime 持有 REST 适配器实例);形状兼容则加纯加法 fallback,不兼容则保持独立 |
| 0016 | accepted | 转译补齐落地:describe 驱动通用引擎(generic.rs)+ 代码生成器(scripts/gen_adapters.py),实现 ADR-0005 后补路径与 ADR-0013 四接缝批量填 describe;覆盖全部 108 个 ccxt 唯一交易所/预测市场(103 sync 含 89 CEX + 14 DEX,及 7 prediction 命名空间,binance/hyperliquid 双列)+ 1 原生 Manifold,合计 109 适配器,transpiler 现额外覆盖 ccxt.prediction 命名空间(limitless/myriad/opinion) |
