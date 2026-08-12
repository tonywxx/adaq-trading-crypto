# 差分测试基建:录制 fixtures 为主,CI 定期 live 差分兜底

行为等价验证以录制 fixtures 为主:用真实 ccxt 参考实现录制请求/响应 JSON 存档,离线回放做契约与行为断言,保证 CI 确定性;辅以 CI 定期 live 差分(对同一公共端点两边同时请求、逐字段比对统一结构输出)兜底录制内容过时。否决纯 live 差分(网络抖动导致 flaky,不适合作为常规门槛)。

- **Status**: accepted
