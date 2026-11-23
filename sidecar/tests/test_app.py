"""墨隐 InkVeil sidecar 接口冒烟测试（pytest）。"""

import os
import threading

from fastapi.testclient import TestClient

from app import _exit_on_stdin_eof, app

client = TestClient(app)


def test_health_ok():
    resp = client.get("/health")
    assert resp.status_code == 200
    assert resp.json() == {"status": "ok", "version": "0.2.0"}


def test_parse_txt_smoke(tmp_path):
    p = tmp_path / "sample.txt"
    p.write_text("姓名：张三\n电话：13800138000\n", encoding="utf-8")
    resp = client.post("/parse", json={"doc_id": "t1", "path": str(p)})
    assert resp.status_code == 200
    data = resp.json()
    assert data["ok"] is True
    assert data["format"] == "txt"
    assert "张三" in data["text"]
    assert len(data["spans"]) == 2
    assert data["spans"][0]["block"] == "行1"


def test_parse_missing_file_404():
    resp = client.post("/parse", json={"path": "/no/such/file.pdf"})
    assert resp.status_code == 404


def test_redact_smoke(tmp_path):
    p = tmp_path / "sample.txt"
    p.write_text("姓名：张三\n", encoding="utf-8")
    out_dir = tmp_path / "out"
    resp = client.post(
        "/redact",
        json={
            "input_path": str(p),
            "replacements": [{"start": 3, "end": 5, "replacement": "张*"}],
            "output_dir": str(out_dir),
        },
    )
    assert resp.status_code == 200
    body = resp.json()
    assert body["ok"] is True
    f = body["files"][0]
    assert f["replaced_count"] == 1
    assert f["is_deep"] is True
    # 输出名默认不含原文件名（原名常含当事人姓名）
    out = out_dir / os.path.basename(f["output_path"])
    assert out.exists()
    assert "sample" not in out.name


def test_ner_empty_text():
    # 空文本：不加载模型，返回空实体（新端点契约的轻量冒烟）
    resp = client.post("/ner", json={"text": ""})
    assert resp.status_code == 200
    body = resp.json()
    assert body["ok"] is True
    assert body["entities"] == []


def test_auth_required_when_token_set(monkeypatch):
    # Rust 主进程通过 --token 注入认证后，所有端点必须携带 Bearer token，
    # 否则拒绝（防本机其它进程扫端口后直接 /parse 读任意文件、/redact 写任意目录）。
    import app as app_module

    monkeypatch.setattr(app_module, "_TOKEN", "test-secret")
    try:
        resp = client.get("/health")
        assert resp.status_code == 401, "未携带 token 应 401"
        resp = client.post("/parse", json={"path": "/no/such/file.pdf"})
        assert resp.status_code == 401
        resp = client.get("/health", headers={"Authorization": "Bearer wrong"})
        assert resp.status_code == 401
        resp = client.get("/health", headers={"Authorization": "Bearer test-secret"})
        assert resp.status_code == 200
        assert resp.json() == {"status": "ok", "version": "0.2.0"}
    finally:
        monkeypatch.undo()


def test_token_auth_enforced_and_constant_time(monkeypatch):
    import app as app_module

    monkeypatch.setattr(app_module, "_TOKEN", "secret-token")
    c = TestClient(app_module.app)
    assert c.get("/health").status_code == 401
    assert c.get("/health", headers={"Authorization": "Bearer wrong"}).status_code == 401
    assert c.get("/health", headers={"Authorization": "Bearer secret-token"}).status_code == 200


def test_corrupt_files_give_friendly_errors(tmp_path):
    bad = tmp_path / "bad.docx"
    bad.write_bytes(b"PK\x03\x04" + b"not really a zip")
    r = client.post("/parse", json={"path": str(bad)})
    assert r.status_code == 500
    detail = r.json()["detail"]
    assert "损坏" in detail and "BadZipFile" not in detail and "repr" not in detail

    ole = tmp_path / "enc.xlsx"
    ole.write_bytes(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1" + b"\x00" * 64)
    r = client.post("/parse", json={"path": str(ole)})
    assert r.status_code == 500 and "加密" in r.json()["detail"]


def test_exit_on_stdin_eof_exits_when_pipe_closes(monkeypatch):
    read_end, write_end = os.pipe()
    saved_stdin = os.dup(0)
    exits = []
    monkeypatch.setattr(os, "_exit", exits.append)
    os.dup2(read_end, 0)
    try:
        worker = threading.Thread(target=_exit_on_stdin_eof, daemon=True)
        worker.start()
        worker.join(0.2)
        assert worker.is_alive() and exits == []  # 管道未关闭时持续等待
        os.close(write_end)
        worker.join(5)
    finally:
        os.dup2(saved_stdin, 0)
        os.close(saved_stdin)
        os.close(read_end)
    assert exits == [0]


def test_exit_on_stdin_eof_keeps_running_when_stdin_unavailable(monkeypatch):
    saved_stdin = os.dup(0)
    exits = []
    monkeypatch.setattr(os, "_exit", exits.append)
    os.close(0)
    try:
        _exit_on_stdin_eof()
    finally:
        os.dup2(saved_stdin, 0)
        os.close(saved_stdin)
    assert exits == []
