"""文档还原：docx 占位符跨 run、模糊写法、xlsx 共享串/inlineStr/批注、不覆盖输入。"""

import os
import zipfile

import pytest
from docx import Document
from fastapi.testclient import TestClient
from openpyxl import Workbook, load_workbook
from openpyxl.comments import Comment

from restore_doc import Restorer, restore_file

MAPPING = {"[姓名1]": "陈瑾瑜", "[姓名2]": "李四", "[公司1]": "北京云图科技有限公司"}


def test_restorer_text_variants():
    rs = Restorer(MAPPING)
    out = rs.restore_text("【姓名1】、[ 姓名 2 ]、[公司1]、［姓名１］、[姓名9]、PER-7")
    assert out == "陈瑾瑜、李四、北京云图科技有限公司、陈瑾瑜、[姓名9]、PER-7"
    st = rs.stats()
    assert st["exact_count"] == 1 and st["fuzzy_count"] == 3
    assert [s["text"] for s in st["suspects"]] == ["[姓名9]"]


def _make_docx(path):
    d = Document()
    p = d.add_paragraph()
    # 占位符被拆进多个 run，且中间 run 加粗
    p.add_run("甲方[公")
    r = p.add_run("司")
    r.bold = True
    p.add_run("1]委托，由【姓名")
    p.add_run(" 1】签字。")
    d.add_paragraph("表外：[姓名2] 与 [姓名9]")
    t = d.add_table(rows=1, cols=1)
    t.cell(0, 0).text = "表格内 [ 姓名 2 ]"
    d.sections[0].header.paragraphs[0].text = "页眉[公司1]"
    d.save(path)


def test_docx_cross_run_restore(tmp_path):
    src = str(tmp_path / "稿.docx")
    _make_docx(src)
    before = open(src, "rb").read()
    res = restore_file(src, MAPPING, str(tmp_path / "out"))
    assert open(src, "rb").read() == before, "输入文件不得被改"
    assert res["output_path"] != src and os.path.isfile(res["output_path"])
    d = Document(res["output_path"])
    assert d.paragraphs[0].text == "甲方北京云图科技有限公司委托，由陈瑾瑜签字。"
    assert d.paragraphs[1].text == "表外：李四 与 [姓名9]"
    assert d.tables[0].cell(0, 0).text == "表格内 李四"
    assert d.sections[0].header.paragraphs[0].text == "页眉北京云图科技有限公司"
    assert res["exact_count"] == 3  # [公司1]（跨 run）、[姓名2]、页眉[公司1]
    assert res["fuzzy_count"] == 2  # 【姓名 1】、[ 姓名 2 ]
    assert [s["text"] for s in res["suspects"]] == ["[姓名9]"]
    # 再次还原同名输出不覆盖
    res2 = restore_file(src, MAPPING, str(tmp_path / "out"))
    assert res2["output_path"] != res["output_path"]


def test_docx_keeps_formatting_of_first_run(tmp_path):
    src = str(tmp_path / "a.docx")
    d = Document()
    p = d.add_paragraph()
    r1 = p.add_run("[姓")
    r1.bold = True
    p.add_run("名1]后文")
    d.save(src)
    res = restore_file(src, MAPPING, str(tmp_path / "o"))
    runs = Document(res["output_path"]).paragraphs[0].runs
    assert "".join(r.text for r in runs) == "陈瑾瑜后文"
    assert runs[0].text == "陈瑾瑜" and runs[0].bold


def _make_xlsx(path):
    wb = Workbook()
    ws = wb.active
    ws["A1"] = "联系人：[姓名1]"
    ws["A2"] = "【公司1】"
    ws["A3"] = 42
    ws["A4"] = "[姓名9]"
    ws["B1"].comment = Comment("请联系[ 姓名2 ]", "x")
    wb.save(path)


def test_xlsx_restore(tmp_path):
    src = str(tmp_path / "表.xlsx")
    _make_xlsx(src)
    res = restore_file(src, MAPPING, str(tmp_path / "out"))
    ws = load_workbook(res["output_path"]).active
    assert ws["A1"].value == "联系人：陈瑾瑜"
    assert ws["A2"].value == "北京云图科技有限公司"
    assert ws["A3"].value == 42
    assert ws["A4"].value == "[姓名9]"
    assert ws["B1"].comment.text == "请联系李四"
    assert res["exact_count"] == 1 and res["fuzzy_count"] == 2
    assert [s["text"] for s in res["suspects"]] == ["[姓名9]"]
    with zipfile.ZipFile(res["output_path"]) as z:
        assert z.testzip() is None


def test_unsupported_and_missing(tmp_path):
    with pytest.raises(ValueError):
        p = tmp_path / "a.txt"
        p.write_text("x")
        restore_file(str(p), MAPPING, str(tmp_path))
    with pytest.raises(FileNotFoundError):
        restore_file(str(tmp_path / "none.docx"), MAPPING, str(tmp_path))


def test_restore_endpoint(tmp_path):
    from app import app

    src = str(tmp_path / "稿.docx")
    _make_docx(src)
    c = TestClient(app)
    r = c.post("/restore", json={"input_path": src, "mapping": MAPPING, "output_dir": str(tmp_path / "o")})
    assert r.status_code == 200, r.text
    j = r.json()
    assert j["ok"] and j["fuzzy_count"] == 2
    r = c.post("/restore", json={"input_path": str(tmp_path / "x.txt"), "mapping": MAPPING, "output_dir": str(tmp_path)})
    assert r.status_code in (400, 404)
