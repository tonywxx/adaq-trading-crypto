//! HMAC 签名深模块 —— ADR-0013 `sign` 接缝的共享实现。
//!
//! 各交易所适配器的 `private_request` 仍负责**交易所专属**部分:认证串拼接
//! (binance `query+&signature` vs okx `ts+method+path+body`)与请求头名集。
//! 本模块只收口与交易所无关的加密原语、凭据抽取与请求头装配,使"一处实现、
//! 全适配器可测"成立。
//!
//! 约定:`hmac(secret, msg)` —— secret 在前(归一了 binance 系 `(data, secret)` 的反向实参)。

use crate::Config;
use crate::error::{Error, ErrorKind, Result};
use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use sha2::{Sha256, Sha384, Sha512};

// ============================ 加密原语 ============================

/// HMAC-SHA256 → hex。
pub fn hmac_sha256_hex(secret: &str, msg: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(msg.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// HMAC-SHA256 → base64(okx/myokx/okxus/kucoin/bitget/htx/polymarket)。
pub fn hmac_sha256_b64(secret: &str, msg: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(msg.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

/// HMAC-SHA512 → hex(gate)。
pub fn hmac_sha512_hex(secret: &str, msg: &str) -> String {
    let mut mac = Hmac::<Sha512>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(msg.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// HMAC-SHA512 → base64(kraken)。
pub fn hmac_sha512_b64(secret: &str, msg: &str) -> String {
    let mut mac = Hmac::<Sha512>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(msg.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

/// HMAC-SHA384 → hex(gemini 部分端点)。
pub fn hmac_sha384_hex(secret: &str, msg: &str) -> String {
    let mut mac = Hmac::<Sha384>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(msg.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

// ============================ 凭据抽取 ============================

/// 取 `api_key`,缺失时返回带交易所名的 `Authentication` 错误。
pub fn require_api_key<'a>(cfg: &'a Config, exch: &str) -> Result<&'a str> {
    cfg.api_key.as_deref().ok_or_else(|| {
        Error::new(
            ErrorKind::Authentication,
            format!("{exch} api_key required"),
        )
    })
}

/// 取 `secret`,缺失时返回带交易所名的 `Authentication` 错误。
pub fn require_secret<'a>(cfg: &'a Config, exch: &str) -> Result<&'a str> {
    cfg.secret
        .as_deref()
        .ok_or_else(|| Error::new(ErrorKind::Authentication, format!("{exch} secret required")))
}

/// 取 `password`(作 passphrase,okx/kucoin),缺失时返回带交易所名的 `Authentication` 错误。
pub fn require_passphrase<'a>(cfg: &'a Config, exch: &str) -> Result<&'a str> {
    cfg.password.as_deref().ok_or_else(|| {
        Error::new(
            ErrorKind::Authentication,
            format!("{exch} passphrase required"),
        )
    })
}

// ============================ 请求头装配 ============================

/// 安全插入一个请求头。把 `HeaderName`/`HeaderValue` 构造错误转成 [`Error`],
/// 消除各适配器 `HeaderValue::from_str(...).unwrap()` 的潜在 panic。
///
/// 注意:`HeaderMap::insert` 对 `&str` 键仅接受 `'static` 字符串,故此处先把
/// `name` 构造为 [`HeaderName`](支持任意生命周期的字节),再插入。
pub fn set_header(map: &mut HeaderMap, name: &str, value: &str) -> Result<()> {
    let name = HeaderName::from_bytes(name.as_bytes()).map_err(|e| {
        Error::new(
            ErrorKind::BadRequest,
            format!("invalid header name {name}: {e}"),
        )
    })?;
    let v = HeaderValue::from_str(value).map_err(|e| {
        Error::new(
            ErrorKind::BadRequest,
            format!("invalid header value for {name}: {e}"),
        )
    })?;
    map.insert(name, v);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_hex_known_vector() {
        // binance 文档示例,secret 与 msg 均来自官方
        let sig = hmac_sha256_hex(
            "NhqPtmdSJYdKjVHjA7PZj4Mge3R5YNiP1e3UZjInClVN65XAbvqqM6A7H5fATj0j",
            "symbol=LTCBTC&side=BUY&type=LIMIT&timeInForce=GTC&quantity=1&price=0.1&recvWindow=5000&timestamp=1499827319559",
        );
        assert_eq!(
            sig,
            "c8db56825ae71d6d79447849e617115f4a920fa2acdcab2b053c4b2838bd6b71"
        );
    }

    #[test]
    fn sha256_b64_known_vector() {
        assert_eq!(
            hmac_sha256_b64(
                "okx-test-secret",
                "2024-01-01T00:00:00.000ZGET/users/self/verify"
            ),
            "Y3EnSWN5HK2xf//BKO+d0jjwaV9+I7+gaKi1pZDf764="
        );
    }

    #[test]
    fn sha512_hex_known_vector() {
        assert_eq!(
            hmac_sha512_hex("gate-test-secret", "GET\n/api/v4/spot/orders\n\n1700000000"),
            "4a36c950dea4200840f97d3aaac663364addac6f300ba168c21fb81c8add11346846e7a5a206dc9ad5a1fe9e758788d7cbcd21acc82c12ab8859df193d61059e"
        );
    }

    #[test]
    fn sha512_b64_known_vector() {
        assert_eq!(
            hmac_sha512_b64("kraken-test-secret", "method=GetWebSocketsToken"),
            "ZdDGcIAc3Y5QsWcxUuero7QdWbNC/ZIDJdMSlk+GPES5eqW1cn9tW+FWMsMV7ozcZh5qjQGS2mAmB0oRtDUK5w=="
        );
    }

    #[test]
    fn sha384_hex_known_vector() {
        assert_eq!(
            hmac_sha384_hex("gemini-test-secret", "eyJyZXF1ZXN0IjoiL2Nob2xkZXJzIn0="),
            "451b215e69cf5ca4a7ac06cea3762e6f63960269c9173390bde6aacf1f89e3fd8336119c9cc3254383fad3b094dbda47"
        );
    }
}
