//! HttpCore 深模块(ADR-0013):交易所无关的 HTTP 骨架、市集缓存与 safe 提取。
//!
//! 每个交易所适配器持有 [`HttpCore`],只填四个接缝(describe 路径/参数、
//! sign 签名、handle_errors 错误码、字段映射 parse 覆写),不再各自实现
//! 请求骨架、市集缓存与数值解析(原 13 份 `query_string`/`public_get`/
//! `load_markets`/`value_decimal`/`parse_level` 副本的集中处)。

use std::future::Future;
use std::sync::Mutex;

use reqwest::header::HeaderMap;
use serde_json::Value;

use crate::client::Client;
use crate::error::Result;
use crate::exchange::{Config, Params};
use crate::types::{Level, Markets};

/// 交易所无关核心:HTTP 客户端 + 市集缓存。
pub struct HttpCore {
    client: Client,
    base_url: String,
    markets: Mutex<Option<Markets>>,
}

impl HttpCore {
    /// 构造核心。
    ///
    /// `base_url` 为交易所 REST 根地址;`default_rate_limit_ms` 为该交易所
    /// 默认限速,`config.rate_limit_ms` 显式设置时优先。
    pub fn new(config: &Config, base_url: &str, default_rate_limit_ms: u64) -> Result<Self> {
        let rate_limit_ms = if config.rate_limit_ms > 0 {
            config.rate_limit_ms
        } else {
            default_rate_limit_ms
        };
        let client = Client::new(
            config.timeout_ms,
            config.max_retries,
            config.proxy.as_deref(),
            rate_limit_ms,
            config.enable_rate_limit,
        )?;
        Ok(Self {
            client,
            base_url: base_url.to_string(),
            markets: Mutex::new(None),
        })
    }

    /// 发起请求。`path` 为端点路径,GET 可含 `?query`(与签名 requestPath 一致)。
    pub async fn request(
        &self,
        method: &str,
        path: &str,
        headers: &HeaderMap,
        body: Option<Value>,
    ) -> Result<Value> {
        self.request_url(method, &format!("{}{}", self.base_url, path), headers, body)
            .await
    }

    /// 用完整 URL 发起请求(多 base 交易所,如 polymarket 的
    /// data-api/gamma-api/clob,不走 `base_url` 前缀)。
    pub async fn request_url(
        &self,
        method: &str,
        url: &str,
        headers: &HeaderMap,
        body: Option<Value>,
    ) -> Result<Value> {
        self.client.request(method, url, headers, body).await
    }

    /// 公共 GET(自动拼 query)。
    pub async fn public_get(&self, path: &str, params: &Params) -> Result<Value> {
        let url = format!("{}{}", path, query_string(params));
        self.request("GET", &url, &HeaderMap::new(), None).await
    }

    /// 市集缓存:未加载时调用 `fetch` 拉取并缓存,已加载直接返回。
    ///
    /// `fetch` 由适配器提供(市集解析是字段映射接缝的一部分),核心只负责
    /// 缓存读写——原各适配器 `markets: Mutex<Option<Markets>>` + 缓存判断
    /// 的副本在此收口。
    pub async fn load_markets<F, Fut>(&self, fetch: F) -> Result<()>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Markets>>,
    {
        if self.markets.lock().unwrap().is_some() {
            return Ok(());
        }
        let markets = fetch().await?;
        *self.markets.lock().unwrap() = Some(markets);
        Ok(())
    }

    /// 当前市集缓存快照(未加载时为空)。
    pub fn markets_snapshot(&self) -> Markets {
        self.markets.lock().unwrap().clone().unwrap_or_default()
    }
}

/// 兼容字符串/数字的 Decimal 解析(原各适配器 `value_decimal` 副本)。
pub fn value_decimal(v: &Value) -> Option<rust_decimal::Decimal> {
    match v {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.to_string().parse().ok(),
        _ => None,
    }
}

/// `Option<&Value>` → Decimal(原各适配器 `dec` 副本)。
pub fn dec(v: Option<&Value>) -> Option<rust_decimal::Decimal> {
    v.and_then(value_decimal)
}

/// `[price, amount, ...]` → Level(原各适配器 `parse_level` 副本)。
pub fn parse_level(raw: &Value) -> Level {
    let arr = raw.as_array();
    Level {
        price: arr.and_then(|a| a.first()).and_then(value_decimal),
        amount: arr.and_then(|a| a.get(1)).and_then(value_decimal),
    }
}

/// Params → `?k=v&...`(原各适配器 `query_string` 副本)。
pub fn query_string(params: &Params) -> String {
    if params.is_empty() {
        return String::new();
    }
    let pairs: Vec<String> = params
        .iter()
        .map(|(k, v)| {
            let val = match v {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                other => other.to_string(),
            };
            format!("{}={}", pct_encode(k), pct_encode(&val))
        })
        .collect();
    format!("?{}", pairs.join("&"))
}

/// RFC 3986 百分号编码(原各适配器 `pct_encode` 副本)。
pub fn pct_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 当前 UTC 毫秒精度时间戳(签名头用,原 `iso8601_now` 副本)。
pub fn iso8601_now() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

/// 毫秒时间戳 → RFC3339(原 `iso8601` 副本)。
pub fn iso8601(ms: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn value_decimal_parses_string_and_number() {
        assert_eq!(
            value_decimal(&json!("0.0015")),
            Some("0.0015".parse().unwrap())
        );
        assert_eq!(value_decimal(&json!(100)), Some("100".parse().unwrap()));
        assert_eq!(value_decimal(&Value::Null), None);
    }

    #[test]
    fn query_string_encodes_pairs() {
        let mut p = Params::new();
        p.insert("instType".into(), json!("SPOT"));
        p.insert("bar".into(), json!("1H"));
        let qs = query_string(&p);
        assert!(qs.starts_with('?'));
        assert!(qs.contains("instType=SPOT"));
        assert!(qs.contains("bar=1H"));
    }

    #[test]
    fn query_string_empty_when_no_params() {
        assert_eq!(query_string(&Params::new()), "");
    }

    #[test]
    fn parse_level_takes_first_two_elements() {
        let l = parse_level(&json!(["100", "1", "0", "2"]));
        assert_eq!(l.price, Some("100".parse().unwrap()));
        assert_eq!(l.amount, Some("1".parse().unwrap()));
    }

    #[test]
    fn pct_encode_reserves_unreserved_chars() {
        assert_eq!(pct_encode("BTC/USDT"), "BTC%2FUSDT");
        assert_eq!(pct_encode("a-b_c.d~"), "a-b_c.d~");
    }
}
