"""墨隐 InkVeil sidecar — golden corpus 脱敏完整性回归（真实形态文档集）。

覆盖 PDF / DOCX / TXT / MD 四类真实形态文档，敏感类型含：
中文姓名 / 公司全称 / 手机 / 身份证（校验位合法）/ 银行卡（Luhn 合法）/
邮箱 / 地址。门槛：脱敏输出副本经重新解析后不得残留任何敏感原文；
原文件字节必须保持不变。配合 Rust 侧 `anonymizer-core/tests/golden.rs`
（规则层召回门禁 P0≥99% / P1≥95%）构成完整质量门禁。
"""

import os

import pytest
from fastapi.testclient import TestClient

from app import app
from parsers import parse_file
from redact import redact_file

client = TestClient(app)

# 真实形态敏感素材（与 Rust golden 保持一致校验合法性）
GOLDEN = [
    ("陈晓明", "陈**"),
    ("北京星河科技有限公司", "北京*********有限公司"),
    ("13800138000", "138****8000"),
    ("440301197407210509", "440301**********09"),
    ("6225760008219524", "622576********9524"),
    ("zhangsan@example.com", "zhan***@example.com"),
    ("北京市朝阳区建国路88号", "北京市朝阳区***路88号"),
]


def find_replacements(text: str) -> list[dict]:
    repls = []
    for needle, mask in sorted(GOLDEN, key=lambda kv: -len(kv[0])):
        pos = 0
        while True:
            i = text.find(needle, pos)
            if i < 0:
                break
            repls.append({"start": i, "end": i + len(needle), "replacement": mask})
            pos = i + len(needle)
    repls.sort(key=lambda r: r["start"])
    return repls


def assert_no_residue(text: str):
    for needle, _ in GOLDEN:
        assert needle not in text, f"脱敏输出残留敏感原文: {needle}"


# ---------------------------------------------------------------------------
# 样本生成
# ---------------------------------------------------------------------------


def make_pdf(path):
    from reportlab.lib.pagesizes import A4
    from reportlab.pdfbase import pdfmetrics
    from reportlab.pdfbase.cidfonts import UnicodeCIDFont
    from reportlab.pdfgen import canvas

    pdfmetrics.registerFont(UnicodeCIDFont("STSong-Light"))
    c = canvas.Canvas(str(path), pagesize=A4)
    lines = [
        "合同编号：GD-2024-0088",
        "甲方：陈晓明",
        "乙方：北京星河科技有限公司",
        "联系电话：13800138000",
        "身份证号：440301197407210509",
        "收款账号：6225760008219524",
        "联系邮箱：zhangsan@example.com",
        "送达地址：北京市朝阳区建国路88号",
    ]
    y = 760
    for line in lines:
        c.setFont("STSong-Light", 14)
        c.drawString(80, y, line)
        y -= 40
    c.showPage()
    c.save()


def make_docx(path):
    from docx import Document

    doc = Document()
    doc.add_paragraph("采购合同")
    doc.add_paragraph("甲方：陈晓明，身份证号 440301197407210509")
    doc.add_paragraph("乙方：北京星河科技有限公司，电话 13800138000")
    doc.add_paragraph("账号：6225760008219524，邮箱 zhangsan@example.com")
    doc.add_paragraph("地址：北京市朝阳区建国路88号")
    doc.save(str(path))


# ---------------------------------------------------------------------------
# 全格式脱敏完整性
# ---------------------------------------------------------------------------


@pytest.mark.parametrize("fmt", ["pdf", "docx", "txt", "md"])
def test_golden_redact_leaves_no_residue(tmp_path, fmt):
    src = tmp_path / f"golden.{fmt}"
    if fmt == "pdf":
        make_pdf(src)
    elif fmt == "docx":
        make_docx(src)
    elif fmt == "txt":
        src.write_text(
            "客户档案\n姓名：陈晓明\n公司：北京星河科技有限公司\n"
            "手机：13800138000\n身份证：440301197407210509\n"
            "卡号：6225760008219524\n邮箱：zhangsan@example.com\n"
            "住址：北京市朝阳区建国路88号\n",
            encoding="utf-8",
        )
    else:  # md
        src.write_text(
            "# 客户档案\n\n- 姓名：陈晓明\n- 公司：北京星河科技有限公司\n"
            "- 手机：13800138000\n- 身份证：440301197407210509\n"
            "- 卡号：6225760008219524\n- 邮箱：zhangsan@example.com\n"
            "- 住址：北京市朝阳区建国路88号\n",
            encoding="utf-8",
        )
    orig_bytes = src.read_bytes()
    out_dir = tmp_path / "out"
    out_dir.mkdir()

    parsed = parse_file(str(src))
    text = parsed.text
    for needle, _ in GOLDEN:
        assert needle in text, f"样本「{fmt}」未包含标注字段: {needle}"

    repls = find_replacements(text)
    assert repls, "样本应命中敏感字段"
    result = redact_file(str(src), repls, str(out_dir))

    # 原文件不变
    assert src.read_bytes() == orig_bytes
    # 输出存在且重新解析后无任何敏感原文残留
    out_path = result["output_path"]
    assert os.path.exists(out_path)
    out_text = parse_file(out_path).text
    assert_no_residue(out_text)
    # 输出应包含掩码（脱敏确实发生）
    assert any(mask in out_text for _, mask in GOLDEN)


def test_golden_parse_spans_consistent(tmp_path):
    """golden PDF 的 span 偏移与全文严格一致（脱敏定位正确性的前提）。"""
    pdf = tmp_path / "golden.pdf"
    make_pdf(pdf)
    resp = client.post("/parse", json={"path": str(pdf)})
    data = resp.json()
    assert data["ok"] is True
    text = data["text"]
    for sp in data["spans"]:
        assert text[sp["span_start"]:sp["span_end"]] == sp["text"]
        assert sp["bbox"] is not None and len(sp["bbox"]) == 4
