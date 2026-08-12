# 错误体系:ErrorKind 镜像 ccxt 异常树 + 统一 Error 包装

统一错误采用单一 `Error` 结构:`kind: ErrorKind`(封闭枚举,镜像 ccxt 异常树的 40+ 类别,是差分测试与适配器错误映射的比对面)、`context: ErrorContext`(exchange、method、url、http_status、http_error_code、raw_body、重试次数)与 `source` 错误链;`is_retryable()` 由 kind 推导、与重试循环同源;提供 `From<reqwest::Error>`(按 status 映射 DDoS/RateLimit/Timeout/NotAvailable)、`From<serde_json::Error>`(→ BadResponse)等转换。否决纯 thiserror 平铺 40 变体(每变体重复携带上下文、重试语义手写易漂移)与"拆分多个错误类型"(破坏单一 `Result<T, Error>` 契约)。该形态类似 `reqwest::Error` 内部 category 的设计。

- **Status**: accepted
