//! Sidecar 管理器：负责拉起 / 停止 Python sidecar（FastAPI）进程，
//! 并通过 HTTP /health 轮询确认服务就绪。
//!
//! - 开发期：定位 `sidecar/.venv/bin/python` 并运行 `sidecar/app.py`
//! - 发布期：直接运行 bundle 的 externalBin（`binaries/anonkit-sidecar`）

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::process::{Child, Command};

/// uvicorn 启动日志中的端口行前缀。
const UVICORN_PORT_PREFIX: &str = "Uvicorn running on http://127.0.0.1:";
/// health 轮询整体超时。
const HEALTH_TIMEOUT: Duration = Duration::from_secs(30);
/// 单次 health 请求超时。
const REQUEST_TIMEOUT: Duration = Duration::from_millis(800);
/// 轮询间隔。
const POLL_INTERVAL: Duration = Duration::from_millis(250);
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

/// 从 uvicorn 启动日志行解析端口。
///
/// 示例：`INFO:     Uvicorn running on http://127.0.0.1:54321` -> `Some(54321)`
pub fn parse_port_from_line(line: &str) -> Option<u16> {
    let idx = line.find(UVICORN_PORT_PREFIX)?;
    let rest = &line[idx + UVICORN_PORT_PREFIX.len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<u16>().ok()
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
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    /// 启动 sidecar 并轮询 `/health` 直到就绪。
    pub async fn start() -> Result<Self, String> {
        let port = pick_free_port();
        let token = generate_token();
        let mut child = if cfg!(debug_assertions) {
            Self::dev_command(port, &token)
                .spawn()
                .map_err(|e| format!("spawn dev sidecar failed: {e}"))?
        } else {
            Self::release_command(port, &token)
                .spawn()
                .map_err(|e| format!("spawn sidecar failed: {e}"))?
        };

        // stdout 同样持续排空，避免管道写满后阻塞 sidecar
        if let Some(mut out) = child.stdout.take() {
            tokio::spawn(async move {
                let _ = tokio::io::copy(&mut out, &mut tokio::io::sink()).await;
            });
        }

        // 统一从启动日志解析端口：开发模式 uvicorn 同样会打印端口行，
        // 两种模式共用一套逻辑，发布模式的端口解析路径也能被测试覆盖。
        let actual_port = Self::await_port_from_logs(&mut child, HEALTH_TIMEOUT)
            .await
            .map_err(|e| {
                let _ = child.start_kill();
                format!("failed to resolve sidecar port: {e}")
            })?;

        let base_url = format!("http://127.0.0.1:{actual_port}");
        let health_url = format!("{base_url}/health");

        Self::wait_healthy(&health_url, &token)
            .await
            .inspect_err(|_| {
                let _ = child.start_kill();
            })?;

        Ok(Self {
            child,
            port: actual_port,
            base_url,
            health_url,
            token,
        })
    }

    /// 从子进程 stderr 逐行读取 uvicorn 日志，解析实际监听端口。
    ///
    /// uvicorn 的启动日志（`INFO: Uvicorn running on ...`）默认输出到 stderr；
    /// stdout 仅用于业务输出（sidecar 业务日志走 logging，也在 stderr）。
    async fn await_port_from_logs(child: &mut Child, timeout: Duration) -> Result<u16, String> {
        use tokio::io::AsyncBufReadExt;

        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "sidecar stderr is not piped".to_string())?;
        let mut lines = tokio::io::BufReader::new(stderr).lines();
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let next = tokio::time::timeout(remaining, lines.next_line())
                .await
                .map_err(|_| "timed out waiting for sidecar port line".to_string())?
                .map_err(|e| format!("read sidecar stderr failed: {e}"))?;

            match next {
                Some(line) => {
                    if let Some(port) = parse_port_from_line(&line) {
                        // 必须继续读取并丢弃后续 stderr：读端一旦关闭，子进程写日志会遇到
                        // broken pipe；管道写满则会阻塞整个 sidecar。
                        // 日志内容可能含路径等信息，这里只丢弃，不落盘、不转发。
                        tokio::spawn(async move {
                            while let Ok(Some(_)) = lines.next_line().await {}
                        });
                        return Ok(port);
                    }
                }
                None => {
                    return Err("sidecar stderr closed before port was printed".to_string());
                }
            }
        }
    }

    /// 轮询 `/health`，直到返回 2xx 或超时（携带认证 token）。
    async fn wait_healthy(health_url: &str, token: &str) -> Result<(), String> {
        let client = reqwest::Client::new();
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
            if tokio::time::Instant::now() >= deadline {
                return Err(format!("sidecar health check timed out: {health_url}"));
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

    /// 停止 sidecar 进程。
    pub async fn stop(&mut self) {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }
}

impl Drop for SidecarManager {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_port_from_uvicorn_line() {
        let line = "INFO:     Uvicorn running on http://127.0.0.1:54321 (Press CTRL+C to quit)";
        assert_eq!(parse_port_from_line(line), Some(54321));
        assert_eq!(
            parse_port_from_line("INFO:     Uvicorn running on http://127.0.0.1:0"),
            Some(0)
        );
        assert_eq!(parse_port_from_line("unrelated log line"), None);
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
