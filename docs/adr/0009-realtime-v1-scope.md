# 实时通道 v1 范围:核心 8 频道 + 共享 OrderBook 增量引擎 + sync 仅 REST

watch_* 以核心 8 频道起步(watch_ticker / watch_order_book / watch_trades / watch_ohlcv / watch_balance / watch_orders / watch_my_trades / watch_positions),对核心交易所实现,其余频道随适配器增量补齐;core 提供共享 OrderBook store(diff 合并、depth 聚合、limit 修剪),适配器只负责喂增量,不各自维护;sync 阻塞包装仅覆盖 REST,WS 只在 async 面提供(避免 runtime 生命周期管理)。否决"40 频道一次全上"与"仅全量快照、不做 diff 合并"(多数交易所默认 diff 流,不做增量引擎订阅即残废)。

- **Status**: accepted
