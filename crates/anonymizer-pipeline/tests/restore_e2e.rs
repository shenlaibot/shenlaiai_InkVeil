//! 往返：脱敏 → （模拟 AI 改写占位符写法）→ 还原后文本与原文一致。
//! 覆盖 txt（Rust restore_fuzzy）与 docx / xlsx（sidecar /restore）。依赖 sidecar/.venv，缺失时跳过。

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use anonymizer_core::mapping::MappingTable;
use anonymizer_core::rules::builtin_rules;
use anonymizer_core::transform::{OutputKind, Strategy};
use anonymizer_pipeline::{run_batch, PipelineConfig, SidecarClient};

const GEN: &str = r#"
import sys
from docx import Document
from openpyxl import Workbook
d = Document()
d.add_paragraph("甲方：北京云图科技有限公司，联系人张明辉，电话13800138000。")
d.add_paragraph("乙方联系人李国强，电话13912345678；张明辉将参会。")
d.save(sys.argv[1] + "/a.docx")
wb = Workbook(); ws = wb.active
ws["A1"] = "联系人：张明辉"; ws["A2"] = "电话：13800138000"; ws["A3"] = "李国强 13912345678"
wb.save(sys.argv[1] + "/b.xlsx")
open(sys.argv[1] + "/c.txt", "w", encoding="utf-8").write("联系人：张明辉，电话：13800138000\n另有李国强（13912345678）到场。\n")
"#;

/// 把 `[姓名1]` 之类改成 AI 常见的写法：全角括号 + 括号内插空格。
fn mangle(s: &str) -> String {
    let mut out = String::new();
    let mut in_ph = false;
    for c in s.chars() {
        match (c, in_ph) {
            ('[', false) => {
                in_ph = true;
                out.push_str("【 ");
            }
            (']', true) => {
                in_ph = false;
                out.push_str(" 】");
            }
            _ => out.push(c),
        }
    }
    out
}

#[test]
fn redact_then_restore_roundtrip() {
    let Some((_g, url)) = common::start_sidecar() else {
        eprintln!("跳过 restore_e2e：缺少 sidecar/.venv");
        return;
    };
    let py = common::sidecar_python().unwrap();
    let work = std::env::temp_dir().join(format!("anonkit_restore_e2e_{}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    let (inp, out, back) = (work.join("in"), work.join("out"), work.join("back"));
    for d in [&inp, &out, &back] {
        fs::create_dir_all(d).unwrap();
    }
    assert!(Command::new(&py).args(["-c", GEN]).arg(&inp).status().unwrap().success());

    let mut strategy_map = std::collections::HashMap::new();
    let mut ids: Vec<String> = builtin_rules().into_iter().map(|r| r.id).collect();
    ids.extend(["org", "address", "dict"].map(String::from));
    for id in ids {
        strategy_map.insert(id, Strategy { output: OutputKind::Placeholder, ..Default::default() });
    }
    let dict = anonymizer_core::dict::DictMatcher::new("dict", vec!["张明辉".into(), "李国强".into()]);
    let cfg = PipelineConfig { strategy_map, dict_matchers: vec![dict], ..Default::default() };
    let client = SidecarClient::new(url);
    let files: Vec<_> = ["a.docx", "b.xlsx", "c.txt"].iter().map(|n| inp.join(n)).collect();
    let res = run_batch(&client, &files, &out, &cfg, |_, _| {});
    let mut mapping = MappingTable::new();
    mapping.from_json(&fs::read_to_string(res.anonymap_path.as_ref().expect("应生成映射")).unwrap()).unwrap();
    assert!(mapping.len() >= 4, "映射应覆盖姓名/电话/公司: {}", mapping.len());

    for (f, r) in files.iter().zip(res.files.iter()) {
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        let masked = Path::new(r.output_path.as_ref().unwrap_or_else(|| panic!("{name} 脱敏失败: {:?}", r.error)));
        let orig_text = client.parse(f).unwrap().text;
        let masked_text = client.parse(masked).unwrap().text;
        assert!(!masked_text.contains("张明辉"), "{name} 脱敏不彻底");

        let restored_text = if name.ends_with(".txt") {
            // 文本：模拟 AI 把占位符改成【 姓名1 】后做容错还原
            let r = mapping.restore_fuzzy(&mangle(&masked_text));
            assert_eq!(r.exact_count, 0);
            assert!(r.fuzzy_count >= 3, "{}", r.fuzzy_count);
            assert!(r.suspects.is_empty());
            r.output_text
        } else {
            // 文档：直接还原为新文件
            let v = client.restore_doc(masked, &back, mapping.placeholder_to_original()).unwrap();
            assert!(v["exact_count"].as_u64().unwrap() >= 3, "{v}");
            assert_eq!(v["suspects"].as_array().unwrap().len(), 0, "{v}");
            let outp = v["output_path"].as_str().unwrap();
            assert_ne!(Path::new(outp), masked, "不得覆盖脱敏输入");
            client.parse(Path::new(outp)).unwrap().text
        };
        assert_eq!(restored_text.trim_end(), orig_text.trim_end(), "{name} 往返不一致");
    }
    let _ = fs::remove_dir_all(&work);
}
