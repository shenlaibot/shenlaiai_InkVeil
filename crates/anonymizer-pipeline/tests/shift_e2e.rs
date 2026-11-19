//! 保结构替换端到端：真实 sidecar 写回 + 项目保险库跨批次/跨会话 + 精确还原。

mod common;

use std::collections::HashMap;
use std::fs;

use anonymizer_core::rules::builtin_rules;
use anonymizer_core::transform::{OutputKind, Strategy};
use anonymizer_core::vault::Vault;
use anonymizer_pipeline::{run_batch, PipelineConfig, SidecarClient};

fn cfg(initial: Option<anonymizer_core::mapping::MappingTable>) -> PipelineConfig {
    let mut strategy_map: HashMap<String, Strategy> = HashMap::new();
    let mut ids: Vec<String> = builtin_rules().into_iter().map(|r| r.id).collect();
    ids.extend(["org", "address", "dict"].map(String::from));
    for id in ids {
        let output = match id.as_str() {
            "amount" => OutputKind::Scale,
            "date" => OutputKind::Shift,
            _ => OutputKind::Keep,
        };
        strategy_map.insert(id, Strategy { output, ..Default::default() });
    }
    PipelineConfig { strategy_map, initial_mapping: initial, ..Default::default() }
}

const CONTRACT: &str = "股权转让合同\n\
本合同自2024-03-15起生效。转让价款为人民币壹佰贰拾万元整（小写：¥1,200,000.00），\n\
其中首期款300万元于2024年4月1日支付，余款于2024年12月31日前付清。\n\
逾期违约金按日万分之五计算，利率为年化8.5%；依据《民法典》第1,200条，\n\
《中华人民共和国民法典》自2021年1月1日起施行。\n";

#[test]
fn scale_and_shift_end_to_end_with_vault() {
    let Some((_g, url)) = common::start_sidecar() else {
        eprintln!("跳过 shift_e2e：缺少 sidecar/.venv");
        return;
    };
    let work = std::env::temp_dir().join(format!("anonkit_shift_e2e_{}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(work.join("in")).unwrap();
    fs::create_dir_all(work.join("out1")).unwrap();
    fs::create_dir_all(work.join("out2")).unwrap();
    let input = work.join("in").join("合同.txt");
    fs::write(&input, CONTRACT).unwrap();
    let client = SidecarClient::new(url);

    let vpath = work.join("案件.akcase");
    let mut vault = Vault::create(&vpath, "保结构测试", "pw").unwrap();
    let r1 = run_batch(&client, &[input.clone()], &work.join("out1"), &cfg(Some(vault.mapping.clone())), |_, _| {});
    assert!(r1.files[0].error.is_none(), "{:?}", r1.files[0]);
    let out1 = fs::read_to_string(r1.files[0].output_path.as_ref().unwrap()).unwrap();
    vault.mapping = r1.mapping.clone();
    vault.save().unwrap();

    // 1. 真实数值与日期都不在输出里
    for secret in ["1,200,000.00", "壹佰贰拾万元", "300万元", "2024-03-15", "2024年4月1日", "2024年12月31日"] {
        assert!(!out1.contains(secret), "{secret} 不应保留:\n{out1}");
    }
    // 2. 不该动的原样保留：百分比、条款号、法规施行日
    for keep in ["年化8.5%", "第1,200条", "2021年1月1日起施行", "万分之五"] {
        assert!(out1.contains(keep), "{keep} 应保留:\n{out1}");
    }
    // 3. 大写与小写金额缩放后仍一致：用账本里的记忆值核对
    let book = r1.mapping.shift_book();
    let up = &book.entries["人民币壹佰贰拾万元整"];
    let lo = &book.entries["¥1,200,000.00"];
    let lo_yuan: i64 = lo.trim_start_matches('¥').split('.').next().unwrap().replace(',', "").parse().unwrap();
    assert!(out1.contains(up.as_str()) && out1.contains(lo.as_str()));
    let k = book.params.as_ref().unwrap().k["CNY"];
    assert!((lo_yuan as f64 - 1_200_000.0 * k).abs() <= 1.0, "{lo} vs k={k}");
    // 4. 日期间隔保持：4/1 → 12/31 的天数不变（同一 Δd）
    let d1 = &book.entries["2024年4月1日"];
    let d2 = &book.entries["2024年12月31日"];
    assert!(out1.contains(d1.as_str()) && out1.contains(d2.as_str()));

    // 5. 重新打开项目（新会话）：同一文件第二批得到完全相同的输出，参数不变
    drop(vault);
    let vault2 = Vault::open(&vpath, "pw").unwrap();
    assert_eq!(vault2.mapping.shift_book().params, book.params);
    let r2 = run_batch(&client, &[input], &work.join("out2"), &cfg(Some(vault2.mapping.clone())), |_, _| {});
    let out2 = fs::read_to_string(r2.files[0].output_path.as_ref().unwrap()).unwrap();
    assert_eq!(out1, out2, "跨批次应一致");

    // 6. 往返：走对照表精确还原得到原文
    assert_eq!(vault2.mapping.shift_book().restore_exact(&out1), CONTRACT);
    let _ = fs::remove_dir_all(&work);
}
