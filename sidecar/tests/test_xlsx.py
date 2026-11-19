"""墨隐 InkVeil sidecar — XLSX 解析与脱敏全链路测试。

覆盖：
  1. POST /parse 解析多 sheet 工作簿（合并表头/公式/数字格式），span 偏移与全文一致
  2. POST /redact 脱敏写回：输出文件存在、原文件字节不变、replaced_count 正确、is_deep=True
  3. 重开输出 xlsx：敏感原文零残留、掩码可见、number_format/合并单元格保留、core properties 已清
  4. 公式单元格命中 → 不崩溃、warning 明示、公式保留
"""

import os

from fastapi.testclient import TestClient

from app import app
from parsers import parse_file
from redact import redact_file

client = TestClient(app)

# 敏感词 -> 掩码（与产品规则引擎产物同构的最小实现）
SENSITIVE = [
    ("张三", "张*"),
    ("13800138000", "138****8000"),
    ("440301197407210509", "440301**********09"),
    ("6222020200088888884", "622202*********8884"),
]

# 与 test_documents.py 一致的替换构造逻辑
def find_replacements(text: str) -> list[dict]:
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


def make_xlsx(path):
    from openpyxl import Workbook

    wb = Workbook()
    # --- Sheet 1: 员工信息（含合并表头、文本格式身份证列、加粗） ---
    ws = wb.active
    ws.title = "员工信息"
    ws.append(["姓名", "手机号", "身份证号", "银行卡号", "住址", "邮箱"])
    ws.append(["张三", "13800138000", "440301197407210509", "6222020200088888884",
               "北京市朝阳区示例路88号", "zhangsan@example.com"])
    ws.append(["李四", "13900139000", "110105199609043750", "6222020200099999993",
               "上海市浦东新区测试路1号", "lisi@example.com"])
    # 合并区（空区 G1:H1，不影响全文文本，用于断言合并保留）
    ws.merge_cells("G1:H1")
    # 身份证/银行卡列设为文本格式（真实登记表通常如此，避免数值精度截断）
    for row in ws.iter_rows(min_row=2, max_row=3, min_col=3, max_col=4):
        for cell in row:
            cell.number_format = "@"
    # 加粗表头
    from copy import copy

    for cell in ws[1]:
        new_font = copy(cell.font)
        new_font.bold = True
        cell.font = new_font
    # 公式单元格：汇总手机号数量
    ws.append(["员工数", "=COUNTA(B2:B3)", "", "", "", ""])

    # --- Sheet 2: 工资明细（金额/日期 + number_format） ---
    ws2 = wb.create_sheet("工资明细")
    ws2.append(["月份", "姓名", "实发金额", "发放日期"])
    ws2.append(["2024-06", "张三", 1234567.89, "2024-06-30"])
    ws2.append(["2024-07", "李四", 2345678.9, "2024-07-31"])
    for row in ws2.iter_rows(min_row=2, max_row=3, min_col=3, max_col=3):
        for cell in row:
            cell.number_format = "#,##0.00"
    for row in ws2.iter_rows(min_row=2, max_row=3, min_col=4, max_col=4):
        for cell in row:
            cell.number_format = "yyyy-mm-dd"

    # 元数据（断言清理）
    wb.properties.creator = "测试作者"
    wb.properties.title = "测试标题"
    wb.save(str(path))


def assert_original_unchanged(path, orig_bytes):
    assert path.read_bytes() == orig_bytes


def test_xlsx_parse(tmp_path):
    p = tmp_path / "sample.xlsx"
    make_xlsx(p)
    resp = client.post("/parse", json={"doc_id": "x1", "path": str(p)})
    assert resp.status_code == 200
    data = resp.json()
    assert data["ok"] is True and data["format"] == "xlsx"
    text = data["text"]
    for needle, _mask in SENSITIVE:
        assert needle in text, f"敏感串缺失: {needle}"
    assert "=COUNTA(B2:B3)" in text  # 公式单元格以公式字符串进入全文
    assert data["structure"]["sheet_count"] == 2
    assert {s["name"] for s in data["structure"]["sheets"]} == {"员工信息", "工资明细"}
    assert "员工信息!B4" in data["structure"]["formula_cells"]
    for sp in data["spans"]:
        assert text[sp["span_start"]:sp["span_end"]] == sp["text"], "span 偏移与全文不一致"
        assert sp["bbox"] is None
    # 公式单元格的 span 文本 = 公式字符串
    formula_spans = [sp for sp in data["spans"] if "=COUNTA" in sp["text"]]
    assert len(formula_spans) == 1


def test_xlsx_redact_chain(tmp_path):
    samples = tmp_path / "samples"
    out = tmp_path / "out"
    samples.mkdir()
    out.mkdir()
    p = samples / "sample.xlsx"
    make_xlsx(p)
    orig_bytes = p.read_bytes()

    resp = client.post("/parse", json={"doc_id": "x1", "path": str(p)})
    text = resp.json()["text"]
    repls = find_replacements(text)
    # 张三×2（员工信息+工资明细）+ 手机×1 + 身份证×1 + 银行卡×1
    assert len(repls) == 5

    resp = client.post(
        "/redact",
        json={"input_path": str(p), "replacements": repls, "output_dir": str(out)},
    )
    assert resp.status_code == 200
    body = resp.json()
    f = body["files"][0]
    assert f["is_deep"] is True
    assert f["replaced_count"] == len(repls)
    assert os.path.exists(f["output_path"])
    assert_original_unchanged(p, orig_bytes)

    # 重开输出：敏感原文零残留、掩码可见
    out_text = parse_file(f["output_path"]).text
    for needle, mask in SENSITIVE:
        assert needle not in out_text, f"敏感原文残留: {needle}"
        assert mask in out_text, f"掩码缺失: {mask}"
    # 非敏感内容保留
    assert "北京市朝阳区示例路88号" in out_text
    assert "1234567.89" in out_text


def test_xlsx_formatting_preserved(tmp_path):
    """重开输出 xlsx：number_format/合并单元格保留、core properties 已清。"""
    out = tmp_path / "out"
    out.mkdir()
    p = tmp_path / "sample.xlsx"
    make_xlsx(p)

    resp = client.post("/parse", json={"doc_id": "x1", "path": str(p)})
    repls = find_replacements(resp.json()["text"])
    resp = client.post(
        "/redact",
        json={"input_path": str(p), "replacements": repls, "output_dir": str(out)},
    )
    out_path = resp.json()["files"][0]["output_path"]

    from openpyxl import load_workbook

    wb = load_workbook(out_path)
    ws = wb["员工信息"]
    # 合并单元格保留
    merged = [str(rng) for rng in ws.merged_cells.ranges]
    assert len(merged) >= 1
    # 表头加粗保留
    assert ws["A1"].font.bold is True
    # 身份证列仍为文本格式
    assert ws["C2"].number_format == "@"
    ws2 = wb["工资明细"]
    assert ws2["C2"].number_format == "#,##0.00"
    assert ws2["D2"].number_format == "yyyy-mm-dd"
    # 公式保留
    assert ws["B4"].value == "=COUNTA(B2:B3)"
    # core properties 已清（openpyxl 保存空串后读回为 None）
    assert not wb.properties.creator
    assert not wb.properties.title


def test_xlsx_formula_hit_warned(tmp_path):
    """公式单元格命中敏感信息：不崩溃、warning 明示、公式保留。"""
    out = tmp_path / "out"
    out.mkdir()
    p = tmp_path / "sample.xlsx"
    make_xlsx(p)

    resp = client.post("/parse", json={"doc_id": "x1", "path": str(p)})
    text = resp.json()["text"]
    # 手工构造一个命中公式单元格的替换（公式字符串 =COUNTA(B2:B3) 内嵌）
    i = text.find("=COUNTA")
    repls = [{"start": i, "end": i + 8, "replacement": "XXXXXXX"}]

    resp = client.post(
        "/redact",
        json={"input_path": str(p), "replacements": repls, "output_dir": str(out)},
    )
    assert resp.status_code == 200
    f = resp.json()["files"][0]
    assert f["replaced_count"] == 0
    assert any("公式单元格" in w for w in f["warnings"]), f"warnings={f['warnings']}"

    from openpyxl import load_workbook

    wb = load_workbook(f["output_path"])
    assert wb["员工信息"]["B4"].value == "=COUNTA(B2:B3)"  # 公式原样保留
