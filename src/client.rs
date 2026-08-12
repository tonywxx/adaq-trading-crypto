//! 适配器共用的 HTTP 客户端:Transport(超时/代理/重试)+ 限速。
//!
//! 每个交易所适配器持有一个 [`Client`],在统一方法实现中调用
//! [`Client::request`] 完成带限速、带重试的 HTTP 往返。

use reqwest::header::HeaderMap;
use serde_json::Value;

use crate::error::Result;
use crate::throttle::{ThrottleMode, Throttler};
use crate::transport::Transport;

/// 适配器 HTTP 客户端。
pub struct Client {
    transport: Transport,
    throttler: Throttler,
    enable_rate_limit: bool,
}

impl Client {
    /// 构造客户端。
    ///
    /// `rate_limit_ms` 为单次请求最小间隔(毫秒),如 binance 为 50;
    /// 仅在 `enable_rate_limit` 时启用限速。
    pub fn new(
        timeout_ms: u64,
        max_retries: u32,
        proxy: Option<&str>,
        rate_limit_ms: u64,
        enable_rate_limit: bool,
    ) -> Result<Self> {
        let transport = Transport::new(timeout_ms, max_retries, proxy)?;
        // 令牌桶:容量=1 秒突发量,补速=每秒可发请求数
        let refill_rate = if rate_limit_ms > 0 {
            1000.0 / rate_limit_ms as f64
        } else {
            // 未配置限速:构造一个几乎不限的桶,仅当 enable 时才被调用
            1e6
        };
        let throttler = Throttler::new(ThrottleMode::LeakyBucket {
            capacity: refill_rate,
            refill_rate,
        });
        Ok(Self {
            transport,
            throttler,
            enable_rate_limit,
        })
    }

    /// 请求 JSON 响应(带限速与重试)。
    pub async fn request(
        &self,
        method: &str,
        url: &str,
        headers: &HeaderMap,
        body: Option<Value>,
    ) -> Result<Value> {
        if self.enable_rate_limit {
            self.throttler.throttle(1).await;
        }
        self.transport.fetch_json(method, url, headers, body).await
    }
}
