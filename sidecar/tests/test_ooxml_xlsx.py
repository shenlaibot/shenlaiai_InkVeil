"""XLSX XML 层写回：共享字符串、批注、页眉页脚、图表/图片保留、数值与公式。"""

import os
import re
import tempfile
import zipfile

from openpyxl import Workbook, load_workbook
from openpyxl.chart import BarChart, Reference
from openpyxl.comments import Comment

from parsers import parse_file
from redact import redact_file
from verifier import find_residue


def _make(path):
    wb = Workbook()
    ws = wb.active
    ws.title = "名册"
    ws["A1"] = "联系人"
    ws["B1"] = "张三丰"          # 共享字符串
    ws["A2"] = "联系人"
    ws["B2"] = "李留存"          # 同样共享字符串区里另一条，用户不脱敏
    ws["A3"] = "电话"
    ws["B3"] = 13800138000       # 数值单元格
    ws["B4"] = '="联系人"&"王五六"'  # 公式里的字符串字面量
    ws["C1"] = 100
    ws["C2"] = 200
    ws["B1"].comment = Comment("批注里有赵六七的电话", "对方律师")
    ws.oddHeader.center.text = "页眉客户周八九"
    chart = BarChart()
    chart.add_data(Reference(ws, min_col=3, min_row=1, max_row=2))
    ws.add_chart(chart, "E2")
    wb.properties.creator = "作者本人"
    wb.save(path)


def _repl(parsed, needle, mask="[X]"):
    i = parsed.text.index(needle)
    return {"start": i, "end": i + len(needle), "replacement": mask}


def test_redact_cells_comments_header_and_keep_chart():
    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, "a.xlsx")
        _make(src)
        p = parse_file(src)
        needles = ["张三丰", "13800138000", "王五六", "赵六七", "周八九"]
        repls = [_repl(p, n) for n in needles if n in p.text]
        r = redact_file(src, repls, os.path.join(d, "out"))
        out = r["output_path"]
        # 批注和页眉不在 parse 全文里，但原文同样必须被同步改写
        res = find_residue(out, "xlsx", ["张三丰", "13800138000", "王五六"])
        assert res == [], res
        with zipfile.ZipFile(out) as zf:
            names = zf.namelist()
            assert any(n.startswith("xl/charts/") for n in names), "图表不应丢失"
            assert not any(n.startswith("docProps/thumbnail") for n in names)
        wb = load_workbook(out)
        ws = wb["名册"]
        assert ws["B1"].value == "[X]"
        assert ws["B2"].value == "李留存"  # 未选中的共享字符串条目保持不变
        assert ws["C1"].value == 100        # 无关数值保持数值类型


def test_comment_header_replaced_by_pairs():
    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, "a.xlsx")
        _make(src)
        p = parse_file(src)
        # 只选中单元格里的“张三丰”：批注里出现同文时也要同步改
        wb = load_workbook(src)
        wb["名册"]["B1"].comment = Comment("张三丰的批注", "对方律师")
        wb.save(src)
        p = parse_file(src)
        r = redact_file(src, [_repl(p, "张三丰")], os.path.join(d, "out"))
        assert find_residue(r["output_path"], "xlsx", ["张三丰", "对方律师"]) == []


def test_numeric_cell_becomes_text_only_where_redacted():
    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, "a.xlsx")
        _make(src)
        p = parse_file(src)
        r = redact_file(src, [_repl(p, "13800138000", "138****8000")], os.path.join(d, "out"))
        wb = load_workbook(r["output_path"])
        assert wb["名册"]["B3"].value == "138****8000"
        assert wb["名册"]["C2"].value == 200


def test_formula_literal_rewritten_and_recalc_flag():
    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, "a.xlsx")
        _make(src)
        p = parse_file(src)
        r = redact_file(src, [_repl(p, "王五六")], os.path.join(d, "out"))
        assert r["replaced_count"] == 1
        with zipfile.ZipFile(r["output_path"]) as zf:
            wbx = zf.read("xl/workbook.xml").decode("utf-8")
            assert 'fullCalcOnLoad="1"' in wbx
        assert find_residue(r["output_path"], "xlsx", ["王五六"]) == []


def test_untouched_parts_byte_identical_and_authors_scrubbed():
    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, "a.xlsx")
        _make(src)
        p = parse_file(src)
        r = redact_file(src, [_repl(p, "张三丰")], os.path.join(d, "out"))
        with zipfile.ZipFile(src) as a, zipfile.ZipFile(r["output_path"]) as b:
            for n in a.namelist():
                if n.startswith("xl/charts/") or n == "xl/styles.xml":
                    assert a.read(n) == b.read(n), n
        assert find_residue(r["output_path"], "xlsx", ["对方律师", "作者本人"]) == []
