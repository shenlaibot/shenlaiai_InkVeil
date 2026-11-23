//! 内置规则集：正则定义 + P0 类型的格式校验器。

use crate::RuleDef;

pub const ID_MOBILE: &str = "mobile";
pub const ID_ID_CARD: &str = "id_card";
pub const ID_BANK_CARD: &str = "bank_card";
pub const ID_CREDIT_CODE: &str = "credit_code";
pub const ID_COMPANY_NAME: &str = "company_name";
pub const ID_BANK_NAME: &str = "bank_name";
pub const ID_PHONE: &str = "phone";
pub const ID_PLATE: &str = "plate";
pub const ID_EMAIL: &str = "email";
pub const ID_IP: &str = "ip";
pub const ID_URL: &str = "url";
pub const ID_AMOUNT: &str = "amount";
pub const ID_DATE: &str = "date";
pub const ID_NAME: &str = "name";
pub const ID_CASE_NO: &str = "case_no";
pub const ID_BANK_ACCOUNT: &str = "bank_account";
pub const ID_PASSPORT: &str = "passport";
pub const ID_ID_CARD_HMT: &str = "id_card_hmt";
pub const ID_CONTRACT_NO: &str = "contract_no";
pub const ID_ADDRESS: &str = "address";

/// 手机号中允许出现的分隔符（空格/中划线/点/下划线/全角空格等）
pub const MOBILE_SEPARATORS: &[char] = &[' ', '-', '·', '_', '\u{3000}', '.'];

/// 内置规则集（顺序即优先级参考）
pub fn builtin_rules() -> Vec<RuleDef> {
    vec![
        RuleDef {
            id: ID_MOBILE.into(),
            label: "手机号".into(),
            // 11 位手机号，数字间可含常见分隔符（空格/-/·/_/全角空格/点），
            // 如 138 0013 8000、138-0013-8000、138.0013.8000。
            // 前 10 位数字后可带 0-1 个分隔符，第 11 位必须是数字（整体以数字结尾）。
            regex: r"[1１][3-9３-９](?:[\p{Zs}\t\-·_.]?\d){9}".into(),
            enabled: true,
            confidence: 0.98,
            context_keywords: vec!["手机".into(), "电话".into(), "联系电话".into(), "Mobile".into()],
        },
        RuleDef {
            id: ID_ID_CARD.into(),
            label: "身份证号".into(),
            regex: r"(\d{6}[ \u{3000}]?\d{8}[ \u{3000}]?\d{3}[\dXx]|\d{15})".into(),
            enabled: true,
            confidence: 0.99,
            context_keywords: vec!["身份证".into(), "证件号".into(), "公民身份".into()],
        },
        RuleDef {
            id: ID_BANK_CARD.into(),
            label: "银行卡号".into(),
            // 16 或 19 位数字（真实卡号主要长度），数字间可含常见分隔符，
            // 如 6222 0202 0008 8888 884、5100 1406 8350 5150 6677。
            // 不匹配 17/18 位连续数字，避免命中 18 位身份证。Luhn 仅作置信度调节（见 detect.rs）。
            // 换行可能是相邻段落或单元格的边界，不能把边界两侧拼成卡号。
            regex: r"(?:\d{4}[\p{Zs}\t\-·_.]?){3}\d{4}(?:[\p{Zs}\t\-·_.]?\d{3,4})?".into(),
            enabled: true,
            confidence: 0.95,
            context_keywords: vec!["银行卡".into(), "卡号".into(), "银行账户".into(), "账号".into()],
        },
        RuleDef {
            id: ID_CREDIT_CODE.into(),
            label: "统一社会信用代码".into(),
            // 18 位：登记管理部门(2) + 行政区划(6) + 主体标识(9) + 校验码(1)。
            // 字符集按 GB 32100-2015 排除 I/O/S/V；字母数字混合，不会与纯数字的身份证/银行卡撞车。
            regex: r"[0-9A-HJ-NP-RT-UW-Y]{2}\d{6}[0-9A-HJ-NP-RT-UW-Y]{10}".into(),
            enabled: true,
            confidence: 0.98,
            context_keywords: vec!["统一社会信用代码".into(), "信用代码".into(), "营业执照".into()],
        },
        RuleDef {
            id: ID_COMPANY_NAME.into(),
            label: "公司名称".into(),
            // 如「成都云图科技有限公司」。要求「户名：/开户名：/公司名称：/甲方：」等上下文，
            // 避免正文中零散出现「有限公司」被误报；捕获组 1 仅取公司名本体（见 detect.rs）。
            regex: r"(?:户名|开户名|公司名称|收款方|甲方|乙方|丙方|丁方|法定代表人|法人|原告|被告|第三人|上诉人|被上诉人|申请人|被申请人|申请执行人|被执行人|出租方|承租方|出租人|承租人|委托方|受托方|买方|卖方|转让方|受让方|发包方|承包方|供应商|合作方)[:：\s]*([\u4e00-\u9fa5]{2,15}(?:股份有限公司|有限责任公司|集团有限公司|有限公司|合伙企业|事务所|公司))".into(),
            enabled: true,
            confidence: 0.72,
            context_keywords: vec!["户名".into(), "开户".into(), "法定代表人".into(), "公司".into()],
        },
        RuleDef {
            id: ID_BANK_NAME.into(),
            label: "开户银行".into(),
            // 如「开户银行：中国银行成都高新支行」。同样要求「开户行/开户银行」上下文，
            // 避免正文中零散出现「有限公司」被误报；捕获组 1 仅取银行名本体（见 detect.rs）。
            regex: r"(?:开户银行|开户行|收款行)[:：\s]*([\u4e00-\u9fa5]{2,15}银行(?:[\u4e00-\u9fa5]{2,15}(?:支行|分行|营业部|分理处))?)".into(),
            enabled: true,
            confidence: 0.75,
            context_keywords: vec!["开户银行".into(), "开户行".into(), "银行".into(), "户名".into()],
        },
        RuleDef {
            id: ID_NAME.into(),
            label: "姓名".into(),
            // 如「签字人：李明辉」。要求「姓名/签字人/联系人…」等上下文，避免正文
            // 中零散的两字词被误报；捕获组 1 仅取姓名本体（见 detect.rs）。
            // 分隔符只允许冒号/空格/制表符，刻意排除换行（\s 含 \n）：表格单元格
            // 之间以换行分隔，「姓名\n身份证号」这类相邻表头不得跨单元格误粘为姓名。
            regex: r"(?:姓名|名字|签字人|簽字人|签署人|簽署人|联系人|聯絡人|聯繫人|负责人|負責人|经办人|經辦人|申请人|申請人|被申请人|收款人|付款人|授权代表|法定代表人|法人代表|委托代理人|委託代理人|委托诉讼代理人|诉讼代理人|法定代理人|代理人|委托人|受托人|项目经理|项目对接人|对接人|驻场人员|原告|被告|第三人|上诉人|被上诉人|申请执行人|被执行人|再审申请人|审判长|审判员|人民陪审员|书记员|法官助理|辩护人|公诉人|证人|鉴定人)[:： \t]*([\u4e00-\u9fa5&&[^诉被]]{2,4})".into(),
            enabled: true,
            confidence: 0.65,
            context_keywords: vec!["姓名".into(), "签字".into(), "联系人".into(), "负责人".into()],
        },
        RuleDef {
            id: ID_PHONE.into(),
            label: "固定电话".into(),
            regex: r"(?:[（(]0\d{2,3}[）)]|0\d{2,3})[\p{Zs}\t-]?\d{7,8}(?:-\d{1,5})?".into(),
            enabled: true,
            confidence: 0.9,
            context_keywords: vec!["电话".into(), "座机".into(), "传真".into()],
        },
        RuleDef {
            id: ID_PLATE.into(),
            label: "车牌号".into(),
            regex: r"[京津沪渝冀豫云辽黑湘皖鲁新苏浙赣鄂桂甘晋蒙陕吉闽贵粤青藏川宁琼][A-Z][A-Z0-9]{5,6}".into(),
            enabled: true,
            confidence: 0.88,
            context_keywords: vec!["车牌".into(), "车辆".into()],
        },
        RuleDef {
            id: ID_EMAIL.into(),
            label: "电子邮箱".into(),
            regex: r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}".into(),
            enabled: true,
            confidence: 0.97,
            context_keywords: vec!["邮箱".into(), "Email".into(), "E-mail".into()],
        },
        RuleDef {
            id: ID_IP.into(),
            label: "IP地址".into(),
            regex: r"(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)(?:\.(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)){3}".into(),
            enabled: true,
            confidence: 0.85,
            context_keywords: vec!["IP".into(), "地址".into()],
        },
        RuleDef {
            id: ID_URL.into(),
            label: "网址".into(),
            regex: r"https?://[^\s,;，。]+".into(),
            enabled: true,
            confidence: 0.9,
            context_keywords: vec!["网址".into(), "链接".into()],
        },
        RuleDef {
            id: ID_AMOUNT.into(),
            label: "金额".into(),
            // 四种形态：带货币符号（¥/￥）、带千分位的裸数字（如 480,000.00）、
            // 带「万/万元」单位（如 12.5万、300万元）、中文大写金额（如 人民币壹佰叁拾柒万贰仟元整）。
            // 裸数字必须含千分位或单位，避免把日期/编号/比例误判为金额（如 2026.08、0.05%）。
            regex: r"[¥￥]\s?\d+(?:,\d{3})*(?:\.\d{1,2})?(?:元)?|\d{1,3}(?:,\d{3})+(?:\.\d{1,2})?(?:元)?|\d+(?:\.\d{1,2})?万(?:元)?|\d+(?:\.\d{1,2})?元|(?:人民币)?[零壹贰叁肆伍陆柒捌玖拾佰仟万亿]{2,}(?:元|圆)(?:整|(?:[零壹贰叁肆伍陆柒捌玖]角)?(?:[零壹贰叁肆伍陆柒捌玖]分)?)".into(),
            enabled: true,
            confidence: 0.8,
            context_keywords: vec!["金额".into(), "价款".into(), "人民币".into(), "元".into()],
        },
        RuleDef {
            id: ID_CASE_NO.into(),
            label: "案号".into(),
            // 如「(2024)京0105民初12345号」「（2023）最高法民终123号」「〔2024〕京仲案字第0123号」。
            regex: r"[（(〔]\d{4}[）)〕][\u4e00-\u9fa5]{1,6}\d{0,5}[\u4e00-\u9fa5]{0,6}第?\d{1,7}号".into(),
            enabled: true,
            confidence: 0.95,
            context_keywords: vec!["案号".into(), "案由".into(), "受理".into(), "法院".into()],
        },
        RuleDef {
            id: ID_CONTRACT_NO.into(),
            label: "合同/业务编号".into(),
            // 要求「合同编号/协议编号/订单号/工号/病历号…」上下文；捕获组 1 为编号本体，
            // 且必须含至少一个数字（detect.rs 校验），避免把普通词当编号。
            regex: r"(?:合同编号|协议编号|合同号|订单编号|订单号|员工编号|工号|病历号|住院号|病案号|编号)[:： \t]*([A-Za-z0-9][A-Za-z0-9\-_/]{3,31})".into(),
            enabled: true,
            confidence: 0.8,
            context_keywords: vec!["合同".into(), "编号".into(), "订单".into()],
        },
        RuleDef {
            id: ID_ADDRESS.into(),
            label: "地址".into(),
            // 两种形态：①「住址/地址/送达地址…：」后的整段；②以省/直辖市开头、含路街村镇与门牌号的完整地址。
            regex: r"(?:住址|住所地?|地址|通讯地址|送达地址|经常居住地|注册地址|办公地址|联系地址|户籍地)[:：\s]*([^\n。；;，,]{6,60})|(?:北京|天津|上海|重庆|河北|山西|辽宁|吉林|黑龙江|江苏|浙江|安徽|福建|江西|山东|河南|湖北|湖南|广东|海南|四川|贵州|云南|陕西|甘肃|青海|台湾|内蒙古|广西|西藏|宁夏|新疆|香港|澳门)(?:省|市|自治区|特别行政区)?[一-龥]{1,12}?(?:路|街|大道|巷|弄|村|镇|乡)[一-龥A-Za-z0-9\-]{0,10}?\d{1,5}号(?:院)?(?:\d{1,4}(?:栋|幢|座|号楼|楼))?(?:\d{1,4}(?:单元|室|层))*".into(),
            enabled: true,
            confidence: 0.75,
            context_keywords: vec!["住址".into(), "地址".into(), "住所".into()],
        },
        RuleDef {
            id: ID_BANK_ACCOUNT.into(),
            label: "银行账号".into(),
            // 带「账号/账户/对公账号…」上下文的 8–25 位账号（含对公账号 17/21 位等无法靠卡号规则覆盖的长度）。
            // 捕获组 1 为账号本体；位数在 detect.rs 校验。
            regex: r"(?:对公账号|公司账号|银行账号|开户账号|收款账号|付款账号|账号|账户|帐号|帐户)[:：\s]*([0-9][0-9 \-]{6,28}[0-9])".into(),
            enabled: true,
            confidence: 0.85,
            context_keywords: vec!["账号".into(), "账户".into(), "开户".into()],
        },
        RuleDef {
            id: ID_PASSPORT.into(),
            label: "护照/通行证".into(),
            // 要求「护照/港澳通行证/台胞证」上下文，避免把普通编码当证件号；捕获组 1 为证件号本体。
            regex: r"(?:护照号码|护照号|护照|港澳居民来往内地通行证|港澳通行证|往来港澳通行证|台湾居民来往大陆通行证|台胞证|通行证号|通行证)[:：\s]*([A-Za-z]{1,2}[0-9]{7,9})".into(),
            enabled: true,
            confidence: 0.9,
            context_keywords: vec!["护照".into(), "通行证".into(), "证件".into()],
        },
        RuleDef {
            id: ID_ID_CARD_HMT.into(),
            label: "港澳台证件号".into(),
            // 香港身份证 A123456(7)；台湾身份证（字号）需「身份证/身分證」上下文，捕获组 1 为证件号本体。
            regex: r"[A-Z]{1,2}\d{6}\([0-9A]\)|(?:身份证|身份證|身分證)(?:字号|字號|号码|號碼|号|號)?[:：\s]*([A-Z][12]\d{8})".into(),
            enabled: true,
            confidence: 0.9,
            context_keywords: vec!["身份".into(), "身分".into(), "港".into(), "台".into()],
        },
        RuleDef {
            id: ID_DATE.into(),
            label: "日期".into(),
            regex: r"\d{4}-\d{1,2}-\d{1,2}|\d{4}\s*年\s*\d{1,2}\s*月\s*\d{1,2}\s*日".into(),
            enabled: true,
            confidence: 0.75,
            context_keywords: vec!["日期".into(), "签订".into(), "生效".into()],
        },
    ]
}

/// 18 位身份证校验位算法：加权求和取模 11 映射校验码
pub fn id_card_check_code(digits17: &str) -> char {
    const WEIGHTS: [u32; 17] = [7, 9, 10, 5, 8, 4, 2, 1, 6, 3, 7, 9, 10, 5, 8, 4, 2];
    const CODES: [char; 11] = ['1', '0', 'X', '9', '8', '7', '6', '5', '4', '3', '2'];
    let sum: u32 = digits17
        .chars()
        .zip(WEIGHTS.iter())
        .map(|(c, w)| c.to_digit(10).unwrap_or(0) * w)
        .sum();
    CODES[(sum % 11) as usize]
}

/// 全角数字归一化为半角（０-９ -> 0-9），其余字符原样保留。
///
/// regex crate 的 `\d` 为 Unicode-aware（可匹配全角数字），校验器若按字节
/// 长度/ASCII 过滤处理会把全角形态的证件号静默判非法（漏检）。
fn normalize_fullwidth_digits(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{FF10}'..='\u{FF19}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            _ => c,
        })
        .collect()
}

/// 校验身份证号（15 位仅格式，18 位做校验位验证）。
///
/// 全角数字先归一化再校验；位数按字符数判断（全角串的字节数是字符数的 3 倍，
/// 不可用 `len()`）。
pub fn is_valid_id_card(s: &str) -> bool {
    let s = normalize_fullwidth_digits(s);
    match s.chars().count() {
        15 => s.chars().all(|c| c.is_ascii_digit()) && is_valid_birth15(&s[6..12]),
        18 => {
            let (digits17, last) = s.split_at(17);
            if !digits17.chars().all(|c| c.is_ascii_digit()) {
                return false;
            }
            if !is_valid_birth18(&s[6..14]) {
                return false;
            }
            let expected = id_card_check_code(digits17);
            last.to_ascii_uppercase() == expected.to_string()
        }
        _ => false,
    }
}

fn is_valid_birth15(yymmdd: &str) -> bool {
    let yy: u32 = yymmdd[0..2].parse().unwrap_or(0);
    (50..=99).contains(&yy) || (0..=49).contains(&yy) // 旧证不严格校验，仅格式
}

fn is_valid_birth18(yyyymmdd: &str) -> bool {
    let y: u32 = yyyymmdd[0..4].parse().unwrap_or(0);
    let m: u32 = yyyymmdd[4..6].parse().unwrap_or(0);
    let d: u32 = yyyymmdd[6..8].parse().unwrap_or(0);
    (1900..=2100).contains(&y) && (1..=12).contains(&m) && (1..=31).contains(&d)
}

/// Luhn 校验（银行卡号）。全角数字先归一化（此前被 `is_ascii_digit` 过滤
/// 直接丢弃，导致含全角位的卡号校验对象错位）。
pub fn luhn_valid(digits: &str) -> bool {
    let clean: String = normalize_fullwidth_digits(digits)
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect();
    if clean.len() < 6 || clean.len() > 19 {
        return false;
    }
    let bytes: Vec<u32> = clean.chars().map(|c| c.to_digit(10).unwrap_or(0)).collect();
    let sum: u32 = bytes
        .iter()
        .rev()
        .enumerate()
        .fold(0u32, |mut acc, (i, &d)| {
            let mut v = d;
            if i % 2 == 1 {
                v *= 2;
                if v > 9 {
                    v -= 9;
                }
            }
            acc += v;
            acc
        });
    sum.is_multiple_of(10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn luhn_accepts_valid_bank_card() {
        // Visa / Mastercard 官方测试卡号（Luhn 合法）
        assert!(luhn_valid("4111111111111111"));
        assert!(luhn_valid("5555555555554444"));
    }

    #[test]
    fn luhn_rejects_invalid() {
        assert!(!luhn_valid("4111111111111112"));
        assert!(!luhn_valid("12345"));
    }

    #[test]
    fn id_card_valid_18() {
        // 自动构造合法号码：固定前 17 位 + 计算校验位
        let check = id_card_check_code("11010119840320123");
        let full = format!("11010119840320123{check}");
        assert!(is_valid_id_card(&full));
    }

    #[test]
    fn id_card_invalid_check_digit() {
        assert!(!is_valid_id_card("110101198403201233"));
        assert!(!is_valid_id_card("11010119840320123X")); // X 不在第 18 位时非法
        assert!(!is_valid_id_card("12345678901234567")); // 17 位非法
    }

    #[test]
    fn id_card_fullwidth_digits_normalized() {
        // 全角形态的合法身份证必须通过校验（regex \d 是 Unicode-aware 的，
        // 此前按字节长度判断会静默漏检）
        let full = "11010119840320123";
        let check = id_card_check_code(full);
        let half = format!("{full}{check}");
        let fullwidth: String = half
            .chars()
            .map(|c| char::from_u32(c as u32 + 0xFEE0).unwrap_or(c))
            .collect();
        assert_eq!(fullwidth.chars().count(), 18);
        assert!(is_valid_id_card(&fullwidth), "全角身份证应校验通过: {fullwidth}");
        // 校验位错误的全角串仍应被拒绝
        let bad: String = "110101198403201233"
            .chars()
            .map(|c| char::from_u32(c as u32 + 0xFEE0).unwrap_or(c))
            .collect();
        assert!(!is_valid_id_card(&bad));
    }

    #[test]
    fn luhn_fullwidth_digits_normalized() {
        // 全角卡号与半角卡号校验结果一致
        let half = "4111111111111111";
        let fullwidth: String = half
            .chars()
            .map(|c| char::from_u32(c as u32 + 0xFEE0).unwrap_or(c))
            .collect();
        assert!(luhn_valid(half));
        assert!(luhn_valid(&fullwidth), "全角卡号应 Luhn 通过: {fullwidth}");
    }

    #[test]
    fn builtin_rules_all_compilable() {
        for r in builtin_rules() {
            let re = regex::Regex::new(&r.regex).expect("规则正则应可编译");
            assert!(!re.as_str().is_empty());
        }
    }
}
