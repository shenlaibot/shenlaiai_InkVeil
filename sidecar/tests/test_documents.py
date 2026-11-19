"""墨隐 InkVeil sidecar — 文档解析与脱敏写回全链路测试（真实断言）。

覆盖 PDF / DOCX / TXT(UTF-8 + GBK) / MD 四类格式：
  1. POST /parse 得到全文 text 与 spans（span 偏移与全文一致）
  2. 用简单规则匹配全文得到敏感词偏移，构造 replacements
  3. POST /redact 脱敏写回
  4. 断言：输出文件存在、原文件字节未变、重新解析输出文件确认敏感原文已替换
"""

from __future__ import annotations

import os

import pytest
from fastapi.testclient import TestClient

from app import app
from parsers import parse_file
from redact import redact_file, redact_page_stream, escape_pdf_bytes

client = TestClient(app)

# 敏感词 -> 掩码（与产品规则引擎产物同构的最小实现）
SENSITIVE = [
    ("张三", "张*"),
    ("13800138000", "138****8000"),
    ("110101198403201234", "110101**********34"),
]


def find_replacements(text: str) -> list[dict]:
    """在全文上做规则匹配，模拟 Rust 侧检测产物：按全文偏移构造 replacements。"""
    repls = []
    for needle, mask in sorted(SENSITIVE, key=lambda kv: -len(kv[0])):
        pos = 0
        while True:
            i = text.find(needle, pos)
            if i < 0:
                break
            repls.append({"start": i, "end": i + len(needle), "replacement": mask})
            pos = i + len(needle)
    repls.sort(key=lambda r: r["start"])
    return repls


def assert_original_unchanged(path, orig_bytes):
    assert path.read_bytes() == orig_bytes


def assert_redacted_text(text: str, present: list | None = None):
    """断言脱敏后全文：原文出现过的敏感词全部消失、对应掩码可见。"""
    for needle, mask in present or SENSITIVE:
        assert needle not in text, f"敏感原文残留: {needle}"
        assert mask in text, f"掩码缺失: {mask}"


# ===========================================================================
# 测试文档生成器
# ===========================================================================


def make_pdf(path):
    from reportlab.lib.pagesizes import A4
    from reportlab.pdfbase import pdfmetrics
    from reportlab.pdfbase.cidfonts import UnicodeCIDFont
    from reportlab.pdfgen import canvas

    pdfmetrics.registerFont(UnicodeCIDFont("STSong-Light"))
    c = canvas.Canvas(str(path), pagesize=A4)
    lines = [
        "标题：个人信息登记表",
        "姓名：张三",
        "手机号：13800138000",
        "身份证号：110101198403201234",
        "住址：北京市朝阳区示例路 88 号",
        "备注：无",
    ]
    y = 760
    for line in lines:
        c.setFont("STSong-Light", 14)
        c.drawString(80, y, line)
        y -= 40
    c.showPage()
    c.setFont("STSong-Light", 14)
    c.drawString(80, 760, "第二页：普通内容，无敏感信息")
    c.drawString(80, 720, "张三的合同编号：A-2024-0012")
    c.setFont("STSong-Light", 12)
    c.drawString(80, 660, "联系手机 13800138000 已登记")
    c.showPage()
    c.save()


def add_textbox(paragraph, text: str):
    """在段落中插入一个 VML 文本框（w:pict -> v:shape -> v:textbox -> w:txbxContent）。"""
    from xml.sax.saxutils import escape

    from lxml import etree

    xml = (
        '<w:pict xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" '
        'xmlns:v="urn:schemas-microsoft-com:vml">'
        '<v:shape style="width:120pt;height:30pt"><v:textbox>'
        "<w:txbxContent><w:p><w:r><w:t>{}</w:t></w:r></w:p></w:txbxContent>"
        "</v:textbox></v:shape></w:pict>"
    ).format(escape(text))
    paragraph._p.append(etree.fromstring(xml.encode("utf-8")))


def make_docx(path):
    from docx import Document

    doc = Document()
    # 正文（含格式 run）
    p1 = doc.add_paragraph()
    r1 = p1.add_run("姓名：张三")
    r1.bold = True
    r1.font.size = 12
    p2 = doc.add_paragraph()
    p2.add_run("手机号：13800138000 已登记")
    # 表格
    t = doc.add_table(rows=1, cols=1)
    t.rows[0].cells[0].text = "身份证号：110101198403201234"
    # 页眉
    doc.sections[0].header.paragraphs[0].text = "内部资料 - 张三"
    # 文本框
    add_textbox(doc.add_paragraph(), "文本框：张三的备注")
    # core properties（用于断言清理）
    doc.core_properties.author = "测试作者"
    doc.core_properties.title = "测试标题"
    doc.save(str(path))


# ===========================================================================
# PDF
# ===========================================================================


def test_pdf_parse_and_redact(tmp_path):
    samples = tmp_path / "samples"
    out = tmp_path / "out"
    samples.mkdir()
    out.mkdir()
    pdf_path = samples / "sample.pdf"
    make_pdf(pdf_path)
    orig_bytes = pdf_path.read_bytes()

    # 1. parse
    resp = client.post("/parse", json={"doc_id": "pdf1", "path": str(pdf_path)})
    assert resp.status_code == 200
    data = resp.json()
    assert data["ok"] is True and data["format"] == "pdf"
    text = data["text"]
    assert "张三" in text and "13800138000" in text and "110101198403201234" in text
    assert data["structure"]["page_count"] == 2
    assert data["structure"]["pages"][0]["page"] == 0
    # span 偏移与全文一致；PDF span 全部带 bbox
    assert len(data["spans"]) == 9  # 第一页 6 行 + 第二页 3 行
    for sp in data["spans"]:
        assert text[sp["span_start"]:sp["span_end"]] == sp["text"]
        assert sp["bbox"] is not None
        assert len(sp["bbox"]) == 4

    # 2. replacements + redact
    repls = find_replacements(text)
    assert len(repls) == 5  # 张三 x2 + 手机号 x2 + 身份证 x1
    resp = client.post(
        "/redact",
        json={"input_path": str(pdf_path), "replacements": repls, "output_dir": str(out)},
    )
    assert resp.status_code == 200
    body = resp.json()
    f = body["files"][0]
    assert f["replaced_count"] == 5
    assert f["is_deep"] is True
    assert os.path.exists(f["output_path"])
    assert_original_unchanged(pdf_path, orig_bytes)

    # 3. 重新解析输出：原文不可提取、掩码可见、版面保留
    out_text = parse_file(f["output_path"]).text
    assert_redacted_text(out_text)
    assert "标题：个人信息登记表" in out_text
    assert "住址：北京市朝阳区示例路 88 号" in out_text
    assert "第二页：普通内容，无敏感信息" in out_text
    assert "张三的合同编号" not in out_text
    assert "的合同编号：A-2024-0012" in out_text


def test_pdf_cross_block_fallback():
    """跨内容流块的敏感串：无法精确删除 → is_deep=False，白矩形+掩码兜底。"""
    s1 = escape_pdf_bytes("张".encode("utf-16-be"))
    s2 = escape_pdf_bytes("三".encode("utf-16-be"))
    stream = (
        b"BT /F1 14 Tf 80 700 Td (" + s1 + b") Tj ET "
        b"BT /F1 14 Tf 96 700 Td (" + s2 + b") Tj ET"
    )
    new_stream, results = redact_page_stream(
        stream, [{"needle": "张三", "replacement": "张*", "bbox": (80.0, 700.0, 96.0, 714.0)}]
    )
    assert results[0]["deep"] is False
    # 文本层未删除（跨块残留：两块的 UTF-16BE 序列仍在内容流），白矩形与掩码已追加
    assert escape_pdf_bytes("张".encode("utf-16-be")) in new_stream
    assert escape_pdf_bytes("三".encode("utf-16-be")) in new_stream
    assert b"re f Q" in new_stream  # 白矩形
    assert escape_pdf_bytes("张*".encode("utf-16-be")) in new_stream  # 掩码重绘


# ===========================================================================
# DOCX
# ===========================================================================


def test_docx_parse_and_redact(tmp_path):
    from docx import Document

    samples = tmp_path / "samples"
    out = tmp_path / "out"
    samples.mkdir()
    out.mkdir()
    docx_path = samples / "sample.docx"
    make_docx(docx_path)
    orig_bytes = docx_path.read_bytes()

    # 1. parse
    resp = client.post("/parse", json={"doc_id": "doc1", "path": str(docx_path)})
    assert resp.status_code == 200
    data = resp.json()
    assert data["ok"] is True and data["format"] == "docx"
    text = data["text"]
    assert "姓名：张三" in text
    assert "身份证号：110101198403201234" in text
    assert "内部资料 - 张三" in text          # 页眉
    assert "文本框：张三的备注" in text        # 文本框
    block_types = {b["type"] for b in data["structure"]["blocks"]}
    assert {"正文", "表格", "页眉", "文本框"} <= block_types
    for sp in data["spans"]:
        assert text[sp["span_start"]:sp["span_end"]] == sp["text"]
        assert sp["bbox"] is None

    # 2. replacements + redact
    repls = find_replacements(text)
    assert len(repls) == 5  # 正文张三 + 表格身份证 + 页眉张三 + 文本框张三 + 手机号
    resp = client.post(
        "/redact",
        json={"input_path": str(docx_path), "replacements": repls, "output_dir": str(out)},
    )
    assert resp.status_code == 200
    f = resp.json()["files"][0]
    assert f["replaced_count"] == 5
    assert f["is_deep"] is True
    assert os.path.exists(f["output_path"])
    assert_original_unchanged(docx_path, orig_bytes)

    # 3. 重新解析输出：敏感原文全部替换
    out_text = parse_file(f["output_path"]).text
    assert_redacted_text(out_text)

    # 4. 样式保留：正文首个 run 仍 bold，文本已替换；core properties 已清理
    d = Document(f["output_path"])
    p1_runs = d.paragraphs[0].runs
    assert p1_runs[0].text == "姓名：张*"
    assert p1_runs[0].bold is True
    assert d.core_properties.author == ""
    assert d.core_properties.title == ""


# ===========================================================================
# TXT（UTF-8 / GBK 编码容错）
# ===========================================================================


@pytest.mark.parametrize("encoding", ["utf-8", "gbk"])
def test_txt_parse_and_redact_encodings(tmp_path, encoding):
    out = tmp_path / "out"
    out.mkdir()
    txt_path = tmp_path / f"sample_{encoding}.txt"
    content = "姓名：张三\n电话：13800138000\n住址：北京市朝阳区\n"
    if encoding == "utf-8":
        txt_path.write_text(content, encoding="utf-8")
    else:
        txt_path.write_bytes(content.encode("gbk"))
    orig_bytes = txt_path.read_bytes()

    # 1. parse：两种编码均可正确解析（UTF-8 优先 / GBK 兜底）
    resp = client.post("/parse", json={"doc_id": "txt1", "path": str(txt_path)})
    assert resp.status_code == 200
    data = resp.json()
    assert data["ok"] is True and data["format"] == "txt"
    assert data["text"] == content
    assert data["structure"]["line_count"] == 3
    assert data["spans"][0]["block"] == "行1"
    for sp in data["spans"]:
        assert data["text"][sp["span_start"]:sp["span_end"]] == sp["text"]

    # 2. replacements + redact
    repls = find_replacements(data["text"])
    assert len(repls) == 2
    resp = client.post(
        "/redact",
        json={"input_path": str(txt_path), "replacements": repls, "output_dir": str(out)},
    )
    assert resp.status_code == 200
    f = resp.json()["files"][0]
    assert f["replaced_count"] == 2
    assert os.path.exists(f["output_path"])
    assert_original_unchanged(txt_path, orig_bytes)

    # 3. 输出为 UTF-8，敏感原文已替换（身份证未出现在本样例，不参与掩码断言）
    out_text = f["output_path"] and parse_file(f["output_path"]).text
    assert_redacted_text(out_text, present=SENSITIVE[:2])
    assert out_text == "姓名：张*\n电话：138****8000\n住址：北京市朝阳区\n"


# ===========================================================================
# MD
# ===========================================================================


def test_md_parse_and_redact(tmp_path):
    out = tmp_path / "out"
    out.mkdir()
    md_path = tmp_path / "sample.md"
    content = "# 测试文档\n\n作者：张三\n\n联系方式：13800138000\n"
    md_path.write_text(content, encoding="utf-8")
    orig_bytes = md_path.read_bytes()

    resp = client.post("/parse", json={"doc_id": "md1", "path": str(md_path)})
    assert resp.status_code == 200
    data = resp.json()
    assert data["format"] == "md"
    assert data["text"] == content
    assert data["structure"]["line_count"] == 5

    repls = find_replacements(data["text"])
    assert len(repls) == 2
    resp = client.post(
        "/redact",
        json={"input_path": str(md_path), "replacements": repls, "output_dir": str(out)},
    )
    assert resp.status_code == 200
    f = resp.json()["files"][0]
    assert f["replaced_count"] == 2
    assert os.path.exists(f["output_path"])
    assert_original_unchanged(md_path, orig_bytes)

    out_text = parse_file(f["output_path"]).text
    assert_redacted_text(out_text, present=SENSITIVE[:2])
    assert out_text == "# 测试文档\n\n作者：张*\n\n联系方式：138****8000\n"


# ===========================================================================
# 边界与校验
# ===========================================================================


def test_redact_overlap_rejected(tmp_path):
    p = tmp_path / "a.txt"
    p.write_text("abcd", encoding="utf-8")
    resp = client.post(
        "/redact",
        json={
            "input_path": str(p),
            "replacements": [
                {"start": 0, "end": 2, "replacement": "x"},
                {"start": 1, "end": 3, "replacement": "y"},
            ],
            "output_dir": str(tmp_path / "out"),
        },
    )
    assert resp.status_code == 400


def test_redact_pdf_warns_unmapped(tmp_path):
    """超出全文范围的替换不应崩溃，计入 warnings（PDF 无对应页时）。"""
    p = tmp_path / "b.txt"
    p.write_text("hello world", encoding="utf-8")
    resp = client.post(
        "/redact",
        json={
            "input_path": str(p),
            "replacements": [{"start": 6, "end": 11, "replacement": "xxx"}],
            "output_dir": str(tmp_path / "out"),
        },
    )
    assert resp.status_code == 200
    assert resp.json()["files"][0]["replaced_count"] == 1
