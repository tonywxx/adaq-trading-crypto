//! 契约测试(ADR-0001):统一方法面基线。
//!
//! 方法面清单由 build.rs 从 trait 自动生成;本文件验证清单的完整性、
//! 唯一性与命名规范。M2 起,适配器实现将通过清单声明 `has` 能力,
//! 并在此处增加「对每个声明的方法调用并断言 NotSupported 或合法结果」
//! 的契约运行器。

use std::collections::HashSet;

use adaq_trading_crypto::contract_gen::ADAPTER_PAIRS;
use adaq_trading_crypto::methods::{all_methods, rest_methods, ws_methods};

/// 方法名允许的动词前缀(与 ccxt 命名约定一致)。
const REST_PREFIXES: &[&str] = &[
    "fetch_", "create_", "edit_", "cancel_", "close_", "set_", "add_", "reduce_", "borrow_",
    "repay_", "transfer", "sign_in", "withdraw",
];

#[test]
fn manifest_non_empty() {
    assert!(
        rest_methods().len() >= 100,
        "REST 方法面应 >= 100,实际 {}",
        rest_methods().len()
    );
    assert_eq!(ws_methods().len(), 8, "WS 核心 8 频道(ADR-0009)");
    assert_eq!(
        all_methods().len(),
        rest_methods().len() + ws_methods().len()
    );
}

#[test]
fn manifest_names_unique() {
    let mut seen = HashSet::new();
    for name in all_methods() {
        assert!(seen.insert(*name), "重复方法名: {name}");
    }
}

#[test]
fn rest_names_follow_convention() {
    for name in rest_methods() {
        let ok = REST_PREFIXES.iter().any(|p| name.starts_with(p));
        assert!(
            ok,
            "REST 方法名 {name} 不符合命名约定(允许前缀: {:?})",
            REST_PREFIXES
        );
    }
}

#[test]
fn ws_names_are_watch_methods() {
    for name in ws_methods() {
        assert!(
            name.starts_with("watch_"),
            "WS 方法名 {name} 应以 watch_ 开头"
        );
    }
}

#[test]
fn adapter_implemented_subset_of_manifest() {
    // M2d/M3d:适配器声明的已实现方法必须是统一方法面的子集(防止实现未枚举的方法)
    // 仅校验当前 feature 组合下已编译的适配器(其余交易所按 feature 门控,见 ADR)。
    // 适配器清单由 build.rs 扫描生成并集中在 `ADAPTER_PAIRS`(见 src/contract_gen.rs),
    // 此处不再手抄 pairs。Binance/Kalshi 仍按需直接引用(默认 feature,见下方断言)。
    use adaq_trading_crypto::adapters::{Binance, Kalshi};

    let manifest: HashSet<&str> = all_methods().iter().copied().collect();

    for &(exchange, implemented) in ADAPTER_PAIRS {
        assert!(!implemented.is_empty(), "{exchange} IMPLEMENTED 非空");
        for name in implemented {
            assert!(
                manifest.contains(name),
                "{exchange} 声明已实现 {name},但不在统一方法面清单中"
            );
        }
    }
    assert!(
        Binance::IMPLEMENTED.contains(&"fetch_balance"),
        "binance 应声明私密面方法 fetch_balance"
    );
    assert!(
        Kalshi::IMPLEMENTED.contains(&"create_order"),
        "kalshi 应声明 create_order(预测市场下单)"
    );
}

#[test]
fn manifest_matches_trait_surface() {
    // 关键锚点方法必须存在(契约基线的代表性成员)
    for anchor in [
        "fetch_markets",
        "fetch_ticker",
        "fetch_ohlcv",
        "fetch_order_book",
        "create_order",
        "cancel_order",
        "fetch_order",
        "fetch_balance",
        "fetch_positions",
        "set_leverage",
        "withdraw",
        "watch_order_book",
        "watch_ohlcv",
    ] {
        assert!(all_methods().contains(&anchor), "清单缺少锚点方法 {anchor}");
    }
}
