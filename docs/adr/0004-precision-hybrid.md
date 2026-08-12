# 数值精度:结构字段默认 rust_decimal,另公开 precise 模块按 ccxt 语义重实现

统一结构的价格/数量字段默认用 `rust_decimal`(96-bit 尾数、28 位有效数字,性能好,覆盖 99% 行情与交易场景);同时公开一个 `precise` 模块,按 ccxt `Precise` 的语义(bigint 尾数 + 指数、字符串输入输出、add/sub/mul/div/mod/abs/min/max/compare/reduce)重实现,服务于差分测试、资金逐位对账等极端精度场景。ccxt 的 `decimalToPrecision`(DECIMAL_PLACES / SIGNIFICANT_DIGITS / TICK_SIZE 三模式)也需复刻。否决"全用 bigdecimal"(性能代价无必要)与"仅 rust_decimal"(精度契约无法与 ccxt 完全对齐)。

- **Status**: accepted
