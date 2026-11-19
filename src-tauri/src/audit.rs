//! 审计日志与应用配置的本地持久化（SQLite，bundled）。
//!
//! 存储位置：应用数据目录 `anonkit.db`，单用户单机、本地零外传。
//! 表：
//! - `audit_log`：脱敏操作审计，带哈希链（seq / prev_hash / hash），防篡改
//! - `audit_meta`：审计元数据（保存期限、链头锚点）
//! - `app_config`：应用配置 JSON（键值，单键 `main`）
//!
//! 哈希链：`hash = SHA256(prev_hash ‖ 规范化字段)`，由后端写入时计算。
//! 改某条 → 该条哈希不符；删中间一条 → 下一条 prev_hash 对不上；
//! 删尾部 → 与 `audit_meta` 中的链头锚点不符。
//! 不提供“清空”；只能按保存期限从链头修剪最早的连续区段，并写入不可删除的“修剪记录”
//! （含被删区段的条数、序号范围、根哈希），链仍可校验。
//! 局限：攻击者若同时重算整条链并改写锚点则无法发现（无外部可信根），
//! 本机单用户场景下目标是“发现无意或事后的改动”，而非对抗拥有完全写权限的人。

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Mutex;

/// 创世 prev_hash。
pub const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";
/// 默认保存期限（天）：3 年。
pub const DEFAULT_RETENTION_DAYS: i64 = 1095;

pub const KIND_ENTRY: &str = "entry";
pub const KIND_PRUNE: &str = "prune";
pub const KIND_MIGRATION: &str = "migration";

/// 审计日志条目（与前端 AuditEntry 对齐）。链字段由后端写入时计算，前端传入的会被忽略。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AuditEntryDto {
    #[serde(default)]
    pub seq: i64,
    /// entry（脱敏操作）/ prune（修剪记录）/ migration（旧库补链标记）
    #[serde(default)]
    pub kind: String,
    pub time: String,
    pub file: String,
    pub hit_count: i64,
    pub types: String,
    pub output: String,
    pub mapping: String,
    pub is_deep: bool,
    /// 例外放行理由等备注（只存类别与简短说明，不得含原文）
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub prev_hash: String,
    #[serde(default)]
    pub hash: String,
}

/// 链校验结果。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AuditVerifyResult {
    pub ok: bool,
    /// 当前库内记录数
    pub count: i64,
    /// 首个断点序号（ok 时为 None）
    pub broken_seq: Option<i64>,
    /// 断点原因（ok 时为空）
    pub reason: String,
    /// 累计已按期限修剪的记录数（来自修剪记录）
    pub pruned_total: i64,
}

/// 修剪结果。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AuditPruneResult {
    pub removed: i64,
    pub root_hash: String,
}

fn canonical(e: &AuditEntryDto) -> String {
    // 长度前缀，避免字段拼接歧义
    let mut out = String::new();
    let seq = e.seq.to_string();
    let hit = e.hit_count.to_string();
    let deep = if e.is_deep { "1" } else { "0" };
    for f in [
        seq.as_str(),
        e.kind.as_str(),
        e.time.as_str(),
        e.file.as_str(),
        hit.as_str(),
        e.types.as_str(),
        e.output.as_str(),
        e.mapping.as_str(),
        deep,
        e.note.as_str(),
    ] {
        out.push_str(&f.len().to_string());
        out.push(':');
        out.push_str(f);
        out.push(';');
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `SHA256(prev_hash ‖ 规范化字段)`。
pub fn compute_hash(prev_hash: &str, e: &AuditEntryDto) -> String {
    let mut h = Sha256::new();
    h.update(prev_hash.as_bytes());
    h.update(canonical(e).as_bytes());
    hex(&h.finalize())
}

/// 解析 "2026-08-11 10:00" / "2026/8/11 10:00:00" 的日期部分为天数序号（自 1970-01-01）。
fn parse_day(time: &str) -> Option<i64> {
    let nums: Vec<i64> = time
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .take(3)
        .filter_map(|s| s.parse().ok())
        .collect();
    if nums.len() < 3 || !(1..=12).contains(&nums[1]) || !(1..=31).contains(&nums[2]) {
        return None;
    }
    Some(days_from_civil(nums[0], nums[1], nums[2]))
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 当前时间字符串（UTC，供系统生成的记录使用）。
fn now_string() -> String {
    let secs = now_secs();
    let (y, m, d) = civil_from_days(secs.div_euclid(86400));
    let rem = secs.rem_euclid(86400);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rem / 3600, rem % 3600 / 60)
}

const COLS: &str = "seq, kind, time, file, hit_count, types, output, mapping, is_deep, note, prev_hash, hash";

fn row_to_dto(row: &rusqlite::Row<'_>) -> rusqlite::Result<AuditEntryDto> {
    Ok(AuditEntryDto {
        seq: row.get(0)?,
        kind: row.get(1)?,
        time: row.get(2)?,
        file: row.get(3)?,
        hit_count: row.get(4)?,
        types: row.get(5)?,
        output: row.get(6)?,
        mapping: row.get(7)?,
        is_deep: row.get::<_, i64>(8)? != 0,
        note: row.get(9)?,
        prev_hash: row.get(10)?,
        hash: row.get(11)?,
    })
}

fn meta_get(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM audit_meta WHERE key = ?1", params![key], |r| r.get(0))
        .optional()
}

fn meta_set(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO audit_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// 在已持锁的连接上追加一条链记录（调用方负责事务）。
fn append(conn: &Connection, kind: &str, mut e: AuditEntryDto) -> rusqlite::Result<AuditEntryDto> {
    let (last_seq, last_hash): (i64, String) = match meta_get(conn, "head_seq")? {
        Some(s) => (
            s.parse().unwrap_or(0),
            meta_get(conn, "head_hash")?.unwrap_or_else(|| GENESIS.to_string()),
        ),
        None => (0, GENESIS.to_string()),
    };
    e.seq = last_seq + 1;
    e.kind = kind.to_string();
    e.prev_hash = last_hash;
    e.hash = compute_hash(&e.prev_hash, &e);
    conn.execute(
        &format!("INSERT INTO audit_log ({COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)"),
        params![
            e.seq, e.kind, e.time, e.file, e.hit_count, e.types, e.output, e.mapping,
            e.is_deep as i64, e.note, e.prev_hash, e.hash
        ],
    )?;
    meta_set(conn, "head_seq", &e.seq.to_string())?;
    meta_set(conn, "head_hash", &e.hash)?;
    Ok(e)
}

/// SQLite 审计/配置仓库。`Mutex` 保证并发访问安全（Connection 非 Sync）。
pub struct AuditDb {
    conn: Mutex<Connection>,
}

impl AuditDb {
    /// 打开（或创建）指定路径的数据库并初始化表结构；打开后按保存期限自动修剪。
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self { conn: Mutex::new(conn) };
        db.init()?;
        let days = db.retention_days()?;
        let _ = db.prune(days); // 修剪失败不阻断启动
        Ok(db)
    }

    /// 内存数据库（测试用）。
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn: Mutex::new(conn) };
        db.init()?;
        Ok(db)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn init(&self) -> rusqlite::Result<()> {
        let conn = self.lock();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS audit_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                time TEXT NOT NULL,
                file TEXT NOT NULL,
                hit_count INTEGER NOT NULL DEFAULT 0,
                types TEXT NOT NULL DEFAULT '',
                output TEXT NOT NULL DEFAULT '',
                mapping TEXT NOT NULL DEFAULT '',
                is_deep INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS audit_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS app_config (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            PRAGMA journal_mode = WAL;",
        )?;
        // 迁移：旧库缺少的列逐个补上（已有则忽略错误）
        for ddl in [
            "ALTER TABLE audit_log ADD COLUMN note TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE audit_log ADD COLUMN seq INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE audit_log ADD COLUMN kind TEXT NOT NULL DEFAULT 'entry'",
            "ALTER TABLE audit_log ADD COLUMN prev_hash TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE audit_log ADD COLUMN hash TEXT NOT NULL DEFAULT ''",
        ] {
            let _ = conn.execute(ddl, []);
        }
        Self::migrate_legacy(&conn)
    }

    /// 给尚未入链的历史记录（hash 为空）补链，并在末尾追加“迁移点”记录。
    /// 补链只能证明“迁移之后未被改动”，迁移前的内容无法证明。
    fn migrate_legacy(conn: &Connection) -> rusqlite::Result<()> {
        let legacy: i64 = conn.query_row(
            "SELECT COUNT(*) FROM audit_log WHERE hash = ''",
            [],
            |r| r.get(0),
        )?;
        if legacy == 0 {
            return Ok(());
        }
        let tx = conn.unchecked_transaction()?;
        // 取出全部历史记录（按 id），用新的连续 seq 重建整条链
        let rows: Vec<AuditEntryDto> = {
            let mut stmt = tx.prepare(&format!(
                "SELECT {COLS} FROM audit_log ORDER BY id ASC"
            ))?;
            let v = stmt.query_map([], row_to_dto)?.collect::<rusqlite::Result<Vec<_>>>()?;
            v
        };
        tx.execute("DELETE FROM audit_log", [])?;
        tx.execute("DELETE FROM audit_meta WHERE key IN ('head_seq','head_hash')", [])?;
        for r in rows {
            let kind = if r.kind.is_empty() { KIND_ENTRY.to_string() } else { r.kind.clone() };
            append(&tx, &kind, r)?;
        }
        append(
            &tx,
            KIND_MIGRATION,
            AuditEntryDto {
                time: now_string(),
                file: "（系统）".into(),
                note: format!("旧库升级：为 {legacy} 条历史记录补建哈希链，此点之前的记录无法证明未被改动"),
                ..Default::default()
            },
        )?;
        tx.commit()
    }

    /// 追加一条审计记录（链字段在此计算，传入值被忽略）。
    pub fn insert(&self, e: &AuditEntryDto) -> rusqlite::Result<AuditEntryDto> {
        let conn = self.lock();
        let tx = conn.unchecked_transaction()?;
        let out = append(&tx, KIND_ENTRY, e.clone())?;
        tx.commit()?;
        Ok(out)
    }

    /// 读取审计记录（按序号倒序，最多 `limit` 条，默认 500）。
    pub fn list(&self, limit: Option<i64>) -> rusqlite::Result<Vec<AuditEntryDto>> {
        let limit = limit.unwrap_or(500).clamp(1, 5000);
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLS} FROM audit_log ORDER BY seq DESC LIMIT ?1"
        ))?;
        let rows = stmt.query_map(params![limit], row_to_dto)?;
        rows.collect()
    }

    /// 审计记录总数。
    pub fn count(&self) -> rusqlite::Result<i64> {
        let conn = self.lock();
        conn.query_row("SELECT COUNT(*) FROM audit_log", [], |row| row.get(0))
    }

    /// 保存期限（天），默认 3 年。
    pub fn retention_days(&self) -> rusqlite::Result<i64> {
        let conn = self.lock();
        Ok(meta_get(&conn, "retention_days")?
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_RETENTION_DAYS))
    }

    pub fn set_retention_days(&self, days: i64) -> rusqlite::Result<()> {
        let conn = self.lock();
        meta_set(&conn, "retention_days", &days.clamp(30, 36500).to_string())
    }

    /// 校验整条链。
    pub fn verify(&self) -> rusqlite::Result<AuditVerifyResult> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM audit_log ORDER BY seq ASC"))?;
        let rows = stmt.query_map([], row_to_dto)?.collect::<rusqlite::Result<Vec<_>>>()?;
        let count = rows.len() as i64;
        let mut pruned_total = 0i64;
        let fail = |seq: i64, reason: String, pruned: i64| AuditVerifyResult {
            ok: false,
            count,
            broken_seq: Some(seq),
            reason,
            pruned_total: pruned,
        };
        let mut prev: Option<&AuditEntryDto> = None;
        for r in &rows {
            if let Some(p) = prev {
                if r.seq != p.seq + 1 {
                    return Ok(fail(p.seq + 1, format!("序号不连续：第 {} 条之后缺失记录（疑似被删除）", p.seq), pruned_total));
                }
                if r.prev_hash != p.hash {
                    return Ok(fail(r.seq, "与上一条的链接断裂（上一条被删除或替换）".into(), pruned_total));
                }
            }
            // 首条的 prev_hash 作为锚点：创世或最近一次修剪的末条哈希
            if compute_hash(&r.prev_hash, r) != r.hash {
                return Ok(fail(r.seq, "内容与哈希不符（记录被修改）".into(), pruned_total));
            }
            if r.kind == KIND_PRUNE {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&r.note) {
                    pruned_total += v["removed"].as_i64().unwrap_or(0);
                }
            }
            prev = Some(r);
        }
        // 链头锚点：发现尾部被截断
        if let (Some(last), Some(hs)) = (prev, meta_get(&conn, "head_seq")?) {
            let hh = meta_get(&conn, "head_hash")?.unwrap_or_default();
            if hs != last.seq.to_string() || hh != last.hash {
                return Ok(fail(
                    hs.parse::<i64>().unwrap_or(last.seq),
                    "链尾与锚点不符（末尾记录被删除或替换）".into(),
                    pruned_total,
                ));
            }
        } else if prev.is_none() {
            if let Some(hs) = meta_get(&conn, "head_seq")? {
                if hs != "0" {
                    return Ok(fail(1, "记录全部丢失，但锚点显示曾有记录".into(), 0));
                }
            }
        }
        Ok(AuditVerifyResult { ok: true, count, broken_seq: None, reason: String::new(), pruned_total })
    }

    /// 按保存期限修剪：只删除链头开始、连续且已过期的区段，并追加不可删除的修剪记录。
    /// 返回 None 表示无需修剪。
    pub fn prune(&self, retention_days: i64) -> rusqlite::Result<Option<AuditPruneResult>> {
        self.prune_at(retention_days, now_secs().div_euclid(86400))
    }

    fn prune_at(&self, retention_days: i64, today: i64) -> rusqlite::Result<Option<AuditPruneResult>> {
        let cutoff = today - retention_days.max(30);
        let conn = self.lock();
        let tx = conn.unchecked_transaction()?;
        let rows: Vec<AuditEntryDto> = {
            let mut stmt = tx.prepare(&format!("SELECT {COLS} FROM audit_log ORDER BY seq ASC"))?;
            let v = stmt.query_map([], row_to_dto)?.collect::<rusqlite::Result<Vec<_>>>()?;
            v
        };
        let expired: Vec<&AuditEntryDto> = rows
            .iter()
            .take_while(|r| parse_day(&r.time).map_or(false, |d| d < cutoff))
            .collect();
        if expired.is_empty() {
            return Ok(None);
        }
        let mut root = Sha256::new();
        for r in &expired {
            root.update(r.hash.as_bytes());
        }
        let root_hash = hex(&root.finalize());
        let (first, last) = (expired[0], expired[expired.len() - 1]);
        let removed = expired.len() as i64;
        tx.execute("DELETE FROM audit_log WHERE seq <= ?1", params![last.seq])?;
        let note = serde_json::json!({
            "removed": removed,
            "from_seq": first.seq,
            "to_seq": last.seq,
            "root_hash": root_hash,
            "last_hash": last.hash,
            "retention_days": retention_days,
        })
        .to_string();
        append(
            &tx,
            KIND_PRUNE,
            AuditEntryDto {
                time: now_string(),
                file: "（系统）".into(),
                note,
                ..Default::default()
            },
        )?;
        tx.commit()?;
        Ok(Some(AuditPruneResult { removed, root_hash }))
    }

    /// 读取应用配置 JSON（无则返回 None）。
    pub fn config_get(&self) -> rusqlite::Result<Option<String>> {
        let conn = self.lock();
        conn.query_row(
            "SELECT value FROM app_config WHERE key = 'main'",
            [],
            |row| row.get(0),
        )
        .optional()
    }

    /// 覆盖写入应用配置 JSON。
    pub fn config_set(&self, value: &str) -> rusqlite::Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES ('main', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![value],
        )?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// 读取审计记录（最新在前，最多 500 条）。
#[tauri::command]
pub fn audit_list(
    state: tauri::State<'_, AuditDb>,
    limit: Option<i64>,
) -> Result<Vec<AuditEntryDto>, String> {
    state.list(limit).map_err(|e| format!("读取审计日志失败: {e}"))
}

/// 追加一条审计记录（链哈希由后端计算）。
#[tauri::command]
pub fn audit_add(state: tauri::State<'_, AuditDb>, entry: AuditEntryDto) -> Result<(), String> {
    state
        .insert(&entry)
        .map(|_| ())
        .map_err(|e| format!("写入审计日志失败: {e}"))
}

/// 校验审计哈希链。
#[tauri::command]
pub fn audit_verify(state: tauri::State<'_, AuditDb>) -> Result<AuditVerifyResult, String> {
    state.verify().map_err(|e| format!("校验审计日志失败: {e}"))
}

/// 按保存期限修剪（不可清空；会留下修剪记录）。
#[tauri::command]
pub fn audit_prune(
    state: tauri::State<'_, AuditDb>,
    retention_days: i64,
) -> Result<i64, String> {
    state
        .prune(retention_days)
        .map(|r| r.map_or(0, |r| r.removed))
        .map_err(|e| format!("修剪审计日志失败: {e}"))
}

/// 读取保存期限（天）。
#[tauri::command]
pub fn audit_retention_get(state: tauri::State<'_, AuditDb>) -> Result<i64, String> {
    state.retention_days().map_err(|e| format!("读取保存期限失败: {e}"))
}

/// 设置保存期限（天，30–36500）。
#[tauri::command]
pub fn audit_retention_set(state: tauri::State<'_, AuditDb>, days: i64) -> Result<(), String> {
    state.set_retention_days(days).map_err(|e| format!("保存期限失败: {e}"))
}

/// 读取应用配置 JSON（无配置返回 null）。
#[tauri::command]
pub fn config_get(state: tauri::State<'_, AuditDb>) -> Result<Option<String>, String> {
    state.config_get().map_err(|e| format!("读取配置失败: {e}"))
}

/// 覆盖保存应用配置 JSON（前端传 `anonkit.config.v1` 序列化结果）。
#[tauri::command]
pub fn config_set(state: tauri::State<'_, AuditDb>, json: String) -> Result<(), String> {
    state
        .config_set(&json)
        .map_err(|e| format!("保存配置失败: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(time: &str, file: &str, n: i64) -> AuditEntryDto {
        AuditEntryDto {
            time: time.to_string(),
            file: file.to_string(),
            hit_count: n,
            types: "手机×2 姓名×1".to_string(),
            output: format!("{file}_脱敏.pdf"),
            mapping: "已导出".to_string(),
            is_deep: true,
            ..Default::default()
        }
    }

    fn filled(n: usize) -> AuditDb {
        let db = AuditDb::open_in_memory().unwrap();
        for i in 0..n {
            db.insert(&sample("2026-08-11 10:00", &format!("f{i}.pdf"), i as i64)).unwrap();
        }
        db
    }

    #[test]
    fn insert_list_count_and_chain_ok() {
        let db = AuditDb::open_in_memory().unwrap();
        assert_eq!(db.count().unwrap(), 0);
        assert!(db.verify().unwrap().ok);
        let e1 = db.insert(&sample("2026-08-11 10:00", "合同A.pdf", 3)).unwrap();
        let e2 = db.insert(&sample("2026-08-11 10:05", "合同B.pdf", 5)).unwrap();
        assert_eq!((e1.seq, e2.seq), (1, 2));
        assert_eq!(e1.prev_hash, GENESIS);
        assert_eq!(e2.prev_hash, e1.hash);
        let list = db.list(None).unwrap();
        assert_eq!(list, vec![e2.clone(), e1]);
        assert_eq!(db.list(Some(1)).unwrap(), vec![e2]);
        let v = db.verify().unwrap();
        assert!(v.ok && v.count == 2 && v.broken_seq.is_none());
    }

    #[test]
    fn frontend_supplied_chain_fields_are_ignored() {
        let db = AuditDb::open_in_memory().unwrap();
        let mut e = sample("2026-08-11 10:00", "a.txt", 1);
        e.seq = 99;
        e.hash = "forged".into();
        e.kind = "prune".into();
        let out = db.insert(&e).unwrap();
        assert_eq!(out.seq, 1);
        assert_eq!(out.kind, KIND_ENTRY);
        assert_ne!(out.hash, "forged");
        assert!(db.verify().unwrap().ok);
    }

    #[test]
    fn deep_flag_persists() {
        let db = AuditDb::open_in_memory().unwrap();
        let mut shallow = sample("2026-08-11 10:00", "笔记.txt", 1);
        shallow.is_deep = false;
        db.insert(&shallow).unwrap();
        assert!(!db.list(None).unwrap()[0].is_deep);
    }

    #[test]
    fn tampering_one_record_is_detected() {
        let db = filled(5);
        db.lock()
            .execute("UPDATE audit_log SET hit_count = 999 WHERE seq = 3", [])
            .unwrap();
        let v = db.verify().unwrap();
        assert!(!v.ok);
        assert_eq!(v.broken_seq, Some(3));
        assert!(v.reason.contains("修改"));
    }

    #[test]
    fn deleting_middle_record_is_detected() {
        let db = filled(5);
        db.lock().execute("DELETE FROM audit_log WHERE seq = 3", []).unwrap();
        let v = db.verify().unwrap();
        assert!(!v.ok);
        assert_eq!(v.broken_seq, Some(3));
    }

    #[test]
    fn deleting_tail_record_is_detected() {
        let db = filled(5);
        db.lock().execute("DELETE FROM audit_log WHERE seq = 5", []).unwrap();
        let v = db.verify().unwrap();
        assert!(!v.ok);
        assert_eq!(v.broken_seq, Some(5));
    }

    #[test]
    fn prune_keeps_chain_verifiable_and_leaves_record() {
        let db = AuditDb::open_in_memory().unwrap();
        db.insert(&sample("2020-01-01 10:00", "old1.pdf", 1)).unwrap();
        db.insert(&sample("2020/2/3 10:00:00", "old2.pdf", 1)).unwrap();
        db.insert(&sample("2026-08-11 10:00", "new.pdf", 1)).unwrap();
        let today = days_from_civil(2026, 9, 1);
        let r = db.prune_at(1095, today).unwrap().unwrap();
        assert_eq!(r.removed, 2);
        assert_eq!(r.root_hash.len(), 64);
        let list = db.list(None).unwrap();
        assert_eq!(list.len(), 2); // new.pdf + 修剪记录
        assert_eq!(list[0].kind, KIND_PRUNE);
        assert!(list[0].note.contains(&r.root_hash) && list[0].note.contains("\"removed\":2"));
        let v = db.verify().unwrap();
        assert!(v.ok, "{v:?}");
        assert_eq!(v.pruned_total, 2);
        // 再次修剪：无过期，不产生新记录
        assert!(db.prune_at(1095, today).unwrap().is_none());
        // 删除修剪记录会被发现
        db.lock().execute("DELETE FROM audit_log WHERE kind = 'prune'", []).unwrap();
        assert!(!db.verify().unwrap().ok);
    }

    #[test]
    fn prune_never_deletes_beyond_contiguous_prefix() {
        let db = AuditDb::open_in_memory().unwrap();
        db.insert(&sample("2026-08-11 10:00", "new.pdf", 1)).unwrap();
        db.insert(&sample("2020-01-01 10:00", "old-but-behind.pdf", 1)).unwrap();
        assert!(db.prune_at(1095, days_from_civil(2026, 9, 1)).unwrap().is_none());
        assert_eq!(db.count().unwrap(), 2);
        assert!(db.verify().unwrap().ok);
    }

    #[test]
    fn retention_default_and_set() {
        let db = AuditDb::open_in_memory().unwrap();
        assert_eq!(db.retention_days().unwrap(), 1095);
        db.set_retention_days(365).unwrap();
        assert_eq!(db.retention_days().unwrap(), 365);
        db.set_retention_days(1).unwrap();
        assert_eq!(db.retention_days().unwrap(), 30);
    }

    #[test]
    fn legacy_db_is_migrated_with_marker() {
        let dir = std::env::temp_dir().join(format!("anonkit-audit-mig-{}", now_secs()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.db");
        {
            // 旧版表结构（无 note / 链字段）
            let c = Connection::open(&path).unwrap();
            c.execute_batch(
                "CREATE TABLE audit_log (id INTEGER PRIMARY KEY AUTOINCREMENT, time TEXT NOT NULL,
                  file TEXT NOT NULL, hit_count INTEGER NOT NULL DEFAULT 0, types TEXT NOT NULL DEFAULT '',
                  output TEXT NOT NULL DEFAULT '', mapping TEXT NOT NULL DEFAULT '', is_deep INTEGER NOT NULL DEFAULT 0);
                 INSERT INTO audit_log (time,file,hit_count) VALUES ('2026-08-01 09:00','a.docx',2);
                 INSERT INTO audit_log (time,file,hit_count) VALUES ('2026-08-02 09:00','b.docx',4);",
            )
            .unwrap();
        }
        let db = AuditDb::open(&path).unwrap();
        let list = db.list(None).unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].kind, KIND_MIGRATION);
        assert!(list[0].note.contains("2 条"));
        assert_eq!(list[2].file, "a.docx");
        assert_eq!(list[2].prev_hash, GENESIS);
        assert!(db.verify().unwrap().ok);
        // 迁移后继续追加，链保持有效；再次打开不重复迁移
        db.insert(&sample("2026-09-01 09:00", "c.docx", 1)).unwrap();
        drop(db);
        let db = AuditDb::open(&path).unwrap();
        assert_eq!(db.count().unwrap(), 4);
        assert!(db.verify().unwrap().ok);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_roundtrip_and_overwrite() {
        let db = AuditDb::open_in_memory().unwrap();
        assert_eq!(db.config_get().unwrap(), None);
        db.config_set(r#"{"maskChar":"*"}"#).unwrap();
        assert_eq!(db.config_get().unwrap().as_deref(), Some(r#"{"maskChar":"*"}"#));
        db.config_set(r#"{"maskChar":"■","nerEnabled":true}"#).unwrap();
        assert_eq!(
            db.config_get().unwrap().as_deref(),
            Some(r#"{"maskChar":"■","nerEnabled":true}"#)
        );
    }
}
