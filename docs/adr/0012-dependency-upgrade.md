# 依赖升级:6 个 0.x 大版本一次升到最新

2026-08 执行 `cargo upgrade`,6 个直接依赖因 0.x 大版本跳变(0.x 中 minor bump 即 breaking)被标 incompatible:reqwest 0.12→0.13.4、hmac 0.12→0.13、sha2 0.10→0.11、sha3 0.10→0.12、base64 0.22→0.23、num-bigint 0.4→0.5。逐项核对官方 changelog 后全部升级:实际代码改动仅两处——reqwest feature `rustls-tls` 改名 `rustls`(且 `query`/`form` 变可选 feature,本项目未使用无需开启),hmac 0.13 的 `new_from_slice` 从 `Mac` trait 移入 `KeyInit` trait(14 个文件 import 补 `KeyInit`,纯增量)。num-bigint 的 breaking 仅在 rand 集成(未用),base64 0.23 / sha2 0.11 / sha3 0.12 对本项目用法为 drop-in。验证:`cargo check --all-features` 通过,`cargo build --all-features` 零警告,`cargo test --all-features` 172 通过 0 失败(含 57 个与 ccxt fixtures 的差分测试,签名行为一致性有保障)。已知残留:锁文件仍含 base64 0.22.1(hyper-util/reqwest 0.13 与 libsecp256k1 传递引入)及 hmac 0.8.1 / sha2 0.9.9(libsecp256k1 0.7 引入),为独立版本图正常共存,消除需另行升级 libsecp256k1。权衡点:reqwest 0.13 默认密码学提供者改为 aws-lc-rs,交叉编译/低配 CI 编译成本可能高于 ring,运行时行为无影响。备选:部分保留(仅跳过 reqwest 0.13)可进一步降低编译成本,但会失去依赖统一的收益,未采纳。

- **Status**: accepted
