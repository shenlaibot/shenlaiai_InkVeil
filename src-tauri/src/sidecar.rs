//! Sidecar 管理器：负责拉起 / 停止 Python sidecar（FastAPI）进程，
//! 并通过 HTTP /health 轮询确认服务就绪。
//!
//! - 开发期：定位 `sidecar/.venv/bin/python` 并运行 `sidecar/app.py`
//! - 发布期：直接运行 bundle 的 externalBin（`binaries/anonkit-sidecar`）

use std::net::TcpListener;
use std::path::PathBuf;
use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::process::{Child, ChildStdin, Command};

/// health 轮询整体超时。单文件产物每次启动都要解压，首次还会被杀毒软件逐个扫描，
/// 实测 Mac 上 7–17 秒，Windows 可能更久。
const HEALTH_TIMEOUT: Duration = Duration::from_secs(180);
/// 启动失败时保留的 stderr 末尾行数。
const STDERR_TAIL_LINES: usize = 8;
/// 启动失败原因的最大字符数。
const FAILURE_MAX_CHARS: usize = 400;
/// 单次 health 请求超时。
const REQUEST_TIMEOUT: Duration = Duration::from_millis(800);
/// 轮询间隔。
const POLL_INTERVAL: Duration = Duration::from_millis(250);
/// 关闭 stdin 后等待 sidecar 自行退出的时长，超时才强杀。
const EXIT_GRACE: Duration = Duration::from_secs(5);
/// 递增计数器，为 token 熵补充进程内唯一性。
static TOKEN_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 生成一次性随机认证 token（防本机其它进程扫到端口后直接调用 sidecar）。
///
/// 使用操作系统 CSPRNG（OsRng）生成 32 字节随机数，64 位十六进制。
/// 注意：token 目前仍经命令行参数传给子进程（同机其他用户可用 ps 看到），
/// 后续计划改为 stdio 传输以彻底去掉端口与 token。
fn generate_token() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    // 额外混入计数器，保证即使 CSPRNG 异常（极端情况）同进程内也不会重复
    let counter = TOKEN_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut hasher = Sha256::new();
    hasher.update(buf);
    hasher.update(counter.to_le_bytes());
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// sidecar 的启动进度。扫描等命令据此等待启动完成，失败时拿到具体原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupStatus {
    Starting,
    Ready,
    Failed(String),
}

/// 启动进度的发送端，由 Tauri 托管；命令通过 `subscribe()` 等待。
pub type StartupWatch = tokio::sync::watch::Sender<StartupStatus>;

/// 启动期 stderr 的末尾几行；就绪后置为 None，不再保留任何日志。
type StderrTail = Arc<Mutex<Option<VecDeque<String>>>>;

fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

fn tail_text(tail: &StderrTail) -> String {
    let guard = tail.lock().unwrap_or_else(|p| p.into_inner());
    let joined = guard
        .as_ref()
        .map(|q| q.iter().cloned().collect::<Vec<_>>().join(" | "))
        .unwrap_or_default();
    truncate_chars(&joined, FAILURE_MAX_CHARS)
}

/// `/health` 响应结构。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct HealthStatus {
    pub status: String,
    pub version: Option<String>,
}

/// 解析 `/health` 返回的 JSON。
pub fn parse_health_json(body: &str) -> Result<HealthStatus, serde_json::Error> {
    serde_json::from_str(body)
}

/// 挑选一个当前空闲的随机端口（监听 0 由系统分配后随即释放）。
pub fn pick_free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .map(|addr| addr.port())
        .unwrap_or(0)
}

/// Sidecar 进程管理器。
pub struct SidecarManager {
    child: Child,
    /// sidecar stdin 的写端。释放（含主进程崩溃时由系统关闭）后 sidecar 读到 EOF 自行退出。
    /// 不能靠杀进程收尾：PyInstaller 单文件产物是"引导进程 + Python 子进程"两层，
    /// 杀引导进程会留下 Python 孤儿进程和解压出的临时目录。
    stdin: Option<ChildStdin>,
    port: u16,
    base_url: String,
    health_url: String,
    /// 认证 token（同进程内共享给 SidecarClient）
    token: String,
}

impl SidecarManager {
    /// 开发期命令：`sidecar/.venv/bin/python sidecar/app.py --host 127.0.0.1 --port <port> --token <token>`
    fn dev_command(port: u16, token: &str) -> Command {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let sidecar_dir = manifest_dir.join("..").join("sidecar");
        let python = if cfg!(windows) {
            sidecar_dir.join(".venv").join("Scripts").join("python.exe")
        } else {
            sidecar_dir.join(".venv").join("bin").join("python")
        };
        let app = sidecar_dir.join("app.py");

        let mut cmd = Command::new(python);
        cmd.arg(app)
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .arg("--token")
            .arg(token)
            .arg("--exit-on-stdin-eof")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    /// 发布期命令：运行 bundle 的 externalBin（PyInstaller 单文件产物）。
    fn release_command(port: u16, token: &str) -> Command {
        let exe = std::env::current_exe().expect("failed to locate current exe");
        let bin_dir = exe.parent().expect("failed to locate exe directory");
        let sidecar = if cfg!(windows) {
            bin_dir.join("anonkit-sidecar.exe")
        } else {
            bin_dir.join("anonkit-sidecar")
        };

        let mut cmd = Command::new(sidecar);
        cmd.arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .arg("--token")
            .arg(token)
            .arg("--exit-on-stdin-eof")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    /// 启动 sidecar 并轮询 `/health` 直到就绪。
    pub async fn start() -> Result<Self, String> {
        let port = pick_free_port();
        if port == 0 {
            return Err("无法分配本地端口".to_string());
        }
        let token = generate_token();
        let mut child = if cfg!(debug_assertions) {
            Self::dev_command(port, &token)
                .spawn()
                .map_err(|e| format!("无法启动开发版引擎：{e}"))?
        } else {
            Self::release_command(port, &token)
                .spawn()
                .map_err(|e| format!("无法启动引擎程序：{e}"))?
        };

        let stdin = child.stdin.take();

        // stdout / stderr 必须持续排空，否则管道写满后会阻塞 sidecar
        if let Some(mut out) = child.stdout.take() {
            tokio::spawn(async move {
                let _ = tokio::io::copy(&mut out, &mut tokio::io::sink()).await;
            });
        }
        let tail: StderrTail = Arc::new(Mutex::new(Some(VecDeque::new())));
        if let Some(err) = child.stderr.take() {
            tokio::spawn(Self::drain_stderr(err, tail.clone()));
        }

        // 端口由这里选定并传给 sidecar，不依赖解析日志：
        // 打包产物的 stderr 在部分平台上不一定有输出。
        let base_url = format!("http://127.0.0.1:{port}");
        let health_url = format!("{base_url}/health");

        if let Err(e) = Self::wait_healthy(&mut child, &health_url, &token, &tail).await {
            Self::shutdown(&mut child, stdin, EXIT_GRACE).await;
            return Err(e);
        }
        *tail.lock().unwrap_or_else(|p| p.into_inner()) = None;

        Ok(Self {
            child,
            stdin,
            port,
            base_url,
            health_url,
            token,
        })
    }

    /// 持续读取 stderr，启动期保留末尾几行用于报错。
    /// 按字节读取再有损解码：Windows 上可能出现非 UTF-8 输出，按行读字符串会中途报错退出。
    async fn drain_stderr(stderr: tokio::process::ChildStderr, tail: StderrTail) {
        use tokio::io::AsyncBufReadExt;

        let mut reader = tokio::io::BufReader::new(stderr);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf).await {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            let mut guard = tail.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(q) = guard.as_mut() {
                let line = String::from_utf8_lossy(&buf).trim().to_string();
                if !line.is_empty() {
                    if q.len() == STDERR_TAIL_LINES {
                        q.pop_front();
                    }
                    q.push_back(truncate_chars(&line, FAILURE_MAX_CHARS));
                }
            }
        }
    }

    /// 轮询 `/health`，直到返回 2xx、进程退出或超时（携带认证 token）。
    async fn wait_healthy(
        child: &mut Child,
        health_url: &str,
        token: &str,
        tail: &StderrTail,
    ) -> Result<(), String> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .map_err(|e| format!("无法创建本地连接：{e}"))?;
        let deadline = tokio::time::Instant::now() + HEALTH_TIMEOUT;

        loop {
            let resp = client
                .get(health_url)
                .header("Authorization", format!("Bearer {token}"))
                .timeout(REQUEST_TIMEOUT)
                .send()
                .await;
            if let Ok(resp) = resp {
                if resp.status().is_success() {
                    return Ok(());
                }
            }
            if let Ok(Some(status)) = child.try_wait() {
                // 等 stderr 读完最后几行
                tokio::time::sleep(Duration::from_millis(200)).await;
                return Err(format!("引擎进程已退出（{status}）：{}", tail_text(tail)));
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(format!(
                    "引擎 {} 秒内未就绪：{}",
                    HEALTH_TIMEOUT.as_secs(),
                    tail_text(tail)
                ));
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn health_url(&self) -> &str {
        &self.health_url
    }

    /// 认证 token（构造 SidecarClient 时使用，所有请求携带 Bearer 头）。
    pub fn token(&self) -> &str {
        &self.token
    }

    /// 停止 sidecar 进程并等待其退出。
    pub async fn stop(&mut self) {
        Self::shutdown(&mut self.child, self.stdin.take(), EXIT_GRACE).await;
    }

    /// 关闭 stdin 通知 sidecar 退出；超过 `grace` 仍未退出才强杀。
    async fn shutdown(child: &mut Child, stdin: Option<ChildStdin>, grace: Duration) {
        drop(stdin);
        if tokio::time::timeout(grace, child.wait()).await.is_err() {
            let _ = child.kill().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn spawn_with_stdin(program: &str, args: &[&str]) -> (Child, Option<ChildStdin>) {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        (child, stdin)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn shutdown_lets_child_exit_on_stdin_eof() {
        let (mut child, stdin) = spawn_with_stdin("cat", &[]);
        SidecarManager::shutdown(&mut child, stdin, Duration::from_secs(30)).await;
        // 正常退出：不是被信号杀掉的
        assert_eq!(child.wait().await.unwrap().code(), Some(0));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn shutdown_kills_child_that_ignores_eof() {
        let (mut child, stdin) = spawn_with_stdin("sleep", &["60"]);
        SidecarManager::shutdown(&mut child, stdin, Duration::from_millis(100)).await;
        assert_eq!(child.wait().await.unwrap().code(), None);
    }

    #[test]
    fn truncate_chars_keeps_char_boundaries() {
        assert_eq!(truncate_chars("处理引擎", 10), "处理引擎");
        assert_eq!(truncate_chars("处理引擎", 2), "处理…");
    }

    #[tokio::test]
    async fn startup_watch_reports_failure_reason() {
        let (tx, mut rx) = tokio::sync::watch::channel(StartupStatus::Starting);
        tx.send_replace(StartupStatus::Failed("原因".to_string()));
        let status = rx
            .wait_for(|s| *s != StartupStatus::Starting)
            .await
            .unwrap()
            .clone();
        assert_eq!(status, StartupStatus::Failed("原因".to_string()));
    }

    #[test]
    fn parse_health_json_ok() {
        let health: HealthStatus =
            parse_health_json(r#"{"status":"ok","version":"0.1.0"}"#).unwrap();
        assert_eq!(health.status, "ok");
        assert_eq!(health.version.as_deref(), Some("0.1.0"));
    }

    #[test]
    fn parse_health_json_invalid() {
        assert!(parse_health_json("not json").is_err());
        assert!(parse_health_json(r#"{"status":1}"#).is_err());
    }

    #[test]
    fn pick_free_port_returns_in_range() {
        let port = pick_free_port();
        assert!((1..=65535).contains(&port));
    }
}
