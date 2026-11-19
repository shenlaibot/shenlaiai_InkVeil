//! 项目保险库（Vault）：按案件/项目持久保存映射表与敏感词典，跨批次编号稳定。
//!
//! 为什么需要：此前每次导出都新建映射表（占位符编号从 1 重新开始）并覆盖输出目录里的同名映射文件，
//! 同一个人在不同批次里可能是 `[姓名1]` 也可能是 `[姓名7]`，用错映射表还原会**静默还原成别人的原文**。
//!
//! 文件格式 `.akcase`（JSON 信封，敏感内容全部在密文里）：
//! ```text
//! { "magic":"ANONKIT-VAULT", "version":1, "project_id":"<16 字节随机 hex>",
//!   "salt":"<base64>", "rounds":600000, "payload":"AES:<iv>:<ct>" }
//! ```
//! payload 解密后是 [`VaultData`] 的 JSON（项目名、词典、映射表、批次记录都在其中，项目名也不外露）。
//!
//! 可靠性：
//! - 保存 = 写临时文件 → fsync → 原子改名；覆盖前保留上一版为 `.bak`（崩溃/写坏可回滚）；
//! - 口令只用于派生一次密钥并驻留内存，每次保存只做 AES（不重复 PBKDF2）；
//! - 口令错误与文件损坏都会被 AES-GCM 认证拦截，不会得到被篡改的映射。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};

use crate::crypto::{aes_decrypt, aes_encrypt, derive_key};
use crate::mapping::MappingTable;

const MAGIC: &str = "ANONKIT-VAULT";
const VERSION: u32 = 1;
const ROUNDS: u32 = 600_000;
const SALT_LEN: usize = 16;

/// 一次导出批次的记录（不含任何原文）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BatchRecord {
    /// 批次序号（从 1 递增）
    pub seq: u32,
    /// Unix 秒
    pub time: u64,
    /// 本批处理的文件数
    pub files: usize,
    /// 本批新增的映射条目数
    pub entries_added: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct Envelope {
    magic: String,
    version: u32,
    project_id: String,
    salt: String,
    rounds: u32,
    payload: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct VaultData {
    name: String,
    created: u64,
    /// `MappingTable::to_json` 的结果
    mapping: String,
    dict_terms: Vec<String>,
    batches: Vec<BatchRecord>,
    /// 保结构替换参数（每币种缩放系数 k、日期平移天数 Δd、种子）。随机生成、不由原文派生；
    /// 旧项目文件没有此字段（向后兼容）。“原文→替换值”对照在 `mapping` 里。
    #[serde(default)]
    params: Option<crate::shift::ShiftParams>,
}

pub struct Vault {
    pub path: PathBuf,
    pub project_id: String,
    pub name: String,
    pub created: u64,
    pub mapping: MappingTable,
    pub dict_terms: Vec<String>,
    pub batches: Vec<BatchRecord>,
    salt: Vec<u8>,
    rounds: u32,
    key: [u8; 32],
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Vault {
    /// 新建项目文件。目标已存在则报错（绝不覆盖已有项目）。
    pub fn create(path: &Path, name: &str, passphrase: &str) -> Result<Vault, String> {
        if passphrase.is_empty() {
            return Err("项目口令不能为空".into());
        }
        if path.exists() {
            return Err(format!("文件已存在，不会覆盖: {}", path.display()));
        }
        let mut salt = vec![0u8; SALT_LEN];
        OsRng.fill_bytes(&mut salt);
        let mut id = [0u8; 16];
        OsRng.fill_bytes(&mut id);
        let key = derive_key(passphrase, &salt, ROUNDS);
        let v = Vault {
            path: path.to_path_buf(),
            project_id: hex(&id),
            name: name.trim().to_string(),
            created: now_secs(),
            mapping: MappingTable::new(),
            dict_terms: Vec::new(),
            batches: Vec::new(),
            salt,
            rounds: ROUNDS,
            key,
        };
        v.save()?;
        Ok(v)
    }

    /// 打开项目。口令错误/文件被篡改都返回错误（AES-GCM 认证）。
    pub fn open(path: &Path, passphrase: &str) -> Result<Vault, String> {
        let text = fs::read_to_string(path).map_err(|e| format!("读取项目文件失败: {e}"))?;
        let env: Envelope =
            serde_json::from_str(&text).map_err(|_| "不是有效的项目文件（格式无法识别）".to_string())?;
        if env.magic != MAGIC {
            return Err("不是有效的项目文件（标识不符）".into());
        }
        if env.version > VERSION {
            return Err(format!("项目文件版本 {} 高于本软件支持的 {VERSION}，请升级软件", env.version));
        }
        let salt = B64.decode(&env.salt).map_err(|_| "项目文件已损坏（盐无法解码）".to_string())?;
        let key = derive_key(passphrase, &salt, env.rounds);
        let json = aes_decrypt(&env.payload, &key).map_err(|_| "口令错误，或项目文件已损坏".to_string())?;
        let data: VaultData =
            serde_json::from_str(&json).map_err(|_| "项目数据损坏（解密成功但内容无法解析）".to_string())?;
        let mut mapping = MappingTable::new();
        mapping.from_json(&data.mapping)?;
        if let Some(p) = data.params {
            mapping.shift_book_mut().adopt_params(p);
        }
        Ok(Vault {
            path: path.to_path_buf(),
            project_id: env.project_id,
            name: data.name,
            created: data.created,
            mapping,
            dict_terms: data.dict_terms,
            batches: data.batches,
            salt,
            rounds: env.rounds,
            key,
        })
    }

    /// 原子保存：临时文件 → fsync → 保留上一版为 `.bak` → 改名覆盖。
    pub fn save(&self) -> Result<(), String> {
        let data = VaultData {
            name: self.name.clone(),
            created: self.created,
            mapping: self.mapping.to_json(),
            dict_terms: self.dict_terms.clone(),
            batches: self.batches.clone(),
            params: self.mapping.shift_book().params.clone(),
        };
        let json = serde_json::to_string(&data).map_err(|e| e.to_string())?;
        let payload = aes_encrypt(&json, &self.key).map_err(|e| e.to_string())?;
        let env = Envelope {
            magic: MAGIC.into(),
            version: VERSION,
            project_id: self.project_id.clone(),
            salt: B64.encode(&self.salt),
            rounds: self.rounds,
            payload,
        };
        let bytes = serde_json::to_vec(&env).map_err(|e| e.to_string())?;

        let dir = self.path.parent().unwrap_or_else(|| Path::new("."));
        let tmp = dir.join(format!(
            ".{}.tmp",
            self.path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
        ));
        {
            let mut f = fs::File::create(&tmp).map_err(|e| format!("写入临时文件失败: {e}"))?;
            f.write_all(&bytes).map_err(|e| format!("写入失败: {e}"))?;
            f.sync_all().map_err(|e| format!("落盘失败: {e}"))?;
        }
        if self.path.exists() {
            let bak = self.path.with_extension("akcase.bak");
            let _ = fs::copy(&self.path, &bak);
        }
        fs::rename(&tmp, &self.path).map_err(|e| format!("替换项目文件失败: {e}"))?;
        Ok(())
    }

    /// 记录一次批次（调用方传入批次前的映射条目数）。
    pub fn record_batch(&mut self, files: usize, entries_before: usize) {
        let seq = self.batches.last().map(|b| b.seq + 1).unwrap_or(1);
        self.batches.push(BatchRecord {
            seq,
            time: now_secs(),
            files,
            entries_added: self.mapping.len().saturating_sub(entries_before),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("anonkit_vault_{}_{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d.join("case.akcase")
    }

    #[test]
    fn placeholders_stable_across_sessions() {
        let p = tmp("stable");
        let mut v = Vault::create(&p, "甲公司并购案", "pw1").unwrap();
        let a = v.mapping.placeholder_for("姓名", "陈瑾瑜");
        let b = v.mapping.placeholder_for("公司", "北京云图科技有限公司");
        v.save().unwrap();
        drop(v);

        // 第二批：重新打开，已有实体编号不变，新实体接着编号
        let mut v2 = Vault::open(&p, "pw1").unwrap();
        assert_eq!(v2.mapping.placeholder_for("姓名", "陈瑾瑜"), a);
        assert_eq!(v2.mapping.placeholder_for("公司", "北京云图科技有限公司"), b);
        let c = v2.mapping.placeholder_for("姓名", "李国强");
        assert_ne!(c, a, "新实体不得复用旧编号");
        assert_eq!(c, "[姓名2]");
        v2.save().unwrap();
        // 用第二批保存后的映射还原第一批产物
        let v3 = Vault::open(&p, "pw1").unwrap();
        assert_eq!(v3.mapping.restore(&format!("联系人{a}，{c}")), "联系人陈瑾瑜，李国强");
        assert_eq!(v3.name, "甲公司并购案");
    }

    #[test]
    fn wrong_passphrase_and_tamper_rejected() {
        let p = tmp("auth");
        let mut v = Vault::create(&p, "x", "right").unwrap();
        v.mapping.placeholder_for("姓名", "陈瑾瑜");
        v.save().unwrap();
        assert!(Vault::open(&p, "wrong").is_err());
        // 篡改密文
        let s = fs::read_to_string(&p).unwrap();
        let tampered = s.replacen("AES:", "AES:AAAA", 1);
        fs::write(&p, tampered).unwrap();
        assert!(Vault::open(&p, "right").is_err(), "被篡改的文件必须被拒绝");
    }

    #[test]
    fn file_contains_no_plaintext_and_is_not_overwritten_on_create() {
        let p = tmp("plain");
        let mut v = Vault::create(&p, "机密项目名", "pw").unwrap();
        v.mapping.placeholder_for("姓名", "陈瑾瑜");
        v.dict_terms = vec!["星辰计划".into()];
        v.save().unwrap();
        let raw = fs::read_to_string(&p).unwrap();
        for secret in ["陈瑾瑜", "机密项目名", "星辰计划"] {
            assert!(!raw.contains(secret), "文件中不得出现明文: {secret}");
        }
        assert!(Vault::create(&p, "again", "pw").is_err(), "已存在的项目不得被覆盖");
    }

    #[test]
    fn save_keeps_backup_and_batches_recorded() {
        let p = tmp("bak");
        let mut v = Vault::create(&p, "x", "pw").unwrap();
        let before = v.mapping.len();
        v.mapping.placeholder_for("姓名", "甲");
        v.mapping.placeholder_for("姓名", "乙");
        v.record_batch(3, before);
        v.save().unwrap();
        v.record_batch(1, v.mapping.len());
        v.save().unwrap();
        assert!(p.with_extension("akcase.bak").exists(), "覆盖前应保留上一版备份");
        let o = Vault::open(&p, "pw").unwrap();
        assert_eq!(o.batches.len(), 2);
        assert_eq!(o.batches[0], BatchRecord { seq: 1, time: o.batches[0].time, files: 3, entries_added: 2 });
        assert_eq!(o.batches[1].seq, 2);
        // 备份可独立打开（回滚）
        assert!(Vault::open(&p.with_extension("akcase.bak"), "pw").is_ok());
    }

    #[test]
    fn shift_params_and_entries_persist_with_project() {
        let p = tmp("shift");
        let mut v = Vault::create(&p, "x", "pw").unwrap();
        let r = v.mapping.shift_book_mut().scale_amount("100万元", "", "").unwrap();
        let d = v.mapping.shift_book_mut().shift_date("2024-03-01", "", "").unwrap();
        let params = v.mapping.shift_book().params.clone().unwrap();
        v.save().unwrap();
        let mut o = Vault::open(&p, "pw").unwrap();
        assert_eq!(o.mapping.shift_book().params.as_ref(), Some(&params));
        // 跨批次：同一原文得到同一替换值，且可精确还原
        assert_eq!(o.mapping.shift_book_mut().scale_amount("100万元", "", "").unwrap(), r);
        assert_eq!(o.mapping.shift_book().restore_exact(&format!("{r}，{d}")), "100万元，2024-03-01");
        // 原文不得以明文出现在文件里
        assert!(!fs::read_to_string(&p).unwrap().contains("100万元"));
    }

    #[test]
    fn old_project_without_params_still_opens() {
        let d: VaultData = serde_json::from_str(
            r#"{"name":"x","created":1,"mapping":"{\"version\":1,\"entries\":[],\"counters\":{}}","dict_terms":[],"batches":[]}"#,
        )
        .unwrap();
        assert!(d.params.is_none());
    }

    #[test]
    fn dictionary_persists_and_empty_passphrase_rejected() {
        let p = tmp("dict");
        assert!(Vault::create(&p, "x", "").is_err());
        let mut v = Vault::create(&p, "x", "pw").unwrap();
        v.dict_terms = vec!["星辰计划".into(), "Project Phoenix".into()];
        v.save().unwrap();
        assert_eq!(Vault::open(&p, "pw").unwrap().dict_terms, vec!["星辰计划", "Project Phoenix"]);
    }
}
