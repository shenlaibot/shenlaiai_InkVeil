"""PDF 安全模式：命中页栅格化涂黑，原文不可提取；未命中页保持原样；非敏感文字仍可检索。"""

import os
import tempfile

import pikepdf
import pypdfium2 as pdfium
from pikepdf import Array, Dictionary, Name

from parsers import parse_file
from redact import redact_file
from verifier import find_residue


def _make_pdf(path, pages, rotate=0):
    """pages: [[(x, y, text), ...], ...]，Helvetica 英文文本（WinAnsi 单字节字体）。"""
    pdf = pikepdf.new()
    font = pdf.make_indirect(
        Dictionary(Type=Name.Font, Subtype=Name.Type1, BaseFont=Name.Helvetica, Encoding=Name.WinAnsiEncoding)
    )
    for lines in pages:
        ops = ["BT /F1 14 Tf"]
        for x, y, t in lines:
            ops.append(f"1 0 0 1 {x} {y} Tm ({t}) Tj")
        ops.append("ET")
        page = pdf.add_blank_page(page_size=(400, 300))
        page.Resources = Dictionary(Font=Dictionary(F1=font))
        page.Contents = pikepdf.Stream(pdf, "\n".join(ops).encode("latin-1"))
        if rotate:
            page.Rotate = rotate
    pdf.docinfo["/Author"] = "John Smith"
    pdf.save(path)


def _extract(path):
    doc = pdfium.PdfDocument(path)
    try:
        return [doc[i].get_textpage().get_text_bounded() for i in range(len(doc))]
    finally:
        doc.close()


def _repl(parsed, needle, mask="[X]"):
    i = parsed.text.index(needle)
    return {"start": i, "end": i + len(needle), "replacement": mask}


def test_hit_page_rasterized_text_gone_other_page_kept():
    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, "a.pdf")
        _make_pdf(src, [
            [(40, 200, "Contact Alice Wonder today"), (40, 150, "phone 13800138000 ok")],
            [(40, 200, "Second page keeps Bob Builder text")],
        ])
        p = parse_file(src)
        r = redact_file(src, [_repl(p, "Alice Wonder"), _repl(p, "13800138000")], os.path.join(d, "out"))
        out = r["output_path"]
        assert r["unplaced_count"] == 0 and r["is_deep"]
        pages = _extract(out)
        # 命中页：原文彻底不可提取；非敏感文字仍可检索（不可见文字层）
        assert "Alice" not in pages[0] and "13800138000" not in pages[0]
        assert "Contact" in pages[0] and "today" in pages[0]
        assert "[X]" in pages[0]  # 替换文本进入文字层
        # 未命中页保持原样
        assert "Bob Builder" in pages[1]
        # 复检：字节、解压流、PDFium 文字、元数据都不含原文
        assert find_residue(out, "pdf", ["Alice Wonder", "13800138000", "John Smith"]) == []
        # 命中页变成图片页
        with pikepdf.open(out) as pdf:
            assert "/Im0" in pdf.pages[0].Resources.XObject
            assert "/Im0" not in (pdf.pages[1].Resources.get("/XObject") or {})


def test_rendered_page_has_black_box_at_hit():
    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, "a.pdf")
        _make_pdf(src, [[(40, 200, "SECRETWORD here")]])
        p = parse_file(src)
        r = redact_file(src, [_repl(p, "SECRETWORD")], os.path.join(d, "out"))
        doc = pdfium.PdfDocument(r["output_path"])
        try:
            img = doc[0].render(scale=1).to_pil().convert("L")
        finally:
            doc.close()
        dark = sum(1 for px in img.getdata() if px < 30)
        assert dark > 200, "命中区域应被涂黑"


def test_rotated_page_redacted_with_text_layer():
    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, "a.pdf")
        _make_pdf(src, [[(40, 200, "Rotated Secret Name")]], rotate=90)
        p = parse_file(src)
        r = redact_file(src, [_repl(p, "Secret Name")], os.path.join(d, "out"))
        txt = _extract(r["output_path"])[0]
        assert "Secret" not in txt
        assert "Rotated" in txt  # 旋转页同样保留可检索的非敏感文字
        assert "[X]" in txt  # 命中处写入替换文本，可检索/可还原


def test_no_hits_leaves_pdf_pages_untouched():
    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, "a.pdf")
        _make_pdf(src, [[(40, 200, "Nothing sensitive here")]])
        r = redact_file(src, [], os.path.join(d, "out"))
        assert "Nothing sensitive" in _extract(r["output_path"])[0]
        # 元数据仍被清理
        assert find_residue(r["output_path"], "pdf", ["John Smith"]) == []


def test_non_ascii_text_layer_survives_via_tounicode():
    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, "a.pdf")
        _make_pdf(src, [[(40, 200, "Herr M\xfcller call 13800138000")]])
        p = parse_file(src)
        r = redact_file(src, [_repl(p, "13800138000")], os.path.join(d, "out"))
        text = _extract(r["output_path"])[0]
        assert "M\xfcller" in text and "13800138000" not in text
