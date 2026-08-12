# v1 即包含实时(WebSocket)通道

实时通道纳入 v1 范围(而非后置):在统一接口中提供 watch_* 系列方法与 WS 传输层,与 REST 共用适配器解析逻辑。trait 按"REST + realtime 两个平面"设计,每个适配器可独立声明实时能力。这是对"v1 仅 REST"的偏离——量化平台的实时订阅(行情/订单/持仓推送)是核心能力,后置会导致 trait 与订阅管理模型二次返工。ccxt.pro 约 40 个 watch_* 方法的具体覆盖范围、OrderBook 增量引擎、sync 包装与 WS 的关系,见后续 ADR 与设计讨论。

- **Status**: accepted
