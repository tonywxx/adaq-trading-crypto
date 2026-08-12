//! 本地 CLI:功能测试与代码示例(ADR-0011 的 M0 骨架)。
//!
//! 发布到 crates.io 的包会通过 `Cargo.toml` 的 `exclude` 排除本文件;
//! 仅用于本地冒烟测试与示例演示。后续里程碑将扩展为
//! `--exchange --method` 通用调用与差分测试手动入口。

use std::process::ExitCode;

use adaq_trading_crypto::precise::{DIV_PRECISION, Precise};
use adaq_trading_crypto::{PaddingMode, PrecisionMode, RoundingMode, decimal_to_precision};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("precise") => cmd_precise(&args[2..]),
        Some("format") => cmd_format(&args[2..]),
        Some("exchanges") => cmd_exchanges(),
        Some("-h") | Some("--help") | None => {
            print_usage();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("unknown command: {other}");
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn cmd_precise(args: &[String]) -> ExitCode {
    if args.len() != 3 {
        eprintln!("usage: adaq-trading-crypto precise <a> <op> <b>");
        return ExitCode::from(2);
    }
    let a = match args[0].parse::<Precise>() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("invalid a: {e}");
            return ExitCode::from(2);
        }
    };
    let b = match args[2].parse::<Precise>() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("invalid b: {e}");
            return ExitCode::from(2);
        }
    };
    let result = match args[1].as_str() {
        "add" | "+" => a.add(&b).to_string(),
        "sub" | "-" => a.sub(&b).to_string(),
        "mul" | "*" => a.mul(&b).to_string(),
        "div" | "/" => a.div(&b, DIV_PRECISION).to_string(),
        "mod" | "%" => a.rem(&b).to_string(),
        "gt" => a.gt(&b).to_string(),
        "ge" => a.ge(&b).to_string(),
        "lt" => a.lt(&b).to_string(),
        "le" => a.le(&b).to_string(),
        "eq" => a.equals(&b).to_string(),
        op => {
            eprintln!("unknown op: {op} (add|sub|mul|div|mod|gt|ge|lt|le|eq)");
            return ExitCode::from(2);
        }
    };
    println!("{} {} {} = {}", args[0], args[1], args[2], result);
    ExitCode::SUCCESS
}

fn cmd_format(args: &[String]) -> ExitCode {
    if args.len() < 4 {
        eprintln!("usage: adaq-trading-crypto format <value> <places|tick> <mode> [round]");
        return ExitCode::from(2);
    }
    let value = &args[0];
    let precision = &args[1];
    let mode = match args[2].as_str() {
        "dp" => PrecisionMode::DecimalPlaces,
        "sig" => PrecisionMode::SignificantDigits,
        "tick" => PrecisionMode::TickSize,
        other => {
            eprintln!("unknown mode: {other} (dp|sig|tick)");
            return ExitCode::from(2);
        }
    };
    let rounding = match args.get(3).map(String::as_str).unwrap_or("round") {
        "round" => RoundingMode::Round,
        "truncate" => RoundingMode::Truncate,
        "up" => RoundingMode::RoundUp,
        "down" => RoundingMode::RoundDown,
        other => {
            eprintln!("unknown rounding: {other} (round|truncate|up|down)");
            return ExitCode::from(2);
        }
    };
    match decimal_to_precision(value, mode, precision, rounding, PaddingMode::NoPadding) {
        Ok(out) => {
            println!("{value} -> {out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

fn cmd_exchanges() -> ExitCode {
    println!("registered exchanges (M0): none yet — adapters arrive in M2 (binance first)");
    ExitCode::SUCCESS
}

fn print_usage() {
    println!(
        "adaq-trading-crypto {} — AdaQ unified trading interface (local CLI)\n\n\
         commands:\n  \
         precise <a> <op> <b>            exact decimal arithmetic demo (add|sub|mul|div|mod|gt|ge|lt|le|eq)\n  \
         format <v> <prec> <mode> [r]    decimalToPrecision demo (mode: dp|sig|tick; r: round|truncate|up|down)\n  \
         exchanges                      list registered exchange adapters\n  \
         -h, --help                     show this help",
        env!("CARGO_PKG_VERSION")
    );
}
