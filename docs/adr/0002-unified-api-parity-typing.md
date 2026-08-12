# 统一 API 沿用 ccxt 方法名与字段名,结构采用 typed struct

统一接口方法名与结构字段名完全沿用 ccxt 的 snake_case 命名(`fetch_ohlcv`、`create_order`、`set_leverage`、`taker_or_maker` 等),便于 AdaQ 用户从 ccxt 平迁、文档与社区资料可交叉引用。统一数据结构(Order/Market/Ticker/Position 等)用严格 typed struct + serde 建模:字段 `Option<T>` + `#[serde(default)]` 容错、`#[serde(flatten)]` 保留交易所特有字段、附 `info` 字段携带原始响应(延续 ccxt 惯例)。放弃"动态 Value 映射"(ccxt 式松散)与"Rust 化重命名"两条路线,换编译期安全与迁移成本最低。

- **Status**: accepted
