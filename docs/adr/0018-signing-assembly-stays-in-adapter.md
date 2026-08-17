# 签名装配收口(架构评审候选 C)确认:预哈希拼接与头名留在适配器

2026-08-17 架构评审提出候选 C:把各 curated 适配器内联的**预哈希串拼接**(binance
`query+&signature`、okx `ts+method+path+body`、kraken `nonce+body→sha256` 前置 path 字节)
收进 `src/signing.rs`,使其成为按 `SignScheme` 枚举驱动的单接口深模块,适配器只选方案。

经 grilling 决策树评估,**确认不深化**——本决策是 ADR-0013 四接缝收口边界的既定结论,
此处仅做显式收口记录,避免未来架构评审重复提出同一候选。

- **Status**: accepted

## 背景与依据

ADR-0013 把统一接口切为四接缝(`describe` / `sign` / `handle_errors` / 字段映射),其中
`sign` 接缝的共享实现落在 `src/signing.rs`,但其范围被刻意限定为**与交易所无关的加密原语**:

- `hmac_sha256_hex` / `hmac_sha256_b64` / `hmac_sha512_hex` / `hmac_sha512_b64` /
  `hmac_sha384_hex` / `hmac_sha512_b64_bytes`
- `sign_ecdsa_recoverable`(EIP-712/secp256k1)、`sign_rsa_pss`(Kalshi)
- `require_api_key` / `require_secret` / `require_passphrase`(带交易所名的凭据抽取)
- `set_header`(请求头装配,消除 `.unwrap()` panic)

ADR-0013 的 2026-08-14 实施注记已明确:*"各适配器 `private_request` 仅保留真正的交易所差异——
认证串拼接(`query+&signature` vs `ts+method+path+body`)与请求头名集"*,并把 kraken/polymarket
的字节密钥 HMAC 记为"差异留接缝"例外。即**真正的共享面早已集中在 `signing.rs`**;候选 C 所指
的"浅"是 ADR 工作的结果,而非遗漏。

## Considered Options

- **(a) 重开 ADR-0013,把预哈希装配搬进 `signing.rs`(SignScheme 枚举驱动单接口)** — 否决。
  删除测试表明:删掉适配器装配后,复杂度不是被**集中**而是被**移动到** `signing.rs`,且会让
  加密模块反过来依赖 `query_string` / `pct_encode` / `nonce` 生成 / URL 构造,违背其
  "交易所无关加密"的单一职责,局部性反而变差。三处装配本质不同且与请求构造紧耦合:
  - binance:签名进 **URL `?signature=`**,头名 `X-MBX-APIKEY`;
  - okx:`ts+method+path+body` 进**头**,且 REST 与 WS 登录帧共用(`sign_str`);
  - kraken:`nonce+body→sha256` 前置 path 字节、字节密钥 `hmac_sha512_b64_bytes` 进**头 `API-Sign`**。
  无跨所可抽取的"共同预哈希"逻辑,搬移只产生一层薄转发。
- **(b) 保持 ADR-0013 边界(`signing.rs` = 加密原语,适配器 = 预哈希/头名装配)** — 采纳。
  共享加密原语已集中;剩余的请求耦合装配留在适配器是正确的接缝,契合 ccxt 基类/子类模型
  (`sign()` 在子类覆写)。

## Consequences

- **未来架构评审不再把"把签名装配收口进 `signing.rs`"列为深化候选**。若某评审判定
  `signing.rs` "浅",应理解为 ADR-0013 的刻意收口,而非待修缺陷。
- **新型签名「方案」(非装配)仍属 `signing.rs` 演进范围**:若新增交易所需要全新的签名
  *算法* 族(而非既有 HMAC/ECDSA/RSA-PSS 的另一种拼接),按 ADR-0013 候选④的路径扩展
  `generic::SignScheme` 枚举 + `sign_headers` 分支,属"方案枚举"而非"装配上提",与本条不冲突。
- **kraken / polymarket 字节密钥 HMAC** 继续作为 ADR-0013 的"差异留接缝"例外,保留各自内联实现。
- 无代码改动,无门禁影响。

## 关联

- ADR-0013(四接缝模型 + 2026-08-14 `signing.rs` 共享深模块实施注记,本决策的直接依据)
- ADR-0016(`SignScheme` 枚举的扩展边界:逐所定制签名属手写适配器职责)
