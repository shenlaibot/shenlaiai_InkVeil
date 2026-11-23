#!/usr/bin/env python3
"""运行打包进安装包的 sidecar，确认它在真实的启动方式下可用，且退出后不留进程。

与应用一致：stdin/stdout/stderr 用管道、传 --port、--token 和 --exit-on-stdin-eof，
然后等 /health，再解析真实的 docx 和 pdf（会加载 lxml、pikepdf、pypdfium2 等二进制扩展）。
最后检查两种退出方式：主进程关闭 stdin（正常退出）、主进程被强杀。
用法：python scripts/smoke_bundled_sidecar.py <sidecar 可执行文件>
"""
from __future__ import annotations

import json
import os
import secrets
import signal
import socket
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from collections import deque

TIMEOUT_S = 180
EXIT_TIMEOUT_S = 30
SAMPLES = {"docx": "testdocs/01-房屋租赁合同.docx", "pdf": "testdocs/03-合作框架协议.pdf"}
HOST_MODE = "--simulate-host"
TOKEN_ENV = "SMOKE_SIDECAR_TOKEN"

tail: deque[str] = deque(maxlen=30)


class SmokeFailure(Exception):
    pass


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def start_sidecar(binary: str, port: int, token: str) -> subprocess.Popen:
    return subprocess.Popen(
        [binary, "--host", "127.0.0.1", "--port", str(port), "--token", token, "--exit-on-stdin-eof"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def drain(pipe, sink) -> None:
    threading.Thread(
        target=lambda: [sink(raw.decode("utf-8", "replace").rstrip()) for raw in pipe],
        daemon=True,
    ).start()


def simulate_host(binary: str, port: int) -> int:
    """模拟 Tauri 主进程：持有 sidecar 的 stdin 写端并排空输出，一直运行到被强杀。

    token 走环境变量，保证命令行里带 token 的只有 sidecar 进程。
    """
    proc = start_sidecar(binary, port, os.environ[TOKEN_ENV])
    drain(proc.stdout, lambda line: print(line, flush=True))
    drain(proc.stderr, lambda line: print(line, flush=True))
    proc.wait()
    return 0


def sidecar_pids(token: str) -> list[int]:
    """列出命令行含 token 的 sidecar 进程：引导进程和 Python 子进程的参数相同。"""
    if os.name == "nt":
        query = (
            "Get-CimInstance Win32_Process -Filter \"Name='anonkit-sidecar.exe'\" | "
            'ForEach-Object { "$($_.ProcessId) $($_.CommandLine)" }'
        )
        cmd = ["powershell", "-NoProfile", "-NonInteractive", "-Command", query]
    else:
        cmd = ["ps", "axww", "-o", "pid=,args="]
    out = subprocess.run(cmd, capture_output=True, text=True, errors="replace", check=True).stdout
    return [int(line.split()[0]) for line in out.splitlines() if token in line]


def wait_healthy(opener, base: str, token: str, proc: subprocess.Popen) -> None:
    auth = {"Authorization": f"Bearer {token}"}
    started = time.time()
    while True:
        try:
            with opener.open(urllib.request.Request(f"{base}/health", headers=auth), timeout=2) as r:
                if r.status == 200:
                    break
        except (urllib.error.URLError, OSError):
            pass
        if proc.poll() is not None:
            time.sleep(0.3)
            raise SmokeFailure(f"process exited with {proc.returncode} before healthy")
        if time.time() - started > TIMEOUT_S:
            raise SmokeFailure(f"not healthy within {TIMEOUT_S}s")
        time.sleep(0.25)
    print(f"health ok after {time.time() - started:.1f}s")


def running_layers(token: str) -> list[int]:
    """确认能看到两层进程，否则后面的“无残留”检查没有意义。"""
    pids = sidecar_pids(token)
    if len(pids) < 2:
        raise SmokeFailure(f"expected bootloader + python processes, found {pids}")
    return pids


def assert_exits(token: str, scenario: str) -> None:
    started = time.time()
    while True:
        pids = sidecar_pids(token)
        if not pids:
            print(f"no leftover processes after {scenario} ({time.time() - started:.1f}s)")
            return
        if time.time() - started > EXIT_TIMEOUT_S:
            for pid in pids:
                os.kill(pid, getattr(signal, "SIGKILL", signal.SIGTERM))
            raise SmokeFailure(f"processes left {EXIT_TIMEOUT_S}s after {scenario}: {pids}")
        time.sleep(0.25)


def check_parse_then_close_stdin(binary: str, opener) -> None:
    port, token = free_port(), secrets.token_hex(16)
    base = f"http://127.0.0.1:{port}"
    auth = {"Authorization": f"Bearer {token}"}
    proc = start_sidecar(binary, port, token)
    drain(proc.stdout, tail.append)
    drain(proc.stderr, tail.append)
    try:
        wait_healthy(opener, base, token, proc)
        for fmt, rel in SAMPLES.items():
            body = json.dumps({"doc_id": f"smoke-{fmt}", "path": os.path.abspath(rel)}).encode()
            req = urllib.request.Request(
                f"{base}/parse", data=body, headers={**auth, "Content-Type": "application/json"}
            )
            with opener.open(req, timeout=60) as r:
                data = json.load(r)
            if data.get("ok") is not True or data.get("format") != fmt:
                raise SmokeFailure(f"/parse {fmt}: ok={data.get('ok')} format={data.get('format')}")
            print(f"parse {fmt} ok")
        running_layers(token)
        proc.stdin.close()
        assert_exits(token, "stdin closed")
    finally:
        proc.kill()


def check_host_killed(binary: str, opener) -> None:
    port, token = free_port(), secrets.token_hex(16)
    host = subprocess.Popen(
        [sys.executable, os.path.abspath(__file__), HOST_MODE, binary, str(port)],
        env={**os.environ, TOKEN_ENV: token},
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    drain(host.stdout, tail.append)
    try:
        wait_healthy(opener, f"http://127.0.0.1:{port}", token, host)
        running_layers(token)
        host.kill()
        host.wait()
        assert_exits(token, "host killed")
    finally:
        host.kill()


def main() -> int:
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    if sys.argv[1] == HOST_MODE:
        return simulate_host(sys.argv[2], int(sys.argv[3]))

    binary = sys.argv[1]
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    try:
        check_parse_then_close_stdin(binary, opener)
        check_host_killed(binary, opener)
    except SmokeFailure as e:
        print(f"FAILED: {e}")
        print("\n".join(tail))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
