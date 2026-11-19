//! 复杂语料端到端：生成（含隐藏内容的 docx/xlsx/pdf/txt）→ 检测 → 脱敏写回 → 复检 → 还原。
//!
//! 语料与标准答案由 `scripts/gen_complex_corpus.py` 生成；判定由 `scripts/verify_corpus.py` 完成。
//! 依赖 `sidecar/.venv`；缺失时跳过。设置环境变量 `ANONKIT_E2E_KEEP=1` 保留产物并打印目录。

mod common;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anonymizer_core::mapping::MappingTable;
use anonymizer_core::rules::builtin_rules;
use anonymizer_core::transform::{OutputKind, Strategy};
use anonymizer_pipeline::{run_batch, PipelineConfig, SidecarClient};

fn temp_dir(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("anonkit_e2e_{}_{name}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    base
}

#[test]
fn corpus_e2e() {
    let Some((_guard, base_url)) = common::start_sidecar() else {
        eprintln!("跳过 corpus_e2e：缺少 sidecar/.venv");
        return;
    };
    let root = common::workspace_root();
    let py = common::sidecar_python().unwrap();
    let work = temp_dir("work");
    let corpus = work.join("corpus");
    let out = work.join("out");
    let restored = work.join("restored");
    fs::create_dir_all(&out).unwrap();
    fs::create_dir_all(&restored).unwrap();

    let st = Command::new(&py)
        .arg(root.join("scripts").join("gen_complex_corpus.py"))
        .arg(&corpus)
        .status()
        .expect("无法运行语料生成脚本");
    assert!(st.success(), "语料生成失败");

    let mut inputs: Vec<PathBuf> = fs::read_dir(&corpus)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().is_some_and(|n| n != "truth.json"))
        .collect();
    inputs.sort();

    let mut strategy_map: HashMap<String, Strategy> = HashMap::new();
    let mut ids: Vec<String> = builtin_rules().into_iter().map(|r| r.id).collect();
    ids.extend(["org", "address", "dict"].map(String::from));
    for id in ids {
        strategy_map.insert(
            id,
            Strategy { output: OutputKind::Placeholder, ..Default::default() },
        );
    }
    // 设置 ANK_NER_MODEL=<模型目录> 时启用 NER 通道，验证真实模型接入后整条链路不退化
    let ner_model_path = std::env::var("ANK_NER_MODEL").ok();
    let cfg = PipelineConfig { strategy_map, ner_model_path, ..Default::default() };
    let client = SidecarClient::new(base_url);
    let res = run_batch(&client, &inputs, &out, &cfg, |_, _| {});

    // 还原：解析输出全文 → 用映射表还原 → 写出供 verify 脚本核对
    let mut mapping = MappingTable::new();
    if let Some(p) = &res.anonymap_path {
        mapping.from_json(&fs::read_to_string(p).unwrap()).unwrap();
    }
    let mut manifest = Vec::new();
    for (inp, r) in inputs.iter().zip(res.files.iter()) {
        let name = inp.file_name().unwrap().to_string_lossy().to_string();
        if let Some(o) = &r.output_path {
            if let Ok(parsed) = client.parse(std::path::Path::new(o)) {
                let back = mapping.restore(&parsed.text);
                fs::write(restored.join(format!("{name}.txt")), back).unwrap();
            }
        }
        manifest.push(serde_json::json!({
            "file": name,
            "output": r.output_path,
            "error": r.error,
            "is_deep": r.is_deep,
            "hit_count": r.hit_count,
            "unplaced": r.unplaced_count,
            "warnings": r.warnings,
        }));
    }
    let manifest_path = work.join("manifest.json");
    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();

    let keep = std::env::var("ANONKIT_E2E_KEEP").is_ok();
    if keep {
        eprintln!("E2E 产物目录: {}", work.display());
    }
    let out_v = Command::new(&py)
        .arg(root.join("scripts").join("verify_corpus.py"))
        .arg(&corpus)
        .arg(&manifest_path)
        .arg(&restored)
        .output()
        .expect("无法运行校验脚本");
    println!("{}", String::from_utf8_lossy(&out_v.stdout));
    eprintln!("{}", String::from_utf8_lossy(&out_v.stderr));
    if !keep {
        let _ = fs::remove_dir_all(&work);
    }
    assert!(out_v.status.success(), "复杂语料端到端校验未通过，详见上方报告");
}
