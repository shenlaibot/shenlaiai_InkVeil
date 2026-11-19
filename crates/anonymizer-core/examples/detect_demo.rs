//! 临时诊断工具：对文本文件运行内置检测，输出命中（基于字符偏移）。
//! 用法: cargo run -p anonymizer-core --example detect_demo -- <file>
use anonymizer_core::detect::Detector;
use anonymizer_core::rules::builtin_rules;
use std::io::Read;

fn main() {
    let mut args = std::env::args();
    let _ = args.next();
    let path = args.next().expect("usage: detect_demo <file>");
    let mut buf = String::new();
    std::fs::File::open(&path)
        .unwrap()
        .read_to_string(&mut buf)
        .unwrap();
    let hits = Detector::detect(&buf, &builtin_rules());
    for h in &hits {
        println!("{:<12} conf={:<5} [{}..{}] {:?}", h.rule_id, h.confidence, h.start, h.end, &buf[h.start..h.end]);
    }
    println!("total hits: {}", hits.len());
}