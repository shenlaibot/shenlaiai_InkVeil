//! Golden corpus 质量门禁（spec Requirement：质量门禁）。
//!
//! 每次测试运行即执行：内置「真实形态」敏感样本（合同/登记表/配置等），
//! 用内置规则检测引擎计算召回率。
//!   - P0 类型（手机号 / 身份证 / 银行卡）召回 ≥ 99%
//!   - P1 类型（邮箱 / 座机 / 车牌 / IP / 金额 / 日期 / 网址）召回 ≥ 95%
//!     任何规则变更造成样本回归 → 构建失败。
//!
//! 注意：P0 号码样本必须通过对应格式校验器（身份证校验位、银行卡 Luhn），
//! 否则 Detector 的 `is_valid_id_card` / `luhn_valid` 会正确地拒绝它们，
//! 测试将无法通过（此时应更换样本而非放宽门禁）。

use anonymizer_core::{Hit, detect::Detector, rules::builtin_rules};

#[derive(Clone, Copy, PartialEq, Debug)]
enum Tier {
    P0,
    P1,
}

/// 单个样本：一段真实形态文本 + 其中应被识别的敏感原文列表。
struct Case {
    name: &'static str,
    tier: Tier,
    text: &'static str,
    sensitive: &'static [&'static str],
}

const CASES: &[Case] = &[
    // ---------- P0：手机号 ----------
    Case {
        name: "mobile_contact",
        tier: Tier::P0,
        text: "客户联系方式：13800138000（张伟），备用手机 13912345678，请于工作日联系。",
        sensitive: &["13800138000", "13912345678"],
    },
    // ---------- P0：身份证（校验位合法） ----------
    Case {
        name: "id_card_contract",
        tier: Tier::P0,
        text: "合同签署人王芳，身份证号 440301197407210509，与关联人员 110105199609043750 共同列席。",
        sensitive: &["440301197407210509", "110105199609043750"],
    },
    // ---------- P0：银行卡（Luhn 合法） ----------
    Case {
        name: "bank_card_payroll",
        tier: Tier::P0,
        text: "工资卡 930046350446801675，报销卡 733894260718908051，均已在我行柜台登记。",
        sensitive: &["930046350446801675", "733894260718908051"],
    },
    // ---------- P1：邮箱 ----------
    Case {
        name: "email_contact",
        tier: Tier::P1,
        text: "联系邮箱 zhangsan@example.com、lisi@corp.com.cn，备用 lwang@outlook.com 均可接收。",
        sensitive: &[
            "zhangsan@example.com",
            "lisi@corp.com.cn",
            "lwang@outlook.com",
        ],
    },
    // ---------- P1：座机 ----------
    Case {
        name: "phone_landline",
        tier: Tier::P1,
        text: "公司总机 010-88886666，传真 021-55667788，对外公开。",
        sensitive: &["010-88886666", "021-55667788"],
    },
    // ---------- P1：车牌 ----------
    Case {
        name: "plate_vehicle",
        tier: Tier::P1,
        text: "公务车辆京A12345、沪B88888 已到车队备案。",
        sensitive: &["京A12345", "沪B88888"],
    },
    // ---------- P1：IP ----------
    Case {
        name: "ip_config",
        tier: Tier::P1,
        text: "内网 IP 192.168.1.100、10.0.3.21，出口网关 172.16.0.1 由运维配置。",
        sensitive: &["192.168.1.100", "10.0.3.21", "172.16.0.1"],
    },
    // ---------- P1：金额 ----------
    Case {
        name: "amount_money",
        tier: Tier::P1,
        text: "合同金额 ¥1,234,567.89，预付款 50万元 于签约后支付。",
        sensitive: &["¥1,234,567.89", "50万元"],
    },
    // ---------- P1：日期 ----------
    Case {
        name: "date_sign",
        tier: Tier::P1,
        text: "合同签署日期 2024-06-30，生效日期 2025年1月15日。",
        sensitive: &["2024-06-30", "2025年1月15日"],
    },
    // ---------- P1：网址 ----------
    Case {
        name: "url_link",
        tier: Tier::P1,
        text: "官方网址 https://example.com/legal，备案号可在站内查询。",
        sensitive: &["https://example.com/legal"],
    },
    // ---------- P1：公司名称（需上下文引导，命中本体不含前缀） ----------
    Case {
        name: "company_name_payee",
        tier: Tier::P1,
        text: "户名：成都云图科技有限公司；开户名：四川锦江商务咨询有限公司。",
        sensitive: &["成都云图科技有限公司", "四川锦江商务咨询有限公司"],
    },
    // ---------- P1：开户银行 ----------
    Case {
        name: "bank_name_account",
        tier: Tier::P1,
        text: "开户银行：中国建设银行成都高新支行；开户行：招商银行北京分行营业部。",
        sensitive: &["中国建设银行成都高新支行", "招商银行北京分行营业部"],
    },
];

/// 计算召回：标注原文是否被任一命中完整覆盖（按原文在文本中出现的偏移判定）。
fn recall(case: &Case, hits: &[Hit]) -> (usize, usize) {
    let mut covered = 0usize;
    for needle in case.sensitive {
        let ok = case.text.match_indices(needle).any(|(pos, _)| {
            hits.iter()
                .any(|h| h.start <= pos && h.end >= pos + needle.len())
        });
        if ok {
            covered += 1;
        }
    }
    (covered, case.sensitive.len())
}

fn tier_cases(tier: Tier) -> impl Iterator<Item = &'static Case> {
    CASES.iter().filter(move |c| c.tier == tier)
}

/// 聚合多个样本的召回率。
fn aggregate(tier: Tier, rules: &[anonymizer_core::RuleDef]) -> (usize, usize) {
    let mut covered = 0usize;
    let mut total = 0usize;
    for case in tier_cases(tier) {
        let hits = Detector::detect(case.text, rules);
        let (c, n) = recall(case, &hits);
        covered += c;
        total += n;
    }
    (covered, total)
}

fn rate(c: usize, n: usize) -> f64 {
    c as f64 / n as f64
}

#[test]
fn golden_p0_recall_at_least_99_percent() {
    let rules = builtin_rules();
    let (c, n) = aggregate(Tier::P0, &rules);
    assert!(
        rate(c, n) >= 0.99,
        "P0（手机/身份证/银行卡）召回率 {c}/{n} = {:.2}% < 99%。\
         请检查规则是否回归，或样本号码是否通过格式校验器（身份证校验位/Luhn）",
        rate(c, n) * 100.0
    );
}

#[test]
fn golden_p1_recall_at_least_95_percent() {
    let rules = builtin_rules();
    let (c, n) = aggregate(Tier::P1, &rules);
    assert!(
        rate(c, n) >= 0.95,
        "P1 类型召回率 {c}/{n} = {:.2}% < 95%。请检查规则是否回归",
        rate(c, n) * 100.0
    );
}

#[test]
fn golden_no_sample_regression() {
    // 逐样本门禁：任一 P0 样本召回 < 99%、任一 P1 样本召回 < 95% 即失败。
    let rules = builtin_rules();
    for case in CASES {
        let hits = Detector::detect(case.text, &rules);
        let (c, n) = recall(case, &hits);
        let threshold = match case.tier {
            Tier::P0 => 0.99,
            Tier::P1 => 0.95,
        };
        assert!(
            rate(c, n) >= threshold,
            "样本「{}」({:?}) 召回率 {c}/{n} = {:.2}% < {:.0}%：漏检 {:?}",
            case.name,
            case.tier,
            rate(c, n) * 100.0,
            threshold * 100.0,
            case.sensitive
                .iter()
                .filter(|needle| !case.text.match_indices(*needle).any(|(pos, _)| {
                    hits.iter()
                        .any(|h| h.start <= pos && h.end >= pos + needle.len())
                }))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn golden_detector_returns_hits_with_sane_ranges() {
    // 完整性冒烟：所有命中区间都落在文本内且 text 字段与原文一致。
    let rules = builtin_rules();
    for case in CASES {
        for h in Detector::detect(case.text, &rules) {
            assert!(h.start < h.end, "空命中: {h:?}");
            assert!(h.end <= case.text.len(), "越界命中: {h:?}");
            assert_eq!(&case.text[h.start..h.end], h.text, "命中文本与原文不一致");
        }
    }
}
