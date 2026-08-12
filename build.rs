//! 构建脚本:从 `src/exchange.rs` 自动提取统一方法面清单(ADR-0001)。
//!
//! 生成 `OUT_DIR/methods.rs`:
//! - `REST_METHODS`:Exchange trait 的 REST 方法名(含显式 `async fn` 与
//!   `def_method!` 宏生成的方法),按源码出现顺序。
//! - `WS_METHODS`:Realtime trait 的 watch_* 方法名。
//!
//! 清单与 trait 同源,避免手维护漂移;`rerun-if-changed` 保证 trait 变动时重建。

use std::path::Path;

use regex::Regex;

fn main() {
    println!("cargo:rerun-if-changed=src/exchange.rs");

    let src = std::fs::read_to_string("src/exchange.rs").expect("read src/exchange.rs");

    // 定位两个 trait 的文本区间
    let exchange_start = src
        .find("pub trait Exchange")
        .expect("pub trait Exchange not found");
    let realtime_start = src
        .find("pub trait Realtime")
        .expect("pub trait Realtime not found");
    let tests_start = src.find("#[cfg(test)]").unwrap_or(src.len());

    let rest_section = &src[exchange_start..realtime_start];
    let ws_section = &src[realtime_start..tests_start];

    let method_re = Regex::new(r"(?:def_method!\s*\(\s*|async fn\s+)([a-z_][a-z0-9_]*)").unwrap();

    fn collect(section: &str, re: &Regex) -> Vec<String> {
        let mut names = Vec::new();
        for cap in re.captures_iter(section) {
            names.push(cap[1].to_string());
        }
        names
    }

    let rest = collect(rest_section, &method_re);
    let ws = collect(ws_section, &method_re);

    // 生成清单文件
    let mut out = String::new();
    out.push_str("// 本文件由 build.rs 自动生成,请勿手改。\n");
    out.push_str("// 来源:src/exchange.rs(Exchange / Realtime trait)。\n\n");
    out.push_str(&format!(
        "pub const REST_METHODS: &[&str] = &[\n    {},\n];\n\n",
        rest.iter()
            .map(|n| format!("\"{n}\""))
            .collect::<Vec<_>>()
            .join(",\n    ")
    ));
    out.push_str(&format!(
        "pub const WS_METHODS: &[&str] = &[\n    {},\n];\n\n",
        ws.iter()
            .map(|n| format!("\"{n}\""))
            .collect::<Vec<_>>()
            .join(",\n    ")
    ));
    out.push_str("pub const ALL_METHODS: &[&str] = &[\n    ");
    out.push_str(
        &rest
            .iter()
            .chain(ws.iter())
            .map(|n| format!("\"{n}\""))
            .collect::<Vec<_>>()
            .join(", "),
    );
    out.push_str(",\n];\n");

    let out_path = Path::new(&std::env::var("OUT_DIR").unwrap()).join("methods.rs");
    std::fs::write(&out_path, out).expect("write methods.rs");

    eprintln!(
        "methods manifest: {} REST + {} WS methods",
        rest.len(),
        ws.len()
    );
}
