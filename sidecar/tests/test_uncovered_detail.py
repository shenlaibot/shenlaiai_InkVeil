"""未覆盖部位结构化明细 + /redact 新增字段（只含部位/类型/数量/哈希，不含原文）。"""
import os
import zipfile

import pytest

from doc_coverage import uncovered_detail


def _docx(path):
    with zipfile.ZipFile(path, "w") as z:
        z.writestr("[Content_Types].xml", "<Types/>")
        z.writestr("word/document.xml", "<w:document/>")
        z.writestr("word/media/image1.png", b"png")
        z.writestr("word/media/image2.png", b"png")
        z.writestr("word/embeddings/o1.bin", b"x")


def test_docx_detail(tmp_path):
    p = str(tmp_path / "a.docx")
    _docx(p)
    d = {i["kind"]: i for i in uncovered_detail(p, "docx")}
    assert d["image"]["count"] == 2
    assert d["embedded"]["count"] == 1


def test_pdf_scanned_page(tmp_path):
    PIL = pytest.importorskip("PIL.Image")
    p = str(tmp_path / "scan.pdf")
    PIL.new("RGB", (200, 200), "white").save(p, "PDF")
    d = uncovered_detail(p, "pdf")
    assert any(i["kind"] == "scanned_page" and i["where"] == "第 1 页" for i in d)


def test_redact_endpoint_fields(tmp_path):
    from fastapi.testclient import TestClient

    import app as appmod

    src = tmp_path / "t.txt"
    src.write_text("联系人张三，电话13800138000", encoding="utf-8")
    c = TestClient(appmod.app)
    r = c.post("/redact", json={
        "input_path": str(src), "output_dir": str(tmp_path / "out"),
        "replacements": [{"start": 3, "end": 5, "replacement": "[姓名1]"}],
        "verify_needles": ["张三"],
    })
    f = r.json()["files"][0]
    assert f["verified"] is True and f["residual"] == []
    assert len(f["input_sha256"]) == 64 and len(f["output_sha256"]) == 64
    assert f["input_sha256"] != f["output_sha256"]
    assert f["uncovered"] == [] and f["sidecar_version"]
    assert "张三" not in str(f["uncovered"])
