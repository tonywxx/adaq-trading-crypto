//! 增量订单簿引擎(ADR-0011):core 共享的快照 + diff 合并、limit 修剪。
//!
//! 各交易所 WS 适配器只喂增量:[`OrderBookStore::apply_binance_delta`] 处理
//! binance `@depth` 局部深度(U/u 序列对账),[`OrderBookStore::apply_polymarket`]
//! 处理 polymarket `price_change`(size 0 删除档位);[`OrderBookStore::snapshot`]
//! 输出统一 [`OrderBook`](crate::types::OrderBook)(bids 降序 / asks 升序)。

use std::collections::BTreeMap;

use rust_decimal::Decimal;

use crate::types::{Level, OrderBook};

/// 价格档位的增量操作。
#[derive(Debug, Clone)]
pub struct PriceChange {
    pub price: Decimal,
    /// `0` 表示删除该档位。
    pub size: Decimal,
}

/// 增量订单簿存储。
#[derive(Debug, Default)]
pub struct OrderBookStore {
    /// bids 以**升序**存储(取快照时反转,避免每次插入排序)。
    bids: BTreeMap<Decimal, Decimal>,
    /// asks 以升序存储。
    asks: BTreeMap<Decimal, Decimal>,
    /// 保留档位数(0 = 不限制)。
    limit: usize,
    /// binance:最后一次应用的末位更新 id。
    pub last_update_id: Option<u64>,
}

impl OrderBookStore {
    pub fn new(limit: usize) -> Self {
        Self {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            limit,
            last_update_id: None,
        }
    }

    /// 快照重置(ccxt `orderbook.reset`;用于 REST 初始快照 / polymarket book 事件)。
    pub fn reset(&mut self, bids: &[(Decimal, Decimal)], asks: &[(Decimal, Decimal)]) {
        self.bids.clear();
        self.asks.clear();
        for (p, s) in bids {
            if !s.is_zero() {
                self.bids.insert(*p, *s);
            }
        }
        for (p, s) in asks {
            if !s.is_zero() {
                self.asks.insert(*p, *s);
            }
        }
        self.trim();
    }

    /// 应用 binance `@depth` diff(`b`/`a` 为 `[price, qty]` 数组,qty=0 删除档位)。
    ///
    /// 返回 `Ok(true)` 表示应用成功;`Ok(false)` 表示该增量应丢弃
    /// (快照前到达、或重复/乱序,由 U/u 序列对账判定)。
    pub fn apply_binance_delta(
        &mut self,
        u: u64,
        bids: &[(Decimal, Decimal)],
        asks: &[(Decimal, Decimal)],
    ) -> bool {
        match self.last_update_id {
            None => false, // 快照前到达的增量,丢弃(ccxt 先 reset 再重放缓存)
            Some(last) if u <= last => false,
            Some(_) => {
                for (p, s) in bids {
                    OrderBookStore::apply_side(&mut self.bids, *p, *s);
                }
                for (p, s) in asks {
                    OrderBookStore::apply_side(&mut self.asks, *p, *s);
                }
                self.last_update_id = Some(u);
                self.trim();
                true
            }
        }
    }

    /// 应用 polymarket `price_change` 增量(size 0 删除)。
    pub fn apply_polymarket(&mut self, changes: &[PriceChange], side: &str) {
        let map = if side.eq_ignore_ascii_case("BUY") {
            &mut self.bids
        } else {
            &mut self.asks
        };
        for c in changes {
            if c.size.is_zero() {
                map.remove(&c.price);
            } else {
                map.insert(c.price, c.size);
            }
        }
        self.trim();
    }

    fn apply_side(map: &mut BTreeMap<Decimal, Decimal>, price: Decimal, size: Decimal) {
        if size.is_zero() {
            map.remove(&price);
        } else {
            map.insert(price, size);
        }
    }

    /// limit 修剪:保留最高 `limit` 档 bids、最低 `limit` 档 asks。
    fn trim(&mut self) {
        if self.limit == 0 {
            return;
        }
        // bids 升序存储 → 保留最后 limit 个(最高价);asks 升序 → 保留前 limit 个
        let bids_keep = self.bids.len().saturating_sub(self.limit);
        for _ in 0..bids_keep {
            if let Some(k) = self.bids.keys().next().copied() {
                self.bids.remove(&k);
            }
        }
        let asks_keep = self.asks.len().saturating_sub(self.limit);
        for _ in 0..asks_keep {
            if let Some(k) = self.asks.keys().next_back().copied() {
                self.asks.remove(&k);
            }
        }
    }

    /// 输出统一 OrderBook(bids 降序、asks 升序)。
    pub fn snapshot(
        &self,
        symbol: &str,
        timestamp: Option<i64>,
        nonce: Option<i64>,
        info: serde_json::Value,
    ) -> OrderBook {
        let bids: Vec<Level> = self
            .bids
            .iter()
            .rev()
            .map(|(p, s)| Level {
                price: Some(*p),
                amount: Some(*s),
            })
            .collect();
        let asks: Vec<Level> = self
            .asks
            .iter()
            .map(|(p, s)| Level {
                price: Some(*p),
                amount: Some(*s),
            })
            .collect();
        OrderBook {
            symbol: symbol.to_string(),
            bids,
            asks,
            timestamp,
            nonce: nonce.or(self.last_update_id.map(|u| u as i64)),
            info,
            ..OrderBook::default()
        }
    }

    /// 当前 bids 档位数(测试用)。
    pub fn bid_len(&self) -> usize {
        self.bids.len()
    }

    /// 当前 asks 档位数(测试用)。
    pub fn ask_len(&self) -> usize {
        self.asks.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Decimal {
        s.parse().unwrap()
    }

    #[test]
    fn binance_delta_drops_pre_snapshot() {
        let mut store = OrderBookStore::new(0);
        // 快照前到达 → 丢弃
        assert!(!store.apply_binance_delta(5, &[], &[(d("0.5"), d("10"))]));
        assert_eq!(store.ask_len(), 0);
        // 快照
        store.reset(&[(d("0.4"), d("20"))], &[(d("0.5"), d("10"))]);
        store.last_update_id = Some(10);
        // u <= last → 丢弃
        assert!(!store.apply_binance_delta(10, &[], &[]));
        // 正常增量
        assert!(store.apply_binance_delta(11, &[(d("0.4"), d("25"))], &[(d("0.6"), d("7"))]));
        let snap = store.snapshot("BTC/USDT", None, None, serde_json::Value::Null);
        assert_eq!(snap.bids[0].price, Some(d("0.4")));
        assert_eq!(snap.bids[0].amount, Some(d("25")));
        assert_eq!(snap.asks.len(), 2);
    }

    #[test]
    fn zero_size_removes_level() {
        let mut store = OrderBookStore::new(0);
        store.reset(&[(d("0.4"), d("20")), (d("0.3"), d("5"))], &[]);
        store.last_update_id = Some(0);
        assert!(store.apply_binance_delta(1, &[(d("0.4"), d("0"))], &[]));
        assert_eq!(store.bid_len(), 1);
        assert_eq!(store.bids.get(&d("0.4")), None);
    }

    #[test]
    fn limit_trims_far_levels() {
        let mut store = OrderBookStore::new(2);
        store.reset(
            &[(d("0.1"), d("1")), (d("0.2"), d("1")), (d("0.3"), d("1"))],
            &[(d("0.4"), d("1")), (d("0.5"), d("1")), (d("0.6"), d("1"))],
        );
        assert_eq!(store.bid_len(), 2);
        assert_eq!(store.ask_len(), 2);
        let snap = store.snapshot("X", None, None, serde_json::Value::Null);
        // 保留最高 2 档 bids(0.3, 0.2)与最低 2 档 asks(0.4, 0.5)
        assert_eq!(snap.bids[0].price, Some(d("0.3")));
        assert_eq!(snap.bids[1].price, Some(d("0.2")));
        assert_eq!(snap.asks[0].price, Some(d("0.4")));
        assert_eq!(snap.asks[1].price, Some(d("0.5")));
    }

    #[test]
    fn polymarket_change_merge() {
        let mut store = OrderBookStore::new(0);
        store.reset(&[(d("0.5"), d("100"))], &[(d("0.6"), d("200"))]);
        store.apply_polymarket(
            &[
                PriceChange {
                    price: d("0.5"),
                    size: d("0"),
                },
                PriceChange {
                    price: d("0.49"),
                    size: d("50"),
                },
            ],
            "BUY",
        );
        assert_eq!(store.bid_len(), 1);
        let snap = store.snapshot("X:YES", None, None, serde_json::Value::Null);
        assert_eq!(snap.bids[0].price, Some(d("0.49")));
        assert_eq!(snap.bids[0].amount, Some(d("50")));
    }
}
