//! 集成测试公共辅助：定位 workspace / sidecar，启动并等待 /health 就绪。

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// workspace 根（`crates/anonymizer-pipeline/tests` → 仓库根）。
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// sidecar 解释器路径；`.venv` 不存在（CI 等环境）时返回 None。
pub fn sidecar_python() -> Option<PathBuf> {
    let root = workspace_root();
    let py = if cfg!(windows) {
        root.join("sidecar")
            .join(".venv")
            .join("Scripts")
            .join("python.exe")
    } else {
        root.join("sidecar")
            .join(".venv")
            .join("bin")
            .join("python")
    };
    py.is_file().then_some(py)
}

fn pick_free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .unwrap_or(0)
}

/// sidecar 进程守卫：测试结束时 kill 子进程。
pub struct SidecarGuard {
    child: Child,
}

impl Drop for SidecarGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 启动 sidecar 并轮询 `/health`（最多 30s）。
///
/// `.venv` 缺失或启动失败时返回 None（调用方打印提示后跳过测试，
/// 保证 CI 无 Python venv 时 `cargo test --workspace` 不失败）。
pub fn start_sidecar() -> Option<(SidecarGuard, String)> {
    let py = sidecar_python()?;
    let root = workspace_root();
    let app = root.join("sidecar").join("app.py");
    let port = pick_free_port();

    let mut child = Command::new(&py)
        .arg(&app)
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .env("ANONKIT_ALLOW_NO_TOKEN", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let base_url = format!("http://127.0.0.1:{port}");
    let client = anonymizer_pipeline::SidecarClient::new(base_url.clone());
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            eprintln!("sidecar 进程提前退出: {status}");
            return None;
        }
        if client.health() {
            return Some((SidecarGuard { child }, base_url));
        }
        if Instant::now() >= deadline {
            eprintln!("sidecar 启动超时（30s）: {base_url}");
            return None;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}
