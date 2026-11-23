"""导出后残留复检：能发现遗漏部位里的原文，且结果不回传原文。"""

import os
import tempfile
import zipfile

from docx import Document

from verifier import find_residue


def _docx(path, body, header=None, split=False):
    doc = Document()
    p = doc.add_paragraph()
    if split:
        p.add_run(body[:1])
        p.add_run(body[1:])  # 敏感词被拆进两个 run
    else:
        p.add_run(body)
    if header:
        doc.sections[0].header.paragraphs[0].text = header
    doc.save(path)


def test_docx_residue_in_header_found_and_no_plaintext_returned():
    with tempfile.TemporaryDirectory() as d:
        p = os.path.join(d, "a.docx")
        _docx(p, "合同正文", header="客户：张三")
        res = find_residue(p, "docx", ["张三"])
        assert res and res[0]["length"] == 2
        assert all("张三" not in str(v) for r in res for v in r.values())
        assert any("header" in r["where"] for r in res)


def test_docx_split_runs_detected():
    with tempfile.TemporaryDirectory() as d:
        p = os.path.join(d, "a.docx")
        _docx(p, "联系人张三", split=True)
        # 原始 XML 里“张”“三”被标签隔开，只有去标签拼接文本能发现
        assert find_residue(p, "docx", ["联系人张三"])


def test_docx_counts_each_occurrence_once_and_respects_paragraph_boundaries(tmp_path):
    path = tmp_path / "a.docx"
    doc = Document()
    doc.add_paragraph("138 0013")
    doc.add_paragraph("8000")
    doc.add_paragraph("13800138000")
    split = doc.add_paragraph()
    split.add_run("138")
    split.add_run("0013 8000")
    doc.save(path)

    assert find_residue(path, "docx", ["13800138000"]) == [
        {"index": 0, "length": 11, "where": "word/document.xml（文本）", "count": 2}
    ]


def test_xlsx_rich_text_does_not_join_distinct_shared_strings(tmp_path):
    path = tmp_path / "a.xlsx"
    with zipfile.ZipFile(path, "w") as zf:
        zf.writestr(
            "xl/sharedStrings.xml",
            '<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
            "<si><t>张</t></si><si><t>三</t></si><si><r><t>张</t></r><r><t>三</t></r></si></sst>",
        )
        zf.writestr("xl/workbook.xml", '<workbook><sheet name="张三"/></workbook>')

    residues = find_residue(path, "xlsx", ["张三"])
    assert [(r["where"], r["count"]) for r in residues] == [
        ("xl/sharedStrings.xml（文本）", 1),
        ("xl/workbook.xml（属性）", 1),
    ]


def test_docx_clean_has_no_residue():
    with tempfile.TemporaryDirectory() as d:
        p = os.path.join(d, "a.docx")
        _docx(p, "联系人[PER_1]")
        assert find_residue(p, "docx", ["张三"]) == []


def test_normalized_variants_detected_in_text():
    with tempfile.TemporaryDirectory() as d:
        p = os.path.join(d, "a.txt")
        with open(p, "w", encoding="utf-8") as f:
            f.write("电话 138 0013 8000")
        assert find_residue(p, "txt", ["13800138000"])  # 分隔符不同也算残留


def test_zip_part_names_and_metadata_scanned():
    with tempfile.TemporaryDirectory() as d:
        p = os.path.join(d, "a.docx")
        _docx(p, "正文")
        with zipfile.ZipFile(p, "a") as zf:
            zf.writestr("docProps/custom.xml", "<x>公司：甲乙丙科技有限公司</x>")
        assert find_residue(p, "docx", ["甲乙丙科技有限公司"])


def test_short_needles_ignored():
    with tempfile.TemporaryDirectory() as d:
        p = os.path.join(d, "a.txt")
        open(p, "w", encoding="utf-8").write("a b c")
        assert find_residue(p, "txt", ["a"]) == []


def test_pdf_metadata_residue_found():
    import pikepdf

    with tempfile.TemporaryDirectory() as d:
        p = os.path.join(d, "a.pdf")
        pdf = pikepdf.new()
        pdf.add_blank_page()
        pdf.docinfo["/Author"] = "张三"
        pdf.save(p)
        res = find_residue(p, "pdf", ["张三"])
        assert res, "PDF 元数据里的原文应被复检发现"
        clean = os.path.join(d, "b.pdf")
        pdf2 = pikepdf.new()
        pdf2.add_blank_page()
        pdf2.save(clean)
        assert find_residue(clean, "pdf", ["张三"]) == []
