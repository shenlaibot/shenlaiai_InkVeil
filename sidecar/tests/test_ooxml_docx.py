"""DOCX XML 层引擎：此前漏掉的部位（超链接/修订/批注/脚注/页眉/嵌套表/文本框/元数据）必须被解析并脱敏。"""

import os
import re
import tempfile
import zipfile

from docx import Document

from parsers import parse_file
from redact import redact_file
from verifier import find_residue

W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"

COMMENTS = f"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:comments xmlns:w="{W}"><w:comment w:id="0" w:author="王大锤" w:initials="WDC"><w:p><w:r><w:t>批注里的电话13900139000</w:t></w:r></w:p></w:comment></w:comments>"""
FOOTNOTES = f"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:footnotes xmlns:w="{W}"><w:footnote w:id="1"><w:p><w:r><w:t>脚注联系人李四</w:t></w:r></w:p></w:footnote></w:footnotes>"""

EXTRA_BODY = (
    '<w:p><w:hyperlink w:anchor="x"><w:r><w:t>链接里的13800138000</w:t></w:r></w:hyperlink></w:p>'
    '<w:p><w:ins w:id="5" w:author="对方律师" w:date="2024-01-01T00:00:00Z"><w:r><w:t>插入的张三丰</w:t></w:r></w:ins>'
    '<w:del w:id="6" w:author="对方律师" w:date="2024-01-01T00:00:00Z"><w:r><w:delText>删除的王五六</w:delText></w:r></w:del></w:p>'
    '<w:p><w:r><w:t>拆分</w:t></w:r><w:r><w:t>的赵六七</w:t></w:r></w:p>'
)


def _make(path):
    doc = Document()
    doc.add_paragraph("正文：合同甲方")
    doc.sections[0].header.paragraphs[0].text = "页眉客户陈七八"
    t = doc.add_table(rows=1, cols=1)
    outer = t.cell(0, 0)
    inner = outer.add_table(rows=1, cols=1)  # 嵌套表格
    inner.cell(0, 0).text = "嵌套表格里的周八九"
    doc.core_properties.author = "作者本人"
    doc.core_properties.title = "标题含机密"
    doc.save(path)
    # 注入批注、脚注、超链接/修订等 python-docx 不会遍历的内容
    tmp = path + ".tmp"
    with zipfile.ZipFile(path) as zin, zipfile.ZipFile(tmp, "w", zipfile.ZIP_DEFLATED) as zout:
        for info in zin.infolist():
            data = zin.read(info.filename)
            if info.filename == "word/document.xml":
                s = data.decode("utf-8")
                s = s.replace("<w:sectPr", EXTRA_BODY + "<w:sectPr", 1)
                data = s.encode("utf-8")
            zout.writestr(info, data)
        zout.writestr("word/comments.xml", COMMENTS)
        zout.writestr("word/footnotes.xml", FOOTNOTES)
    os.replace(tmp, path)


def _redact_all(src, out_dir, needles):
    parsed = parse_file(src)
    repls = []
    for nd in needles:
        i = parsed.text.index(nd)
        repls.append({"start": i, "end": i + len(nd), "replacement": "[X]"})
    return redact_file(src, repls, out_dir)


def test_parse_covers_hidden_parts():
    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, "a.docx")
        _make(src)
        text = parse_file(src).text
        for must in ["链接里的13800138000", "插入的张三丰", "删除的王五六", "拆分的赵六七",
                     "批注里的电话13900139000", "脚注联系人李四", "页眉客户陈七八", "嵌套表格里的周八九"]:
            assert must in text, must


def test_redact_removes_everything_and_verifier_agrees():
    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, "a.docx")
        _make(src)
        needles = ["13800138000", "张三丰", "王五六", "赵六七", "13900139000", "李四", "陈七八", "周八九"]
        r = _redact_all(src, os.path.join(d, "out"), needles)
        assert r["unplaced_count"] == 0
        assert find_residue(r["output_path"], "docx", needles) == []
        # 输出仍是合法 docx，可被 python-docx 打开
        Document(r["output_path"])


def test_authors_and_metadata_scrubbed():
    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, "a.docx")
        _make(src)
        r = _redact_all(src, os.path.join(d, "out"), ["13800138000"])
        out = r["output_path"]
        assert find_residue(out, "docx", ["对方律师", "王大锤", "作者本人", "标题含机密"]) == []
        with zipfile.ZipFile(out) as zf:
            assert not any(n.startswith("docProps/thumbnail") for n in zf.namelist())
            doc = zf.read("word/document.xml").decode("utf-8")
            assert 'w:author="Author"' in doc


def test_untouched_parts_byte_identical():
    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, "a.docx")
        _make(src)
        r = _redact_all(src, os.path.join(d, "out"), ["13800138000"])
        with zipfile.ZipFile(src) as a, zipfile.ZipFile(r["output_path"]) as b:
            for n in ("word/styles.xml", "word/settings.xml"):
                if n in a.namelist():
                    assert a.read(n) == b.read(n), n


def test_coverage_no_longer_flags_handled_parts():
    from doc_coverage import uncovered_parts

    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, "a.docx")
        _make(src)
        gaps = uncovered_parts(src, "docx")
        assert not any("批注" in g or "修订" in g or "脚注" in g for g in gaps), gaps


def test_mc_fallback_copy_also_redacted():
    mc = "http://schemas.openxmlformats.org/markup-compatibility/2006"
    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, "a.docx")
        doc = Document()
        doc.add_paragraph("占位")
        doc.save(src)
        alt = (
            f'<mc:AlternateContent xmlns:mc="{mc}"><mc:Choice Requires="wps">'
            '<w:r><w:txbxContent><w:p><w:r><w:t>文本框里的孙九十</w:t></w:r></w:p></w:txbxContent></w:r></mc:Choice>'
            '<mc:Fallback><w:r><w:txbxContent><w:p><w:r><w:t>文本框里的孙九十</w:t></w:r></w:p></w:txbxContent></w:r></mc:Fallback>'
            "</mc:AlternateContent>"
        )
        tmp = src + ".t"
        with zipfile.ZipFile(src) as zin, zipfile.ZipFile(tmp, "w") as zout:
            for info in zin.infolist():
                data = zin.read(info.filename)
                if info.filename == "word/document.xml":
                    s = data.decode("utf-8").replace("<w:sectPr", f"<w:p>{alt}</w:p><w:sectPr", 1)
                    data = s.encode("utf-8")
                zout.writestr(info, data)
        os.replace(tmp, src)
        text = parse_file(src).text
        assert text.count("孙九十") == 1  # 回退副本不重复进入全文
        r = _redact_all(src, os.path.join(d, "out"), ["孙九十"])
        assert find_residue(r["output_path"], "docx", ["孙九十"]) == []
