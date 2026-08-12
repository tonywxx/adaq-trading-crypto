# async-first API,另提供 sync 阻塞包装(feature gate)

统一接口以 tokio 异步为第一形态(AdaQ 是 Rust 量化平台,基本必然处于 tokio 生态);同时通过 feature gate 提供同步阻塞包装,使本地 `main` 冒烟测试、示例脚本、教学场景免于手动维护 runtime。代价是双 API 表面(同步/异步各一套入口)需要共享实现、避免逻辑复制。纯同步路线(ccxt Python 的 requests 风格)被否决:不符合 Rust 生态惯例,且量化平台集成多为异步。

- **Status**: accepted
