#!/usr/bin/env python3
"""墨隐 InkVeil 测试文档集生成器（本地调测验收 Part 3）。

输出到 testdocs/（提交进仓库，作为演示素材与回归样本）：
  01-房屋租赁合同.docx   合同正文（简称联动）+ 双方信息表格 + 付款计划 + 页眉页脚 + 文本框
  02-员工信息登记表.xlsx 多 sheet（员工信息 + 工资明细），合并表头、文本格式身份证/银行卡列
  03-合作框架协议.pdf    2 页合同形态（reportlab STSong-Light）
  04-会议纪要.txt
  05-需求说明.md
  06-批量文件夹/         供应商名录.docx + 报销单据说明.txt（批量脱敏演示）

敏感素材与 sidecar/tests/test_xlsx.py 的 SENSITIVE 保持一致，
均校验合法（身份证校验位正确、银行卡 Luhn 正确）。

用法：sidecar/.venv/bin/python scripts/gen_testdocs.py [输出目录]
默认输出目录：仓库根/testdocs/
"""

import os
import sys

# 敏感素材（与 sidecar/tests/test_xlsx.py 一致；身份证 440301197407210509 校验位合法、
# 银行卡 6222020200088888884 Luhn 合法）
NAME = "张三"
MOBILE = "13800138000"
ID_CARD = "440301197407210509"
BANK = "6222020200088888884"
EMAIL = "zhangsan@example.com"
ADDRESS = "北京市朝阳区示例路88号"
AMOUNT = "1234567.89"
DATE = "2024-06-30"
OTHER_NAME = "李四"
OTHER_MOBILE = "13900139000"
COMPANY = "XX科技有限公司"


def _repo_root() -> str:
    return os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def _add_textbox(paragraph, text: str) -> None:
    """在段落中插入一个 VML 文本框（w:pict -> v:shape -> v:textbox -> w:txbxContent）。"""
    from xml.sax.saxutils import escape

    from lxml import etree

    xml = (
        '<w:pict xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" '
        'xmlns:v="urn:schemas-microsoft-com:vml">'
        '<v:shape style="width:180pt;height:32pt"><v:textbox>'
        "<w:txbxContent><w:p><w:r><w:t>{}</w:t></w:r></w:p></w:txbxContent>"
        "</v:textbox></v:shape></w:pict>"
    ).format(escape(text))
    paragraph._p.append(etree.fromstring(xml.encode("utf-8")))


def _bold_runs(paragraph) -> None:
    from copy import copy

    for run in paragraph.runs:
        run.bold = True
        run.font.size = 12


# ---------------------------------------------------------------------------
# DOCX：01-房屋租赁合同.docx
# ---------------------------------------------------------------------------


def gen_lease_docx(path: str) -> None:
    from docx import Document

    doc = Document()
    # 页眉 / 页脚
    doc.sections[0].header.paragraphs[0].text = f"房屋租赁合同 - 内部资料（{COMPANY}）"
    doc.sections[0].footer.paragraphs[0].text = f"联系人：{NAME} {MOBILE}"

    # 标题
    h = doc.add_heading("房屋租赁合同", level=1)
    h.alignment = 1  # 居中
    doc.add_paragraph("合同编号：HT-2024-0001")
    doc.add_paragraph(f"签订日期：{DATE}")

    # 合同主体（简称联动模式）
    doc.add_paragraph("出租方（以下简称“甲方”）：")
    p = doc.add_paragraph(f"姓名：{NAME}　　身份证号：{ID_CARD}　　联系电话：{MOBILE}")
    _bold_runs(p)
    doc.add_paragraph(f"住址：{ADDRESS}")
    doc.add_paragraph("承租方（以下简称“乙方”）：")
    doc.add_paragraph(f"单位：{COMPANY}（统一社会信用代码：91110108MA01DEMO8X）")
    doc.add_paragraph(f"法定代表人：{OTHER_NAME}　　联系电话：{OTHER_MOBILE}")

    # 合同正文（用简称，验证简称联动后正文本身不重复敏感词）
    body = (
        "第一条 租赁标的\n"
        f"甲方将位于{ADDRESS}的房屋出租给乙方使用。\n\n"
        "第二条 租赁期限\n"
        "租赁期限自2024年7月1日起至2026年6月30日止。\n\n"
        "第三条 租金及支付方式\n"
        f"月租金为人民币（大写）壹佰万元整（¥{AMOUNT}元），乙方应于每月5日前支付至甲方指定账户。\n\n"
        "第四条 违约责任\n"
        "任何一方违反本合同约定，应向守约方支付违约金人民币伍万元整。"
    )
    for para in body.split("\n"):
        doc.add_paragraph(para)

    # 双方信息表格
    t = doc.add_table(rows=3, cols=2)
    t.style = "Table Grid"
    t.rows[0].cells[0].text = "甲方信息"
    t.rows[0].cells[1].text = "乙方信息"
    t.rows[1].cells[0].text = f"姓名：{NAME}\n身份证：{ID_CARD}\n手机：{MOBILE}"
    t.rows[1].cells[1].text = f"单位：{COMPANY}\n法人：{OTHER_NAME}\n手机：{OTHER_MOBILE}"
    t.rows[2].cells[0].text = f"收款银行卡：{BANK}\n邮箱：{EMAIL}"
    t.rows[2].cells[1].text = f"注册地址：北京市海淀区中关村示例大厦8层"

    # 付款计划表格（金额/日期/收款账户）
    doc.add_heading("付款计划", level=2)
    t2 = doc.add_table(rows=4, cols=4)
    t2.style = "Table Grid"
    for j, htext in enumerate(["期数", "应付金额", "付款日期", "收款账户"]):
        t2.rows[0].cells[j].text = htext
    for i, (period, amount, pay_date) in enumerate(
        [("第一期", AMOUNT, "2024-07-05"), ("第二期", AMOUNT, "2024-08-05"),
         ("第三期", AMOUNT, "2024-09-05")],
        start=1,
    ):
        t2.rows[i].cells[0].text = period
        t2.rows[i].cells[1].text = amount
        t2.rows[i].cells[2].text = pay_date
        t2.rows[i].cells[3].text = BANK

    # 文本框（备注）
    _add_textbox(doc.add_paragraph(), f"备注：甲方收款账户 {BANK}，开户行：中国建设银行示例支行")

    # 元数据（验证清理）
    doc.core_properties.author = f"{NAME}（{COMPANY}）"
    doc.core_properties.title = "房屋租赁合同"
    doc.core_properties.subject = "租赁合同"
    doc.core_properties.keywords = "租赁,合同,隐私"
    doc.core_properties.comments = f"内部文件，联系人 {NAME} {MOBILE}"
    doc.save(path)


# ---------------------------------------------------------------------------
# XLSX：02-员工信息登记表.xlsx
# ---------------------------------------------------------------------------


def gen_employees_xlsx(path: str) -> None:
    from copy import copy

    from openpyxl import Workbook

    wb = Workbook()
    # --- Sheet 1: 员工信息 ---
    ws = wb.active
    ws.title = "员工信息"
    ws.append(["序号", "姓名", "手机号", "身份证号", "银行卡号", "住址", "邮箱"])
    ws.append([1, NAME, MOBILE, ID_CARD, BANK, ADDRESS, EMAIL])
    ws.append([2, OTHER_NAME, OTHER_MOBILE, "110105199609043750", "6222020200099999993",
               "上海市浦东新区测试路1号", "lisi@example.com"])
    # 合并区（空区 G1:H1，不影响全文文本）
    ws.merge_cells("H1:I1")
    # 身份证/银行卡列设为文本格式（避免数值精度截断）
    for row in ws.iter_rows(min_row=2, max_row=3, min_col=4, max_col=5):
        for cell in row:
            cell.number_format = "@"
    for cell in ws[1]:
        new_font = copy(cell.font)
        new_font.bold = True
        cell.font = new_font

    # --- Sheet 2: 工资明细 ---
    ws2 = wb.create_sheet("工资明细")
    ws2.append(["月份", "姓名", "实发金额", "发放日期"])
    ws2.append(["2024-06", NAME, 1234567.89, "2024-06-30"])
    ws2.append(["2024-07", OTHER_NAME, 2345678.9, "2024-07-31"])
    for row in ws2.iter_rows(min_row=2, max_row=3, min_col=3, max_col=3):
        for cell in row:
            cell.number_format = "#,##0.00"
    for row in ws2.iter_rows(min_row=2, max_row=3, min_col=4, max_col=4):
        for cell in row:
            cell.number_format = "yyyy-mm-dd"

    wb.properties.creator = f"{COMPANY} HR"
    wb.properties.title = "员工信息登记表"
    wb.save(path)


# ---------------------------------------------------------------------------
# PDF：03-合作框架协议.pdf
# ---------------------------------------------------------------------------


def gen_agreement_pdf(path: str) -> None:
    from reportlab.lib.pagesizes import A4
    from reportlab.pdfbase import pdfmetrics
    from reportlab.pdfbase.cidfonts import UnicodeCIDFont
    from reportlab.pdfgen import canvas

    pdfmetrics.registerFont(UnicodeCIDFont("STSong-Light"))
    c = canvas.Canvas(path, pagesize=A4)
    y = 770
    page1 = [
        "合作框架协议",
        f"甲方：{COMPANY}",
        f"法定代表人：{OTHER_NAME}，联系电话：{OTHER_MOBILE}",
        "乙方（个人）：",
        f"姓名：{NAME}",
        f"身份证号：{ID_CARD}",
        f"联系电话：{MOBILE}",
        f"电子邮箱：{EMAIL}",
        f"联系地址：{ADDRESS}",
        "",
        "第一条 合作内容",
        "双方本着平等自愿、诚实信用的原则，就示例项目建立长期合作关系。",
        "第二条 报酬与支付",
        f"合作费用共计人民币¥{AMOUNT}元，乙方应于{DATE}前支付至甲方指定账户。",
        f"收款账户：{BANK}（开户行：中国建设银行示例支行）",
        "第三条 保密条款",
        "双方对本协议内容及合作过程中知悉的商业信息负有保密义务。",
    ]
    for line in page1:
        c.setFont("STSong-Light", 13 if line in ("合作框架协议",) else 11)
        c.drawString(70, y, line)
        y -= 26
    c.showPage()

    y = 770
    page2 = [
        "合作框架协议（续）",
        "",
        "第四条 争议解决",
        "因本协议引起的争议，双方应友好协商解决；协商不成的，提交甲方所在地人民法院诉讼。",
        "第五条 协议生效",
        "本协议一式两份，甲乙双方各执一份，自双方签字（盖章）之日起生效。",
        "",
        f"甲方（盖章）：{COMPANY}",
        f"法定代表人签字：{OTHER_NAME}",
        f"乙方签字：{NAME}（身份证号：{ID_CARD}）",
        f"签订日期：{DATE}",
    ]
    for line in page2:
        c.setFont("STSong-Light", 13 if line == "合作框架协议（续）" else 11)
        c.drawString(70, y, line)
        y -= 26
    c.showPage()
    c.save()


# ---------------------------------------------------------------------------
# TXT / MD
# ---------------------------------------------------------------------------


def gen_meeting_minutes(path: str) -> None:
    content = f"""会议纪要
会议主题：2024年上半年项目复盘会
会议时间：{DATE} 14:00-16:30
主持人：{NAME}
参会人：{NAME}、{OTHER_NAME}、{COMPANY}各项目组代表

一、项目进度
1. 核心功能已进入验收阶段，整体进度符合预期。

二、关键决议
1. 敏感数据处理：所有文档统一走脱敏流程后方可外发。
2. 外发联系人统一登记：{NAME}（{MOBILE}），备用 {EMAIL}。

三、遗留事项
1. 预算核对：上半年项目支出合计 ¥{AMOUNT} 元，请财务复核。
2. 结算账户：员工报销统一转入 {BANK}。
"""
    with open(path, "w", encoding="utf-8") as f:
        f.write(content)


def gen_requirement_md(path: str) -> None:
    content = f"""# 需求说明：内部资料脱敏平台二期

## 1. 背景

第一期已实现 PDF / Word / Excel 等文档的敏感信息检测与脱敏，
二期需支持批量处理与审计追踪。

## 2. 联系人

- 产品负责人：{NAME}（{MOBILE}，{EMAIL}）
- 技术负责人：{OTHER_NAME}（{OTHER_MOBILE}）

## 3. 业务规则

- 手机号：11 位，以 1 开头
- 身份证号：18 位，最后一位可为 X
- 银行卡号：16-19 位数字

## 4. 验收口径

批量导入示例数据位于 `{ADDRESS}` 机房样例目录，验收后需清除。
"""
    with open(path, "w", encoding="utf-8") as f:
        f.write(content)


# ---------------------------------------------------------------------------
# 06-批量文件夹
# ---------------------------------------------------------------------------


def gen_vendor_docx(path: str) -> None:
    from docx import Document

    doc = Document()
    doc.add_heading("供应商名录", level=1)
    doc.add_paragraph(f"本名录为 {COMPANY} 2024 年度合作供应商登记表，请勿外传。")
    t = doc.add_table(rows=3, cols=4)
    t.style = "Table Grid"
    for j, htext in enumerate(["供应商", "联系人", "联系电话", "结算账户"]):
        t.rows[0].cells[j].text = htext
    t.rows[1].cells[0].text = "示例商贸有限公司"
    t.rows[1].cells[1].text = NAME
    t.rows[1].cells[2].text = MOBILE
    t.rows[1].cells[3].text = BANK
    t.rows[2].cells[0].text = "示例信息技术有限公司"
    t.rows[2].cells[1].text = OTHER_NAME
    t.rows[2].cells[2].text = OTHER_MOBILE
    t.rows[2].cells[3].text = "6222020200088776659"
    doc.save(path)


def gen_expense_txt(path: str) -> None:
    content = f"""报销单据说明
报销人：{NAME}（员工编号 E-0001）
报销金额：¥{AMOUNT} 元
报销日期：{DATE}
收款银行卡：{BANK}
联系电话：{MOBILE}

说明：
1. 请财务在 3 个工作日内完成打款。
2. 如有疑问请联系报销人本人。
"""
    with open(path, "w", encoding="utf-8") as f:
        f.write(content)


# ---------------------------------------------------------------------------
# 主入口
# ---------------------------------------------------------------------------


def main() -> None:
    out_root = sys.argv[1] if len(sys.argv) > 1 else os.path.join(_repo_root(), "testdocs")
    os.makedirs(out_root, exist_ok=True)
    batch_dir = os.path.join(out_root, "06-批量文件夹")
    os.makedirs(batch_dir, exist_ok=True)

    jobs = [
        ("01-房屋租赁合同.docx", gen_lease_docx),
        ("02-员工信息登记表.xlsx", gen_employees_xlsx),
        ("03-合作框架协议.pdf", gen_agreement_pdf),
        ("04-会议纪要.txt", gen_meeting_minutes),
        ("05-需求说明.md", gen_requirement_md),
        (os.path.join("06-批量文件夹", "供应商名录.docx"), gen_vendor_docx),
        (os.path.join("06-批量文件夹", "报销单据说明.txt"), gen_expense_txt),
    ]
    for rel, fn in jobs:
        p = os.path.join(out_root, rel)
        fn(p)
        print(f"  ✓ {os.path.relpath(p, _repo_root())}（{os.path.getsize(p)}B）")
    print(f"测试文档集已生成: {out_root}")


if __name__ == "__main__":
    main()
