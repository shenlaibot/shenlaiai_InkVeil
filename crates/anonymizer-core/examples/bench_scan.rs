//! 规则扫描性能基准（spec 性能目标：10MB 纯文本规则扫描 < 1s）。
//!
//! 用法：
//!   cargo run --release --example bench_scan -- <text_file>
//!
//! 输出：文本大小 / 字符数 / 命中数 / 耗时 / 吞吐（MB/s）。退出码 0 表示达标
//! （size < 1MB 时按 1MB/s 线性折算阈值，避免小样本误判）。

use std::env;
use std::time::Instant;

use anonymizer_core::{detect::Detector, rules::builtin_rules};

fn main() {
    let path = env::args().nth(1).expect("用法: bench_scan <text_file>");
    let text = std::fs::read_to_string(&path).expect("读取文本失败");
    let size_mb = text.len() as f64 / (1024.0 * 1024.0);

    let rules = builtin_rules();
    // 预热一轮，避免首次分配/缓存冷启动计入计时
    let _ = Detector::detect(&text, &rules);

    let start = Instant::now();
    let hits = Detector::detect(&text, &rules);
    let elapsed = start.elapsed();
    let secs = elapsed.as_secs_f64();
    let throughput = size_mb / secs;

    println!(
        "scan: size={:.2}MB chars={} hits={} elapsed={:.3}s throughput={:.1}MB/s",
        size_mb,
        text.chars().count(),
        hits.len(),
        secs,
        throughput,
    );

    // 达标门槛：10MB < 1s（线性折算到实际大小）
    let budget = size_mb / 10.0;
    if secs <= budget {
        println!(
            "PASS: {:.2}MB 在 {:.2}s 内完成（门槛 {:.2}s）",
            size_mb, secs, budget
        );
    } else {
        println!("FAIL: 超出性能门槛（{:.2}s > {:.2}s）", secs, budget);
        std::process::exit(1);
    }
}
