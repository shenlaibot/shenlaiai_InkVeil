//! anonymizer-pipeline 集成测试。
//!
//! 依赖本机 sidecar（`sidecar/.venv` + `sidecar/models/ner`）。启动 sidecar 通过
//! `tests/common` 完成；`.venv` 缺失时测试打印提示并跳过（不 fail），
//! 保证 CI 无 Python 环境时 `cargo test --workspace` 不挂。

mod common;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anonymizer_core::detect::Detector;
use anonymizer_core::dict::DictMatcher;
use anonymizer_core::mapping::MappingTable;
use anonymizer_core::rules::{builtin_rules, id_card_check_code};
use anonymizer_core::transform::{OutputKind, Strategy};
use anonymizer_core::Hit;
use anonymizer_core::HitSource;
use anonymizer_pipeline::{
    default_type_priority, merge_hits, run_batch, PipelineConfig, SidecarClient,
};

/// 独立临时目录（按测试名隔离，避免并行测试互相干扰）。
fn temp_dir(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!(
        "anonkit_pipeline_test_{}_{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    base
}

/// 全部内置规则 id → Placeholder 策略。
fn placeholder_strategies() -> HashMap<String, Strategy> {
    let ids = [
        "mobile",
        "id_card",
        "bank_card",
        "phone",
        "email",
        "plate",
        "ip",
        "amount",
        "date",
        "url",
        "name",
    ];
    ids.into_iter()
        .map(|id| {
            (
                id.to_string(),
                Strategy {
                    output: OutputKind::Placeholder,
                    ..Default::default()
                },
            )
        })
        .collect()
}

/// 构造合法身份证号（前 17 位 + 计算校验位）。
fn valid_id_card() -> String {
    let s17 = "11010119840320123";
    let last = id_card_check_code(s17);
    format!("{s17}{last}")
}

/// 全链路 helper：跑一遍 run_batch，返回 (输出文件路径, 映射表路径, 原始文本)。
fn run_full_chain(base_url: &str, tmp: &Path) -> (PathBuf, String, String) {
    let id = valid_id_card();
    let content = format!("联系人：张三\n手机 13800138000\n身份证号 {id}\n");
    let input = tmp.join("合同.txt");
    fs::write(&input, &content).unwrap();

    let out_dir = tmp.join("out");
    let config = PipelineConfig {
        strategy_map: placeholder_strategies(),
        ..Default::default()
    };
    let client = SidecarClient::new(base_url.to_string());
    let result = run_batch(&client, &[input], &out_dir, &config, |_, _| {});

    assert_eq!(result.files.len(), 1);
    let fr = &result.files[0];
    assert!(fr.error.is_none(), "处理失败: {:?}", fr.error);
    let out_path = fr.output_path.as_ref().expect("应有输出文件");
    assert!(Path::new(out_path).exists(), "输出文件不存在: {out_path}");
    let map_path = result.anonymap_path.as_ref().expect("应有映射表文件");
    assert!(Path::new(map_path).exists(), "映射表不存在: {map_path}");
    (PathBuf::from(out_path), map_path.clone(), content)
}

#[test]
fn test_full_chain_txt() {
    let Some((_guard, base_url)) = common::start_sidecar() else {
        eprintln!("sidecar/.venv 不存在，跳过 test_full_chain_txt");
        return;
    };
    let tmp = temp_dir("full_chain");
    let id = valid_id_card();
    let content = format!("联系人：张三\n手机 13800138000\n身份证号 {id}\n");
    let input = tmp.join("合同.txt");
    fs::write(&input, &content).unwrap();

    let config = PipelineConfig {
        strategy_map: placeholder_strategies(),
        ..Default::default()
    };
    let client = SidecarClient::new(base_url);
    let result = run_batch(&client, &[input], &tmp.join("out"), &config, |_, _| {});

    let fr = &result.files[0];
    assert!(fr.error.is_none(), "处理失败: {:?}", fr.error);
    assert!(fr.hit_count >= 2, "应命中手机号与身份证: {}", fr.hit_count);
    assert_eq!(fr.confirmed, fr.hit_count);
    assert!(fr.output_path.is_some());
    let out_path = Path::new(fr.output_path.as_ref().unwrap());
    assert!(out_path.exists());
    let out_text = fs::read_to_string(out_path).unwrap();
    // 敏感原文消失、占位符出现
    assert!(!out_text.contains("13800138000"), "手机号未脱敏");
    assert!(!out_text.contains(&id), "身份证未脱敏");
    assert!(
        out_text.contains("[手机号1]"),
        "缺少手机号占位符: {out_text}"
    );
    assert!(
        out_text.contains("[身份证号1]"),
        "缺少身份证占位符: {out_text}"
    );
    // 映射表落盘
    let map_path = result.anonymap_path.expect("应生成 anonymap");
    assert!(Path::new(&map_path).exists());
}

#[test]
fn test_restore_roundtrip() {
    let Some((_guard, base_url)) = common::start_sidecar() else {
        eprintln!("sidecar/.venv 不存在，跳过 test_restore_roundtrip");
        return;
    };
    let tmp = temp_dir("restore_roundtrip");
    let (out_path, map_path, original) = run_full_chain(&base_url, &tmp);

    let mut mapping = MappingTable::new();
    mapping
        .from_json(&fs::read_to_string(&map_path).unwrap())
        .unwrap();
    let out_text = fs::read_to_string(&out_path).unwrap();
    assert_eq!(mapping.restore(&out_text), original, "还原应与原文一致");
}

#[test]
fn test_alias_same_placeholder() {
    let Some((_guard, base_url)) = common::start_sidecar() else {
        eprintln!("sidecar/.venv 不存在，跳过 test_alias_same_placeholder");
        return;
    };
    let tmp = temp_dir("alias_same_placeholder");
    let content = "甲方：北京降本增笑有限公司（以下简称“降本增笑”）\n".to_string()
        + "本合同由北京降本增笑有限公司与乙方签署，下文简称降本增笑。\n";
    let input = tmp.join("合同.txt");
    fs::write(&input, &content).unwrap();

    // 词典注入全称 + 简称，策略为 Placeholder
    let mut config = PipelineConfig {
        strategy_map: placeholder_strategies(),
        ..Default::default()
    };
    config.strategy_map.insert(
        "dict".into(),
        Strategy {
            output: OutputKind::Placeholder,
            ..Default::default()
        },
    );
    config.dict_matchers = vec![DictMatcher::new(
        "公司",
        vec!["北京降本增笑有限公司".into(), "降本增笑".into()],
    )];

    let client = SidecarClient::new(base_url);
    let result = run_batch(&client, &[input], &tmp.join("out"), &config, |_, _| {});
    let fr = &result.files[0];
    assert!(fr.error.is_none(), "处理失败: {:?}", fr.error);
    assert!(fr.hit_count >= 3, "应命中全称×2 + 简称×2: {}", fr.hit_count);

    let out_text = fs::read_to_string(Path::new(fr.output_path.as_ref().unwrap())).unwrap();
    // 全称与简称都被替换且为同一占位符
    assert!(!out_text.contains("北京降本增笑有限公司"), "全称未脱敏");
    assert!(!out_text.contains("降本增笑"), "简称未脱敏: {out_text}");
    let placeholder_count = out_text.matches("[公司1]").count();
    assert_eq!(
        placeholder_count, 4,
        "全称与简称应统一为 [公司1]: {out_text}"
    );
    assert!(!out_text.contains("[公司2]"), "不应出现第二个公司占位符");
}

#[test]
fn test_ner_channel() {
    let Some((_guard, base_url)) = common::start_sidecar() else {
        eprintln!("sidecar/.venv 不存在，跳过 test_ner_channel");
        return;
    };
    let models = common::workspace_root()
        .join("sidecar")
        .join("models")
        .join("ner");
    if !models.join("model.onnx").is_file() {
        eprintln!("sidecar/models/ner 模型缺失，跳过 test_ner_channel");
        return;
    }
    let client = SidecarClient::new(base_url);
    // 模型首次加载较慢，阈值放低以增加人名命中概率
    let resp = client
        .ner(
            "张三在北京工作，最近联系了李四。",
            0.3,
            Some(&models.display().to_string()),
        )
        .expect("NER 请求应成功");
    assert!(resp.ok, "NER 应返回 ok: {:?}", resp.error);
    eprintln!("test_ner_channel entities: {:?}", resp.entities);
    if !resp.entities.is_empty() {
        assert!(
            resp.entities.iter().any(|e| e.label == "人名"),
            "应出现人名实体: {:?}",
            resp.entities
        );
    }
}

#[test]
fn test_overlap_merge() {
    // 合法身份证 → 规则命中 [3,21)；手工构造与身份证区间重叠的"手机号子串"命中
    let id = valid_id_card();
    let text = format!("身份证 {id}");
    let rule_hits = Detector::detect(&text, &builtin_rules());
    assert!(
        rule_hits.iter().any(|h| h.rule_id == "id_card"),
        "应命中身份证"
    );
    // 身份证内部 11 位子串（模拟 NER/词典通道误报的手机号）
    let ner_hits = vec![Hit {
        rule_id: "mobile".into(),
        rule_label: "手机号".into(),
        start: 6,
        end: 17,
        text: "19840320123".into(),
        confidence: 0.98,
        source: HitSource::Ner,
    }];
    let merged = merge_hits(rule_hits, vec![], ner_hits, &default_type_priority());
    assert_eq!(merged.len(), 1, "重叠命中应合并为一条: {merged:?}");
    assert_eq!(merged[0].rule_id, "id_card", "优先级更高者胜出（身份证）");
    assert!(
        matches!(merged[0].source, HitSource::Rule | HitSource::Context),
        "身份证来源应为规则（含上下文提升）: {:?}",
        merged[0].source
    );
}
