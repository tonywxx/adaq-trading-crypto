#![allow(dead_code)]

//! 差分测试公共助手(ADR-0010):fixture 加载与统一结构转换。

use std::path::{Path, PathBuf};
use std::str::FromStr;

use rust_decimal::Decimal;
use serde_json::Value;

/// 录制 fixtures 根目录。
pub fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("recorded")
}

/// 加载某交易所某方法的录制 fixture。
pub fn load_recorded(exchange: &str, method: &str) -> Value {
    let path = fixture_root().join(exchange).join(format!("{method}.json"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("load fixture {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse fixture {}: {e}", path.display()))
}

/// 取 ccxt 统一输出部分。
pub fn normalized(v: &Value) -> &Value {
    v.get("normalized").expect("fixture missing normalized")
}

/// 加载原始响应 fixture(`tests/fixtures/raw/<exchange>/<method>.json`)。
pub fn load_raw(exchange: &str, method: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("raw")
        .join(exchange)
        .join(format!("{method}.json"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("load raw fixture {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("parse raw fixture {}: {e}", path.display()))
}

/// 加载 ccxt 解析基准(`tests/fixtures/ccxt_parsed/<exchange>/<method>.json`)。
pub fn load_ccxt_parsed(exchange: &str, method: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("ccxt_parsed")
        .join(exchange)
        .join(format!("{method}.json"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("load ccxt_parsed fixture {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("parse ccxt_parsed fixture {}: {e}", path.display()))
}

/// 取 ccxt 解析结果的 `parsed` 部分。
pub fn parsed(v: &Value) -> &Value {
    v.get("parsed").expect("fixture missing parsed")
}

/// 兼容数字/字符串/null 的 Decimal 解析(ccxt 输出为 JSON number)。
pub fn value_decimal(v: &Value) -> Option<Decimal> {
    match v {
        Value::String(s) => s.trim().parse().ok(),
        Value::Number(n) => Decimal::from_str(&n.to_string()).ok(),
        _ => None,
    }
}

/// 兼容整数/浮点/字符串的时间戳解析。
pub fn value_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// 把 ccxt 的 OHLCV 数组 `[ts, o, h, l, c, v]` 转成统一结构。
pub fn parse_ohlcv_row(row: &Value) -> adaq_trading_crypto::OHLCV {
    let arr = row.as_array().expect("ohlcv row is array");
    assert!(arr.len() >= 6, "ohlcv row has >=6 elements");
    adaq_trading_crypto::OHLCV {
        timestamp: value_i64(&arr[0]),
        open: value_decimal(&arr[1]),
        high: value_decimal(&arr[2]),
        low: value_decimal(&arr[3]),
        close: value_decimal(&arr[4]),
        volume: value_decimal(&arr[5]),
    }
}

/// 把 ccxt 的档位数组 `[price, amount]` 转成统一结构。
pub fn parse_level(level: &Value) -> adaq_trading_crypto::Level {
    let arr = level.as_array().expect("level is array");
    assert!(arr.len() >= 2, "level has 2 elements");
    adaq_trading_crypto::Level {
        price: value_decimal(&arr[0]),
        amount: value_decimal(&arr[1]),
    }
}
