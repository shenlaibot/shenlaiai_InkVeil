//! 项目保险库端到端：多批次、跨会话、用真实文档验证占位符编号稳定与还原。

mod common;

use std::fs;
use std::path::PathBuf;

use anonymizer_core::mapping::MappingTable;
use anonymizer_core::rules::builtin_rules;
use anonymizer_core::transform::{OutputKind, Strategy};
use anonymizer_core::vault::Vault;
use anonymizer_pipeline::{run_batch, PipelineConfig, SidecarClient};

fn placeholder_cfg(initial: Option<MappingTable>) -> PipelineConfig {
    let mut strategy_map = std::collections::HashMap::new();
    let mut ids: Vec<String> = builtin_rules().into_iter().map(|r| r.id).collect();
    ids.extend(["org", "address", "dict"].map(String::from));
    for id in ids {
        strategy_map.insert(id, Strategy { output: OutputKind::Placeholder, ..Default::default() });
    }
    PipelineConfig { strategy_map, initial_mapping: initial, ..Default::default() }
}

fn write(dir: &std::path::Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, body).unwrap();
    p
}

#[test]
fn placeholders_stable_across_batches_and_sessions() {
    let Some((_g, url)) = common::start_sidecar() else {
        eprintln!("跳过 vault_e2e：缺少 sidecar/.venv");
        return;
    };
    let work = std::env::temp_dir().join(format!("anonkit_vault_e2e_{}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(work.join("in")).unwrap();
    let client = SidecarClient::new(url);
    let vpath = work.join("案件.akcase");

    // ---- 第一批：张明辉 + 北京云图科技有限公司 ----
    let a1 = write(&work.join("in"), "batch1_a.txt", "甲方：北京云图科技有限公司\n联系人：张明辉，电话：13800138000\n");
    let mut vault = Vault::create(&vpath, "测试案件", "pw").unwrap();
    let out1 = work.join("out1");
    fs::create_dir_all(&out1).unwrap();
    let before = vault.mapping.len();
    let r1 = run_batch(&client, &[a1], &out1, &placeholder_cfg(Some(vault.mapping.clone())), |_, _| {});
    vault.mapping = r1.mapping.clone();
    vault.record_batch(1, before);
    vault.save().unwrap();
    let out1_text = fs::read_to_string(r1.files[0].output_path.as_ref().unwrap()).unwrap();
    assert!(!out1_text.contains("张明辉") && !out1_text.contains("13800138000"), "{out1_text}");
    drop(vault);

    // ---- 第二批（重新打开项目）：同一个人 + 新的人，且词序/文件不同 ----
    let b1 = write(&work.join("in"), "batch2_b.txt", "会议纪要\n联系人：李国强，电话：13912345678\n另：联系人：张明辉 将参会，北京云图科技有限公司 负责接待。\n");
    let mut vault = Vault::open(&vpath, "pw").unwrap();
    let out2 = work.join("out2");
    fs::create_dir_all(&out2).unwrap();
    let before = vault.mapping.len();
    let r2 = run_batch(&client, &[b1], &out2, &placeholder_cfg(Some(vault.mapping.clone())), |_, _| {});
    vault.mapping = r2.mapping.clone();
    vault.record_batch(1, before);
    vault.save().unwrap();
    let out2_text = fs::read_to_string(r2.files[0].output_path.as_ref().unwrap()).unwrap();

    // 同一实体在两批里必须是同一个占位符
    let p_name = vault.mapping.placeholder_of("张明辉").unwrap().to_string();
    let p_co = vault.mapping.placeholder_of("北京云图科技有限公司").unwrap().to_string();
    assert!(out1_text.contains(&p_name) && out2_text.contains(&p_name), "两批都应使用 {p_name}\n{out1_text}\n{out2_text}");
    assert!(out1_text.contains(&p_co) && out2_text.contains(&p_co));
    // 新实体不得复用旧编号
    let p_new = vault.mapping.placeholder_of("李国强").unwrap().to_string();
    assert_ne!(p_new, p_name);

    // 用最终项目映射还原**两批**产物
    let back1 = vault.mapping.restore(&out1_text);
    let back2 = vault.mapping.restore(&out2_text);
    assert!(back1.contains("张明辉") && back1.contains("13800138000") && back1.contains("北京云图科技有限公司"), "{back1}");
    assert!(back2.contains("张明辉") && back2.contains("李国强") && back2.contains("13912345678"), "{back2}");
    assert!(!back1.contains('[') || !back1.contains("姓名"), "不应残留占位符: {back1}");

    // 批次记录
    let reopened = Vault::open(&vpath, "pw").unwrap();
    assert_eq!(reopened.batches.len(), 2);
    assert_eq!(reopened.batches[1].seq, 2);
    assert!(reopened.batches[1].entries_added >= 2, "第二批至少新增李国强与手机号");

    // 错误口令打不开
    assert!(Vault::open(&vpath, "nope").is_err());
    let _ = fs::remove_dir_all(&work);
}
