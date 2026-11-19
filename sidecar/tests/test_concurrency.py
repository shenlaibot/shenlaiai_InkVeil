"""墨隐 InkVeil sidecar — 并发安全回归测试。

真实产品中 Rust 主进程以线程池并发调用 sidecar（批量扫描/脱敏多文档）。
pypdfium2/PDFium 非线程安全：修复前 4 线程并发解析/定位 PDF 会触发
Segmentation fault。本测试确保「并发 parse + redact」稳定通过。
"""

from __future__ import annotations

import os
from concurrent.futures import ThreadPoolExecutor

from fastapi.testclient import TestClient

from app import app
from test_documents import SENSITIVE, find_replacements, make_pdf

client = TestClient(app)


def test_concurrent_pdf_parse_and_redact(tmp_path):
    """8 线程并发解析 + 脱敏 8 个 PDF：不崩溃、输出正确、原文件不变。"""
    src = tmp_path / "src"
    out = tmp_path / "out"
    src.mkdir()
    out.mkdir()
    pdfs = []
    originals = {}
    for i in range(8):
        p = src / f"c{i}.pdf"
        make_pdf(p)
        pdfs.append(p)
        originals[i] = p.read_bytes()

    def work(pdf):
        pr = client.post("/parse", json={"path": str(pdf)}).json()
        assert pr["ok"], pr
        repls = find_replacements(pr["text"])
        assert repls
        rr = client.post(
            "/redact",
            json={"input_path": str(pdf), "replacements": repls, "output_dir": str(out)},
        )
        return rr.json()

    with ThreadPoolExecutor(max_workers=8) as ex:
        results = list(ex.map(work, pdfs))

    for i, body in enumerate(results):
        assert body["ok"], body
        f = body["files"][0]
        assert f["replaced_count"] >= 5  # 样例 PDF 至少 5 处敏感命中
        assert os.path.exists(f["output_path"])
        # 原文件不被并发写回破坏
        assert pdfs[i].read_bytes() == originals[i]
        # 输出副本不再含敏感原文（脱敏验证）
        from parsers import parse_file

        text = parse_file(f["output_path"]).text
        for needle, _ in SENSITIVE:
            assert needle not in text, f"输出残留敏感原文: {needle}"
