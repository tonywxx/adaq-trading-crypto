# 交付形态:单 crate + 按交易所 feature + MSRV 1.85

以单 crate 交付并发布 crates.io,`src/main.rs`(本地测试 CLI + 示例)发布时排除;feature 按交易所粒度(每交易所一个 feature)+ 组 feature(prediction / futures / full),realtime 独立 feature,默认开启最小可演示集;MSRV 锁定 1.85(与 edition 2024 对齐,以 `rust-version` 声明);CI 用 GitHub Actions + cargo nextest + clippy `-D warnings`;文档走 rustdoc + docs.rs。规模增长后再评估拆分为 workspace 多 crate。

- **Status**: accepted
