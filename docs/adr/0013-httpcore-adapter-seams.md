# 适配器编写模型:HttpCore 深模块 + 四接缝(新增交易所一律按此模式)

深化统一接口(架构评审候选 1):把 13 个适配器逐字重复的公共机制(`query_string` ×13、`public_get` ×12、`load_markets` 缓存 ×13、`value_decimal`/`parse_level` ×10 等)下沉为一个交易所无关的深模块 **HttpCore**(HTTP 请求骨架、市集缓存、客户端分页过滤、safe 提取、Precise 精确数值),统一接口面保持 158 方法枚举契约不变(ADR-0001 契约测试面不受影响);每个适配器只填四个接缝——**describe**(端点路径/参数)、**sign**(签名算法)、**handle_errors**(错误码映射)、**字段映射**(parse 覆写)。此切法完全对齐 ccxt 基类/子类模型(ccxt `base/exchange.py` 的 `fetch2`/`load_markets`/`safe_*`/分页全在基类,`describe()`/`sign()`/`handle_errors`/`parse_*` 留在子类),是经过 ccxt 93 家交易所验证的边界。形态采用混合:核心以**可选组合**为主(适配器持有 HttpCore 而非强制 trait 默认链),只对普适链(如"公共 GET → 解析")提供 trait 默认实现,不强制 REST 形状——未来纯 WS/GraphQL 交易所可直接跳过核心、不被 158 个默认实现束缚;预测市场特有逻辑(`OutcomeCtx` 解析、合成 ticker、RSA-PSS/secp256k1 签名)全部留在适配器侧,核心保持交易所无关。验收标准:ADR-0005 的"从 ccxt 转译批量补齐适配器"评估落地时,转译器只填四缝、零改动复用 HttpCore——能转译即证明兼容性。被否决:强制 trait 默认实现链(158 方法默认干活,接口/契约测试面失焦,REST 形状被强加于所有交易所)。**后续新增任何交易所(手写或转译)都必须按此模式,避免二次优化。**

- **Status**: accepted
