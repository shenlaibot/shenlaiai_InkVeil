#!/usr/bin/env python3
"""生成“复杂、带隐藏内容”的合成测试语料及标准答案（truth.json）。

用法: sidecar/.venv/bin/python scripts/gen_complex_corpus.py <输出目录>

覆盖此前被漏掉/易出错的真实场景：
- DOCX: 拆 run 的手机号、嵌套表格、页眉页脚、脚注、批注(含作者)、修订插入/删除、超链接(含 mailto 目标)、
        内容控件、文本框 + mc:Fallback 回退副本、图片替代文字、核心/扩展/自定义属性
- XLSX: 共享字符串、数值型手机号、公式字面量、批注、页眉页脚、图表、合并单元格、超链接显示文字
- PDF : 真实 CJK 子集化 TrueType + Identity-H 十六进制字符串（旧实现对其无效）、英文 Helvetica、
        旋转页、多页
- TXT/MD: UTF-8 CRLF、GBK、UTF-8 BOM
truth.json: [{file, secret, kind, detectable}]。detectable=True 表示当前规则应当能检出并脱敏，
输出文件中不得残留；False 为已知的检测盲区（仅用于统计，不判失败）。
"""

from __future__ import annotations

import io
import json
import os
import sys
import zipfile

W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"

TRUTH: list = []


def truth(file, secret, kind, detectable=True):
    TRUTH.append({"file": file, "secret": secret, "kind": kind, "detectable": detectable})
    return secret


# ---------------------------------------------------------------- 数据生成 --


def _id18(prefix17: str) -> str:
    weights = [7, 9, 10, 5, 8, 4, 2, 1, 6, 3, 7, 9, 10, 5, 8, 4, 2]
    codes = "10X98765432"
    s = sum(int(c) * w for c, w in zip(prefix17, weights))
    return prefix17 + codes[s % 11]


def _luhn_card(prefix15: str) -> str:
    def luhn_ok(num):
        total, alt = 0, False
        for d in reversed(num):
            n = int(d)
            if alt:
                n *= 2
                if n > 9:
                    n -= 9
            total += n
            alt = not alt
        return total % 10 == 0

    for c in "0123456789":
        if luhn_ok(prefix15 + c):
            return prefix15 + c
    raise AssertionError


class Pool:
    """每份文件取一组互不相同的敏感值，避免跨文件子串碰撞。"""

    NAMES = ["张明辉", "李国强", "王秀英", "赵文博", "孙晓峰", "周建国", "吴天明", "郑海涛",
             "冯志远", "陈美玲", "杨建华", "黄思远", "徐静怡", "朱文斌", "马永康", "胡雪梅"]
    COMPANIES = ["北京云图科技有限公司", "上海星辰数据有限公司", "深圳远航信息有限公司", "杭州启明网络有限公司",
                 "成都天府软件有限公司", "武汉光谷智能有限公司", "南京金陵科技有限公司", "广州珠江数据有限公司"]

    def __init__(self):
        self.i = 0

    def take(self):
        self.i += 1
        k = self.i
        return {
            "name": [self.NAMES[(k * 3 + j) % len(self.NAMES)] for j in range(4)],
            "company": [self.COMPANIES[(k * 2 + j) % len(self.COMPANIES)] for j in range(2)],
            "mobile": [f"13{k % 10}{(k * 7) % 10}{(k * 3) % 10}{10000000 + k * 7919:08d}"[:11] for _ in range(1)]
            + [f"15{(k + j) % 10}{(10000000 + k * 104729 + j * 31) % 100000000:08d}"[:11] for j in range(1, 5)],
            "id": [_id18(f"4403011990{(k * 3 + j) % 12 + 1:02d}{(k * 5 + j) % 28 + 1:02d}{100 + k * 7 + j:03d}") for j in range(3)],
            "card": [_luhn_card(f"62220202{(k * 100003 + j * 7919) % 10**7:07d}") for j in range(2)],
            "email": [f"user{k}{j}@corp-example{k}.com" for j in range(5)],
            "case": f"（2024）京0105民初{12000 + k}号",
            "contract": f"HT-2024-{4000 + k}",
            "addr": f"北京市朝阳区建国路{80 + k}号院3号楼2单元50{k % 9 + 1}室",
        }


# ------------------------------------------------------------------ DOCX --


def _docx(path, v):
    from docx import Document
    from docx.shared import Inches
    from PIL import Image

    fn = os.path.basename(path)
    doc = Document()
    doc.core_properties.author = truth(fn, v["name"][3], "name", False)  # 属性里的作者：必须被清理（复检额外检查）
    doc.add_heading("技术服务合同", 1)
    doc.add_paragraph(f"合同编号：{truth(fn, v['contract'], 'contract_no')}  案号：{truth(fn, v['case'], 'case_no')}")
    doc.add_paragraph(f"甲方：{truth(fn, v['company'][0], 'company')}")
    doc.add_paragraph(f"联系人：{truth(fn, v['name'][0], 'name')}，联系电话：{truth(fn, v['mobile'][0], 'mobile')}。")

    # 拆 run 的手机号（格式不同的片段）
    m2 = truth(fn, v["mobile"][1], "mobile")
    p = doc.add_paragraph("备用电话：")
    r1 = p.add_run(m2[:3]); r1.bold = True
    p.add_run(m2[3:7])
    r3 = p.add_run(m2[7:]); r3.italic = True

    # 表格 + 嵌套表格
    t = doc.add_table(rows=2, cols=2)
    t.cell(0, 0).text = "身份证号"
    t.cell(0, 1).text = truth(fn, v["id"][0], "id_card")
    t.cell(1, 0).text = "收款账号"
    outer = t.cell(1, 1)
    outer.text = truth(fn, v["card"][0], "bank_card")
    inner = outer.add_table(rows=1, cols=1)
    inner.cell(0, 0).text = f"邮箱：{truth(fn, v['email'][0], 'email')}"

    # 页眉页脚
    sec = doc.sections[0]
    sec.header.paragraphs[0].text = f"甲方：{truth(fn, v['company'][1], 'company')}"
    sec.footer.paragraphs[0].text = f"联系人：{truth(fn, v['name'][1], 'name')}"

    # 图片 + 替代文字（含敏感）
    buf = io.BytesIO()
    Image.new("RGB", (8, 8), (200, 200, 200)).save(buf, "PNG")
    buf.seek(0)
    doc.add_picture(buf, width=Inches(0.5))
    alt_secret = truth(fn, v["mobile"][4], "mobile")

    tmp = path + ".base"
    doc.save(tmp)

    # ---- 注入 python-docx 不会生成/遍历的部位 ----
    ins_email = truth(fn, v["email"][1], "email")
    del_mobile = truth(fn, v["mobile"][2], "mobile")
    link_email = truth(fn, v["email"][2], "email")
    sdt_id = truth(fn, v["id"][1], "id_card")
    tb_name = truth(fn, v["name"][2], "name")
    foot_bank = truth(fn, v["card"][1], "bank_card")
    comment_mobile = truth(fn, v["mobile"][3], "mobile")
    app_company = truth(fn, v["company"][0] + "（扩展属性）", "company", False)
    custom_email = truth(fn, v["email"][3], "email")

    body_extra = (
        f'<w:p><w:ins w:id="101" w:author="对方律师" w:date="2024-01-01T00:00:00Z"><w:r><w:t>新增邮箱 {ins_email}</w:t></w:r></w:ins>'
        f'<w:del w:id="102" w:author="对方律师" w:date="2024-01-01T00:00:00Z"><w:r><w:delText>原电话 {del_mobile}</w:delText></w:r></w:del></w:p>'
        f'<w:p><w:hyperlink r:id="rIdMail" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:r><w:t>联系邮箱 {link_email}</w:t></w:r></w:hyperlink></w:p>'
        f'<w:sdt><w:sdtPr><w:alias w:val="id"/></w:sdtPr><w:sdtContent><w:p><w:r><w:t>身份证 {sdt_id}</w:t></w:r></w:p></w:sdtContent></w:sdt>'
        '<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> HYPERLINK "mailto:' + link_email + '" </w:instrText></w:r>'
        '<w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>点此发信</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>'
        '<w:p><w:r><w:rPr><w:vanish/></w:rPr><w:t>隐藏文字：联系人：' + v["name"][3] + '</w:t></w:r></w:p>'
        '<w:p><w:r><mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006">'
        f'<mc:Choice Requires="wps"><w:txbxContent><w:p><w:r><w:t>联系人：{tb_name}</w:t></w:r></w:p></w:txbxContent></mc:Choice>'
        f'<mc:Fallback><w:txbxContent><w:p><w:r><w:t>联系人：{tb_name}</w:t></w:r></w:p></w:txbxContent></mc:Fallback>'
        '</mc:AlternateContent></w:r></w:p>'
        f'<w:p><w:r><w:t>脚注引用</w:t></w:r><w:r><w:footnoteReference w:id="1"/></w:r></w:p>'
        f'<w:p><w:r><w:commentRangeStart w:id="0"/><w:t>批注范围</w:t><w:commentRangeEnd w:id="0"/><w:commentReference w:id="0"/></w:r></w:p>'
    )
    # 隐藏文字里的姓名是 v['name'][3]（已作为作者登记，detectable False）
    footnotes = (
        f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:footnotes xmlns:w="{W}">'
        '<w:footnote w:id="-1" w:type="separator"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>'
        f'<w:footnote w:id="1"><w:p><w:r><w:t>收款账号 {foot_bank}</w:t></w:r></w:p></w:footnote></w:footnotes>'
    )
    comments = (
        f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:comments xmlns:w="{W}">'
        f'<w:comment w:id="0" w:author="审核人{v["name"][2]}" w:initials="SH"><w:p><w:r><w:t>请核对手机 {comment_mobile}</w:t></w:r></w:p></w:comment></w:comments>'
    )
    truth(fn, "审核人" + v["name"][2], "author", False)
    custom = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/custom-properties" '
        'xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes">'
        f'<property fmtid="{{D5CDD505-2E9C-101B-9397-08002B2CF9AE}}" pid="2" name="owner"><vt:lpwstr>{custom_email}</vt:lpwstr></property></Properties>'
    )

    with zipfile.ZipFile(tmp) as zin, zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as zout:
        for info in zin.infolist():
            data = zin.read(info.filename)
            if info.filename == "word/document.xml":
                s = data.decode("utf-8")
                s = s.replace("<w:sectPr", body_extra + "<w:sectPr", 1)
                # 图片替代文字
                s = s.replace('descr=""', f'descr="手机 {alt_secret}"')
                if "descr=" not in s:
                    s = s.replace("<wp:docPr ", f'<wp:docPr descr="手机 {alt_secret}" ', 1)
                data = s.encode("utf-8")
            elif info.filename == "word/_rels/document.xml.rels":
                s = data.decode("utf-8")
                s = s.replace(
                    "</Relationships>",
                    f'<Relationship Id="rIdMail" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="mailto:{link_email}" TargetMode="External"/></Relationships>',
                )
                data = s.encode("utf-8")
            elif info.filename == "docProps/app.xml":
                s = data.decode("utf-8")
                if "<Company>" in s:
                    s = s.replace("<Company></Company>", f"<Company>{app_company}</Company>").replace("<Company/>", f"<Company>{app_company}</Company>")
                else:
                    s = s.replace("</Properties>", f"<Company>{app_company}</Company></Properties>")
                data = s.encode("utf-8")
            zout.writestr(info, data)
        zout.writestr("word/footnotes.xml", footnotes)
        zout.writestr("word/comments.xml", comments)
        zout.writestr("docProps/custom.xml", custom)
    os.remove(tmp)


# ------------------------------------------------------------------ XLSX --


def _xlsx(path, v):
    from openpyxl import Workbook
    from openpyxl.chart import BarChart, Reference
    from openpyxl.comments import Comment

    fn = os.path.basename(path)
    wb = Workbook()
    ws = wb.active
    ws.title = "名册"
    ws.append(["姓名", "手机", "身份证", "备注"])
    names = [v["name"][0], v["name"][1]]
    # 共享字符串：同一文本出现多次
    ws.append([f"联系人：{truth(fn, names[0], 'name')}", int(truth(fn, v["mobile"][0], "mobile")), truth(fn, v["id"][0], "id_card"), "重点客户"])
    ws.append([f"联系人：{truth(fn, names[1], 'name')}", truth(fn, v["mobile"][1], "mobile"), truth(fn, v["id"][1], "id_card"), "重点客户"])
    ws.append(["公式", '="联系人："&"' + truth(fn, v["name"][2], "name") + '"', "", ""])
    ws["D2"].comment = Comment(f"备用手机 {truth(fn, v['mobile'][2], 'mobile')}", "对方律师")
    ws.oddHeader.center.text = f"甲方：{truth(fn, v['company'][0], 'company')}"
    ws.oddFooter.center.text = f"邮箱 {truth(fn, v['email'][0], 'email')}"
    ws.merge_cells("A6:C6")
    ws["A6"] = f"开户账号：{truth(fn, v['card'][0], 'bank_card')}"
    ws["B8"].hyperlink = f"mailto:{v['email'][1]}"
    ws["B8"] = truth(fn, v["email"][1], "email")
    truth(fn, v["email"][1], "email")  # 超链接目标里的同一邮箱
    ws["E1"], ws["E2"], ws["E3"] = "销量", 120, 340
    chart = BarChart()
    chart.title = "季度销量"
    chart.add_data(Reference(ws, min_col=5, min_row=1, max_row=3), titles_from_data=True)
    ws.add_chart(chart, "G2")

    ws2 = wb.create_sheet("明细")
    for i in range(1, 30):
        ws2.append([f"记录{i}", f"联系人：{v['name'][i % 4]}" if i % 5 == 0 else "普通内容", i * 100])
    truth(fn, v["name"][3], "name")  # 明细表里 name[3] 出现多次（共享字符串）
    wb.properties.creator = truth(fn, v["name"][1], "author", False)
    wb.save(path)


# ------------------------------------------------------------------- PDF --


def _cjk_font_pdf(path, pages, v, fn):
    """真实 CJK TrueType 子集 + Identity-H 十六进制字符串（真实世界 PDF 的常见形态）。"""
    font_path = "/System/Library/Fonts/Supplemental/Arial Unicode.ttf"
    if not os.path.isfile(font_path):
        return False  # 无该字体的环境（如 Linux CI）跳过此样本
    try:
        from fontTools import subset
        from fontTools.ttLib import TTFont
    except ImportError:
        return False
    import pikepdf
    from pikepdf import Array, Dictionary, Name, String

    chars = sorted({c for page in pages for _, _, t in page for c in t})
    opts = subset.Options()
    opts.layout_features = []
    opts.name_IDs = []
    opts.notdef_outline = True
    opts.glyph_names = False
    sub = subset.Subsetter(opts)
    f = TTFont(font_path)
    sub.populate(text="".join(chars))
    sub.subset(f)
    buf = io.BytesIO()
    f.save(buf)
    font_bytes = buf.getvalue()
    sf = TTFont(io.BytesIO(font_bytes))
    cmap = sf.getBestCmap()
    upem = sf["head"].unitsPerEm
    hmtx = sf["hmtx"]
    gid_of = {c: sf.getGlyphID(cmap[ord(c)]) for c in chars if ord(c) in cmap}

    pdf = pikepdf.new()
    ff = pikepdf.Stream(pdf, font_bytes)
    ff.Length1 = len(font_bytes)
    desc = pdf.make_indirect(Dictionary(
        Type=Name.FontDescriptor, FontName=Name("/AAAAAA+ArialUnicodeMS"), Flags=4,
        FontBBox=Array([-1000, -300, 2000, 1000]), ItalicAngle=0, Ascent=900, Descent=-200,
        CapHeight=700, StemV=80, FontFile2=pdf.make_indirect(ff)))
    widths = Array()
    for c, g in sorted(gid_of.items(), key=lambda kv: kv[1]):
        widths.append(g)
        widths.append(Array([int(hmtx[sf.getGlyphName(g)][0] * 1000 / upem)]))
    # ToUnicode
    bf = "\n".join(f"<{g:04X}> <{ord(c):04X}>" for c, g in sorted(gid_of.items(), key=lambda kv: kv[1]) if ord(c) < 0x10000)
    cmap_txt = (
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n"
        "/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n"
        "/CMapName /Adobe-Identity-UCS def /CMapType 2 def\n"
        "1 begincodespacerange <0000> <FFFF> endcodespacerange\n"
        f"{len(gid_of)} beginbfchar\n{bf}\nendbfchar\nendcmap CMapName currentdict /CMap defineresource pop end end"
    )
    cid = pdf.make_indirect(Dictionary(
        Type=Name.Font, Subtype=Name.CIDFontType2, BaseFont=Name("/AAAAAA+ArialUnicodeMS"),
        CIDSystemInfo=Dictionary(Registry=String("Adobe"), Ordering=String("Identity"), Supplement=0),
        FontDescriptor=desc, CIDToGIDMap=Name.Identity, DW=1000, W=widths))
    font = pdf.make_indirect(Dictionary(
        Type=Name.Font, Subtype=Name.Type0, BaseFont=Name("/AAAAAA+ArialUnicodeMS"),
        Encoding=Name("/Identity-H"), DescendantFonts=Array([cid]),
        ToUnicode=pikepdf.Stream(pdf, cmap_txt.encode("ascii"))))
    for lines in pages:
        ops = ["BT /F1 14 Tf"]
        for x, y, t in lines:
            hexs = "".join(f"{gid_of[c]:04X}" for c in t if c in gid_of)
            ops.append(f"1 0 0 1 {x} {y} Tm <{hexs}> Tj")
        ops.append("ET")
        page = pdf.add_blank_page(page_size=(420, 320))
        page.Resources = Dictionary(Font=Dictionary(F1=font))
        page.Contents = pikepdf.Stream(pdf, "\n".join(ops).encode("ascii"))
    pdf.docinfo["/Author"] = truth(fn, v["name"][3], "author", False)
    pdf.save(path)
    return True


def _pdf_cjk(path, v):
    fn = os.path.basename(path)
    pages = []
    for k in range(4):
        pages.append([
            (30, 260, f"合同编号：{truth(fn, v['contract'] + f'-{k}', 'contract_no')}"),
            (30, 230, f"联系人：{truth(fn, v['name'][k % 4], 'name')}  电话：{truth(fn, v['mobile'][k % 5], 'mobile')}"),
            (30, 200, f"甲方：{truth(fn, v['company'][k % 2], 'company')}"),
            (30, 170, "本页其余内容为普通条款，应保持可检索。"),
        ])
    pages.append([(30, 260, "第五页没有任何敏感信息，应保持原样。")])
    return _cjk_font_pdf(path, pages, v, fn)


def _pdf_latin(path, v, rotate=0, npages=3):
    import pikepdf
    from pikepdf import Dictionary, Name

    fn = os.path.basename(path)
    pdf = pikepdf.new()
    font = pdf.make_indirect(Dictionary(Type=Name.Font, Subtype=Name.Type1, BaseFont=Name.Helvetica, Encoding=Name.WinAnsiEncoding))
    for k in range(npages):
        ops = ["BT /F1 12 Tf"]
        lines = [f"Contact mail {truth(fn, v['email'][k % 5], 'email')} today",
                 f"phone {truth(fn, v['mobile'][k % 5], 'mobile')} urgent",
                 "Ordinary clause stays searchable."]
        for i, t in enumerate(lines):
            ops.append(f"1 0 0 1 30 {250 - i * 30} Tm ({t}) Tj")
        ops.append("ET")
        page = pdf.add_blank_page(page_size=(400, 300))
        page.Resources = Dictionary(Font=Dictionary(F1=font))
        page.Contents = pikepdf.Stream(pdf, "\n".join(ops).encode("latin-1"))
        if rotate:
            page.Rotate = rotate
    pdf.docinfo["/Author"] = truth(fn, v["name"][2], "author", False)
    pdf.save(path)


# ------------------------------------------------------------ 更难的场景 --


def _docx_hard(path, v):
    """简称联动 / 生僻姓名全文传播 / 占位符与汉字紧邻 / 带空格银行卡 / +86 / 座机 / 合并单元格 / 多节页眉。"""
    from docx import Document

    fn = os.path.basename(path)
    doc = Document()
    doc.add_heading("补充协议", 1)

    # 1) 简称联动：全称出现一次，后文只用简称
    full = v["company"][0]
    short = full[2:4] + "公司"   # 例：云图公司
    truth(fn, full, "company")
    truth(fn, short, "company_alias")
    doc.add_paragraph(f"甲方：{full}（以下简称“{short}”）")
    doc.add_paragraph(f"{short}应在收到通知后三日内付款，{short}逾期的，按日计息。")

    # 2) 生僻姓名：只有第一次出现带“联系人：”前缀，后文是无前缀的正文
    rare = "陈瑾瑜"
    truth(fn, rare, "name_propagation")
    doc.add_paragraph(f"联系人：{rare}")
    doc.add_paragraph(f"经协商，{rare}负责对接，{rare}应于三日内回复。")

    # 3) 占位符与汉字紧邻（还原时曾因“两侧是汉字”而失败）
    c2 = v["company"][1]
    doc.add_paragraph(f"甲方{truth(fn, v['company'][0], 'company')}委托乙方：{truth(fn, c2, 'company')}办理。")
    doc.add_paragraph(f"联系人：{truth(fn, v['name'][0], 'name')}签字确认。")

    # 4) 格式多样的号码
    card = v["card"][0]
    spaced = " ".join(card[i:i + 4] for i in range(0, len(card), 4))
    doc.add_paragraph(f"收款账号：{truth(fn, spaced, 'bank_card')}")
    m = v["mobile"][0]
    doc.add_paragraph(f"电话：+86 {m[:3]} {m[3:7]} {m[7:]}")
    truth(fn, f"{m[:3]} {m[3:7]} {m[7:]}", "mobile")
    doc.add_paragraph(f"传真：{truth(fn, '010-62345678', 'phone')}")
    doc.add_paragraph(f"邮箱：{truth(fn, v['email'][0].upper(), 'email')}")

    # 5) 合并单元格：同一个 tc 被多次返回（曾导致重复替换写坏文本）
    t = doc.add_table(rows=2, cols=3)
    a = t.cell(0, 0).merge(t.cell(0, 2))
    a.text = f"紧急联系电话：{truth(fn, v['mobile'][2], 'mobile')}"
    t.cell(1, 0).text = "备注"
    t.cell(1, 1).text = "普通"
    t.cell(1, 2).text = "内容"

    # 6) 多节：不同的首页页眉
    from docx.enum.section import WD_SECTION
    sec2 = doc.add_section(WD_SECTION.NEW_PAGE)
    sec2.different_first_page_header_footer = True
    sec2.first_page_header.paragraphs[0].text = f"首页页眉 联系人：{truth(fn, v['name'][1], 'name')}"
    sec2.header.is_linked_to_previous = False
    sec2.header.paragraphs[0].text = f"正文页眉 {truth(fn, v['email'][1], 'email')}"
    doc.add_paragraph("第二节正文，没有敏感信息。")
    doc.save(path)


def _big_docx(path, v, paras=3000):
    from docx import Document

    fn = os.path.basename(path)
    doc = Document()
    for i in range(paras):
        if i % 60 == 0:
            doc.add_paragraph(f"联系人：{Pool.NAMES[(i // 60) % 16]}，电话：{truth(fn, f'139{i:08d}'[:11], 'mobile')}。")
        else:
            doc.add_paragraph(f"第{i}条 普通条款内容，不含任何个人信息，用于测试大文档性能。" * 2)
    doc.save(path)


def _big_xlsx(path, v, rows=6000):
    from openpyxl import Workbook

    fn = os.path.basename(path)
    wb = Workbook()
    ws = wb.active
    ws.title = "大表"
    for i in range(rows):
        ws.append([f"记录{i}", "普通内容", i])
        if i % 100 == 0:
            ws.append([f"联系人：{Pool.NAMES[(i // 100) % 16]}", truth(fn, f"138{i:08d}"[:11], "mobile"), i])
    wb.save(path)


def _formats_txt(outdir, v):
    """检测规则覆盖：各种常见写法。detectable=True 表示产品应当识别。"""
    fn = "formats.txt"
    m = v["mobile"][0]
    card = v["card"][0]
    idn = v["id"][0]
    lines = [
        # 手机号写法
        f"手机：+86 {m}",
        f"手机：0086-{m[:3]}-{m[3:7]}-{m[7:]}",
        f"手机：（86）{v['mobile'][1]}",
        f"手机：{''.join(chr(0xFF10 + int(c)) for c in v['mobile'][2])}",       # 全角数字
        f"客服电话 {v['mobile'][3][:3]} {v['mobile'][3][3:7]} {v['mobile'][3][7:]}",
        # 座机
        "座机：(010)62345678", "座机：010 62345679", "座机：0755-82345670-8001", "座机：021-62345671",
        # 证件
        f"身份证：{idn}", f"身份证：{idn[:6]} {idn[6:14]} {idn[14:]}", f"身份证：{v['id'][1][:17]}{v['id'][1][17].lower()}",
        # 银行卡/账号
        f"卡号：{'-'.join(card[i:i + 4] for i in range(0, len(card), 4))}",
        f"对公账号：{v['case'] and '11001234567890123'}",
        # 邮箱
        f"邮箱：zhang.san+tag{v['email'][0]}", f"Email: {v['email'][1].upper()}",
        # 护照/港澳台
        f"护照号：E{12345600 + len(v['name'][0]) + 7}", f"港澳通行证：C{23456700 + 11}",
        # 车牌、IP、URL
        "车牌：京A12345", "服务器 IP：192.168.31.77", "网址：https://crm.example.com/u/10086",
        # 统一社会信用代码
        "统一社会信用代码：91110108MA01ABCD5X",
        # 无前缀正文里的姓名
        f"会上{v['name'][2]}提出了意见，{v['name'][2]}认为应当延期。",
        f"{v['name'][3][0]}经理已确认，{v['name'][3][0]}总稍后回复。",
        # 地址
        f"住址：{v['addr']}", f"公司位于{v['addr']}，欢迎来访。",
        # 案号
        "案号：〔2024〕京仲案字第0123号", "（2023）最高法民终123号",
        # 港澳台证件 / 繁体前缀
        "香港身份證：A123456(7)", "台灣身份證字號：A123456789", "聯絡人：張明輝，負責人：李國強",
    ]
    truths = [
        (f"+86 {m}", "mobile", True), (f"{m[:3]}-{m[3:7]}-{m[7:]}", "mobile", True),
        (v["mobile"][1], "mobile", True), (v["mobile"][2], "mobile_fullwidth", True),
        (f"{v['mobile'][3][:3]} {v['mobile'][3][3:7]} {v['mobile'][3][7:]}", "mobile", True),
        ("62345678", "phone", True), ("62345679", "phone", True), ("82345670", "phone", True), ("62345671", "phone", True),
        (idn, "id_card", True), (f"{idn[:6]} {idn[6:14]} {idn[14:]}", "id_card_spaced", True), (v["id"][1][:17], "id_card_lower_x", True),
        ("-".join(card[i:i + 4] for i in range(0, len(card), 4)), "bank_card", True),
        ("11001234567890123", "bank_account_17", True),
        (v["email"][1].upper(), "email", True),
        (f"E{12345600 + len(v['name'][0]) + 7}", "passport", True), (f"C{23456700 + 11}", "hk_macao_pass", True),
        ("京A12345", "plate", True), ("192.168.31.77", "ip", True), ("https://crm.example.com/u/10086", "url", True),
        ("91110108MA01ABCD5X", "credit_code", True),
        (v["name"][2], "name_plain", True),
        (v["addr"], "address", True),
        ("〔2024〕京仲案字第0123号", "case_no", True), ("（2023）最高法民终123号", "case_no", True),
        ("A123456(7)", "hk_id", True), ("A123456789", "tw_id", True),
        ("張明輝", "name_trad", True), ("李國強", "name_trad", True),
    ]
    for sec, kind, det in truths:
        truth(fn, sec, kind, det)
    open(os.path.join(outdir, fn), "wb").write(("\n".join(lines) + "\n").encode("utf-8"))


# --------------------------------------------------- 第二轮：另一种生成器/异常文件 --


def _xlsxwriter_file(path, v):
    """XlsxWriter 生成：富文本共享字符串、批注、数据验证、定义名称、隐藏表/行、图表、超链接等（结构与 openpyxl 不同）。"""
    import xlsxwriter

    fn = os.path.basename(path)
    wb = xlsxwriter.Workbook(path)
    ws = wb.add_worksheet("客户")
    bold = wb.add_format({"bold": True})
    ws.write("A1", "姓名", bold)
    ws.write("B1", "联系方式", bold)
    ws.write("A2", f"联系人：{truth(fn, v['name'][0], 'name')}")
    ws.write_number("B2", int(truth(fn, v["mobile"][0], "mobile")))
    ws.write_rich_string("A3", "联系人：", bold, truth(fn, v["name"][1], "name"), " 备注")   # 富文本多个 run
    ws.write("B3", truth(fn, v["email"][0], "email"))
    ws.write_comment("B2", f"私人号码 {truth(fn, v['mobile'][1], 'mobile')}", {"author": "对方律师"})
    ws.data_validation("C2", {"validate": "list", "source": [truth(fn, v["email"][1], "email"), "other@example.org"]})
    ws.write_url("A5", f"mailto:{v['email'][2]}", string=truth(fn, v["email"][2], "email"))
    ws.set_row(6, None, None, {"hidden": True})
    ws.write("A7", f"隐藏行 联系人：{truth(fn, v['name'][2], 'name')}")
    ws.set_header(f"&C甲方：{truth(fn, v['company'][0], 'company')}")
    hidden = wb.add_worksheet("隐藏表")
    hidden.hide()
    hidden.write("A1", f"联系人：{truth(fn, v['name'][3], 'name')}")
    hidden.write("A2", truth(fn, v["id"][0], "id_card"))
    wb.define_name("联系邮箱", "=客户!$B$3")
    chart = wb.add_chart({"type": "column"})
    chart.add_series({"values": "=客户!$D$2:$D$3", "name": "销量"})
    ws.write_column("D2", [10, 20])
    ws.insert_chart("F2", chart)
    wb.close()


def _pdf_annots(path, v):
    """链接注释(mailto)/书签/批注作者/嵌入附件里带敏感值；其中一页无命中（注释仍须清理）。"""
    import pikepdf
    from pikepdf import Array, Dictionary, Name, String

    fn = os.path.basename(path)
    pdf = pikepdf.new()
    font = pdf.make_indirect(Dictionary(Type=Name.Font, Subtype=Name.Type1, BaseFont=Name.Helvetica, Encoding=Name.WinAnsiEncoding))
    mail = truth(fn, v["email"][0], "email")
    phone = truth(fn, v["mobile"][0], "mobile")
    for k in range(3):
        # TJ 数组 + 字距调整：敏感值被拆成多段字符串
        parts = ["Mail ", mail[:4], mail[4:9], mail[9:], " now"]
        tj = " -15 ".join(f"({x})" for x in parts)
        ops = ["BT /F1 12 Tf", f"1 0 0 1 30 250 Tm [{tj}] TJ" if k == 0 else f"1 0 0 1 30 250 Tm (Plain page {k} without secrets) Tj", "ET"]
        page = pdf.add_blank_page(page_size=(400, 300))
        page.Resources = Dictionary(Font=Dictionary(F1=font))
        page.Contents = pikepdf.Stream(pdf, "\n".join(ops).encode("latin-1"))
        annot = pdf.make_indirect(Dictionary(
            Type=Name.Annot, Subtype=Name.Link, Rect=Array([30, 240, 200, 262]),
            A=Dictionary(S=Name.URI, URI=String(f"mailto:{mail}")),
            Contents=String(f"call {phone}"), T=String("对方律师")))
        page.Annots = Array([annot])
    truth(fn, "对方律师", "author", False)
    with pdf.open_outline() as outline:
        outline.root.append(pikepdf.OutlineItem(f"联系人 {mail}", 0))
    pdf.docinfo["/Title"] = f"合同 {phone}"
    pdf.docinfo["/Author"] = truth(fn, v["name"][0], "author", False)
    pdf.attachments["secret.txt"] = pikepdf.AttachedFileSpec(pdf, f"附件里的 {mail}".encode("utf-8"))
    pdf.save(path)


def _robustness(outdir):
    """异常文件：不得崩溃/卡死，应给出明确错误或警告，且不产出半成品。"""
    from docx import Document

    d = Document(); d.add_paragraph("联系人：张明辉 13800138000")
    good = os.path.join(outdir, "_tmp_good.docx"); d.save(good)
    raw = open(good, "rb").read()
    open(os.path.join(outdir, "truncated.docx"), "wb").write(raw[: len(raw) // 2])   # 截断
    open(os.path.join(outdir, "garbage.xlsx"), "wb").write(os.urandom(2048))          # 非 zip（如加密/损坏）
    open(os.path.join(outdir, "empty.txt"), "wb").write(b"")
    Document().save(os.path.join(outdir, "empty.docx"))                                  # 合法但无文字
    os.remove(good)
    # 仅 \r 换行（旧 Mac 风格）
    fn = "cr_only.txt"
    body = "联系人：赵文博\r电话：13700137000\r"
    open(os.path.join(outdir, fn), "wb").write(body.encode("utf-8"))
    truth(fn, "13700137000", "mobile")
    truth(fn, "赵文博", "name")
    # 无文本层的 PDF（扫描件形态）
    import pikepdf
    pdf = pikepdf.new(); pdf.add_blank_page(page_size=(300, 200)); pdf.save(os.path.join(outdir, "scan_like.pdf"))


# ------------------------------------------------------------------ TEXT --


def _texts(outdir, v1, v2, v3):
    fn = "notes_crlf.txt"
    body = (f"会议纪要\r\n联系人：{truth(fn, v1['name'][0], 'name')}\r\n电话：{truth(fn, v1['mobile'][0], 'mobile')}\r\n"
            f"邮箱：{truth(fn, v1['email'][0], 'email')}\r\n身份证号：{truth(fn, v1['id'][0], 'id_card')}\r\n")
    open(os.path.join(outdir, fn), "wb").write(body.encode("utf-8"))
    fn = "notes_gbk.txt"
    body = f"联系人：{truth(fn, v2['name'][0], 'name')}\n电话：{truth(fn, v2['mobile'][0], 'mobile')}\n地址：{truth(fn, v2['addr'], 'address')}\n"
    open(os.path.join(outdir, fn), "wb").write(body.encode("gbk"))
    fn = "readme_bom.md"
    body = f"# 项目\n\n甲方：{truth(fn, v3['company'][0], 'company')}\n\n联系人：{truth(fn, v3['name'][0], 'name')}，{truth(fn, v3['mobile'][0], 'mobile')}\n"
    open(os.path.join(outdir, fn), "wb").write(b"\xef\xbb\xbf" + body.encode("utf-8"))


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "corpus"
    os.makedirs(out, exist_ok=True)
    pool = Pool()
    _docx(os.path.join(out, "contract_complex.docx"), pool.take())
    _docx(os.path.join(out, "contract_complex2.docx"), pool.take())
    _xlsx(os.path.join(out, "roster_complex.xlsx"), pool.take())
    _docx_hard(os.path.join(out, "agreement_hard.docx"), pool.take())
    _big_docx(os.path.join(out, "big_contract.docx"), pool.take())
    _big_xlsx(os.path.join(out, "big_roster.xlsx"), pool.take())
    cjk_ok = _pdf_cjk(os.path.join(out, "contract_cjk.pdf"), pool.take())
    _pdf_latin(os.path.join(out, "mail_latin.pdf"), pool.take())
    _pdf_latin(os.path.join(out, "mail_rotated.pdf"), pool.take(), rotate=90, npages=2)
    _pdf_latin(os.path.join(out, "mail_long.pdf"), pool.take(), npages=25)
    _texts(out, pool.take(), pool.take(), pool.take())
    _formats_txt(out, pool.take())
    _xlsxwriter_file(os.path.join(out, "customers_xlsxwriter.xlsx"), pool.take())
    _pdf_annots(os.path.join(out, "annots.pdf"), pool.take())
    _robustness(out)
    # CJK 字体缺失（非 macOS）时去掉对应 truth
    if not cjk_ok:
        TRUTH[:] = [t for t in TRUTH if t["file"] != "contract_cjk.pdf"]
    with open(os.path.join(out, "truth.json"), "w", encoding="utf-8") as f:
        json.dump(TRUTH, f, ensure_ascii=False, indent=1)
    print(f"生成 {len(set(t['file'] for t in TRUTH))} 个文件，{len(TRUTH)} 条标准答案 -> {out}")


if __name__ == "__main__":
    main()
