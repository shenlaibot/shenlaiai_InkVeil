"""墨隐 InkVeil sidecar — 隐私保障：零残留与零外传冒烟测试。

1. 全流程（解析 + 脱敏写回）结束后，系统临时目录无新增残留文件
   （例如 pikepdf 的 `.pikepdf.*` 临时文件应已清理）；
2. 输出脱敏副本不含任何敏感原文；
3. 处理路径不发起外部网络：本文件用例自身无任何外发请求，
   CI 中以 `HTTP_PROXY=http://127.0.0.1:9 HTTPS_PROXY=...` 运行本测试
   即作为「环境变量代理拦截」冒烟（若代码发起外部请求将因代理不可达而失败）。
"""

from __future__ import annotations

import pathlib
import tempfile

from fastapi.testclient import TestClient

from app import app
from parsers import parse_file
from redact import redact_file
from test_documents import SENSITIVE, find_replacements, make_pdf

client = TestClient(app)


def test_pdf_full_flow_leaves_no_temp_artifacts(tmp_path):
    """PDF 解析 + 脱敏写回全流程后无临时文件残留、输出无敏感原文。"""
    pdf = tmp_path / "privacy.pdf"
    make_pdf(pdf)
    out_dir = tmp_path / "out"
    out_dir.mkdir()

    tmp_root = pathlib.Path(tempfile.gettempdir())
    before = {p.name for p in tmp_root.iterdir() if p.is_file()}

    parsed = parse_file(str(pdf))
    repls = find_replacements(parsed.text)
    assert repls, "样例应命中敏感词"
    result = redact_file(str(pdf), repls, str(out_dir))

    after = {p.name for p in tmp_root.iterdir() if p.is_file()}
    assert after - before == set(), f"处理后有临时文件残留: {after - before}"

    out_path = pathlib.Path(result["output_path"])
    assert out_path.is_file()
    text = parse_file(str(out_path)).text
    for needle, _ in SENSITIVE:
        assert needle not in text, f"脱敏输出残留敏感原文: {needle}"


def test_txt_flow_leaves_no_temp_artifacts(tmp_path):
    """TXT 全流程同样不留临时文件残留。"""
    txt = tmp_path / "privacy.txt"
    txt.write_text("姓名：张三\n手机号：13800138000\n", encoding="utf-8")
    out_dir = tmp_path / "out"
    out_dir.mkdir()

    tmp_root = pathlib.Path(tempfile.gettempdir())
    before = {p.name for p in tmp_root.iterdir() if p.is_file()}

    parsed = parse_file(str(txt))
    repls = find_replacements(parsed.text)
    assert repls, "样例应命中敏感词"
    redact_file(str(txt), repls, str(out_dir))

    after = {p.name for p in tmp_root.iterdir() if p.is_file()}
    assert after - before == set(), f"处理后有临时文件残留: {after - before}"
