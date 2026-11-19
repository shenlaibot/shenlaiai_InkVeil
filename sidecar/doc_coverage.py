"""文档覆盖范围检查：找出「本工具不会解析/脱敏」但文件里实际存在的部位。

这些部位里的原文会原样留在输出文件中，因此必须如实告知用户，
并且不得把结果标记为“深度脱敏/无残留”。只做检测，不修改文件。
"""

from __future__ import annotations

import re
import zipfile

# DOCX 现由 XML 层引擎（ooxml_docx）处理：正文/页眉页脚/脚注尾注/批注/修订/超链接/
# 内容控件/域代码/文本框/嵌套表格/注音/隐藏文字均已纳入解析与替换，并清理作者与元数据。
# 以下仍不在处理范围内，必须如实告知。
_DOCX_PARTS = [
    (r"^word/embeddings/", "嵌入对象（OLE/嵌入文件）"),
    (r"^word/media/", "图片（图片内文字不在检测范围）"),
]

_DOCX_MARKERS: list = []

# XLSX 现由 XML 层引擎（ooxml_xlsx）处理：命中单元格（含公式文字）、批注/线程批注、
# 绘图与图表文字、页眉页脚、超链接与数据验证属性、作者与元数据。以下仍不在处理范围内。
_XLSX_PARTS = [
    (r"^xl/pivotCache/|^xl/pivotTables/", "数据透视表及其缓存"),
    (r"^xl/externalLinks/", "外部链接缓存"),
    (r"^xl/embeddings/", "嵌入对象"),
    (r"^xl/media/", "图片（图片内文字不在检测范围）"),
]

_XLSX_SHEET_MARKERS: list = []


def _scan(zf: zipfile.ZipFile, part_rules, marker_rules, marker_part_re) -> list[str]:
    found: list[str] = []
    names = zf.namelist()
    for pattern, label in part_rules:
        rx = re.compile(pattern)
        if any(rx.search(n) for n in names):
            found.append(label)
    mrx = re.compile(marker_part_re)
    blobs = []
    for n in names:
        if mrx.search(n):
            try:
                blobs.append(zf.read(n))
            except Exception:
                continue
    for marker, label in marker_rules:
        mk = re.compile(marker)
        if any(mk.search(b) for b in blobs):
            found.append(label)
    return found


def _custom_xml_has_text(zf) -> bool:
    """customXml 常是 Word 自带的空参考文献模板；只有真的含文字内容时才算“未处理的部位”。"""
    for n in zf.namelist():
        if re.match(r"^customXml/item\d+\.xml$", n):
            try:
                txt = re.sub(rb"<[^>]+>", b"", zf.read(n)).strip()
            except Exception:
                continue
            if txt:
                return True
    return False


def uncovered_parts(path: str, fmt: str) -> list[str]:
    """返回文件中存在、但脱敏引擎未处理的部位说明（去重，保持顺序）。"""
    try:
        with zipfile.ZipFile(path) as zf:
            if fmt == "docx":
                found = _scan(zf, _DOCX_PARTS, _DOCX_MARKERS, r"^word/document\.xml$")
                if _custom_xml_has_text(zf):
                    found.append("自定义 XML 数据（customXml）")
            elif fmt == "xlsx":
                found = _scan(zf, _XLSX_PARTS, _XLSX_SHEET_MARKERS, r"^xl/worksheets/sheet\d+\.xml$")
                if _custom_xml_has_text(zf):
                    found.append("自定义 XML 数据（customXml）")
            else:
                found = []
    except (zipfile.BadZipFile, OSError):
        return []
    seen, out = set(), []
    for f in found:
        if f not in seen:
            seen.add(f)
            out.append(f)
    return out


# ---------------------------------------------------------------------------
# 结构化明细：供界面“未覆盖清单”和合规报告使用（只含部位/类型/数量，绝不含原文）
# ---------------------------------------------------------------------------

_KIND_BY_LABEL = {
    "嵌入对象（OLE/嵌入文件）": "embedded",
    "嵌入对象": "embedded",
    "图片（图片内文字不在检测范围）": "image",
    "数据透视表及其缓存": "pivot",
    "外部链接缓存": "external_link",
    "自定义 XML 数据（customXml）": "custom_xml",
}

_PART_RULES_BY_FMT = {"docx": _DOCX_PARTS, "xlsx": _XLSX_PARTS}


def _pdf_gaps(path: str) -> list[dict]:
    """PDF：整页是图片（无可提取文字）= 扫描页；有文字但含图片 = 图片内文字未检测。"""
    out: list[dict] = []
    try:
        import pypdfium2 as pdfium
        import pypdfium2.raw as pdfium_c

        doc = pdfium.PdfDocument(path)
        try:
            scanned, with_img = [], []
            for pno in range(len(doc)):
                page = doc[pno]
                try:
                    txt = page.get_textpage().get_text_range().strip()
                    has_img = any(True for _ in page.get_objects(filter=[pdfium_c.FPDF_PAGEOBJ_IMAGE]))
                finally:
                    page.close()
                if has_img and not txt:
                    scanned.append(pno + 1)
                elif has_img:
                    with_img.append(pno + 1)
            for pg in scanned:
                out.append({"where": f"第 {pg} 页", "kind": "scanned_page", "count": 1})
            for pg in with_img:
                out.append({"where": f"第 {pg} 页", "kind": "image", "count": 1})
        finally:
            doc.close()
    except Exception:
        return []
    return out


def uncovered_detail(path: str, fmt: str) -> list[dict]:
    """返回 [{"where", "kind", "count"}]：where 为部位（如“第 3 页”“嵌入对象”），
    kind ∈ scanned_page/image/embedded/pivot/external_link/custom_xml，count 为该部位的件数。"""
    if fmt == "pdf":
        return _pdf_gaps(path)
    rules = _PART_RULES_BY_FMT.get(fmt)
    if not rules:
        return []
    out: list[dict] = []
    try:
        with zipfile.ZipFile(path) as zf:
            names = zf.namelist()
            for pattern, label in rules:
                rx = re.compile(pattern)
                n = sum(1 for x in names if rx.search(x))
                if n:
                    out.append({"where": label.split("（")[0], "kind": _KIND_BY_LABEL.get(label, "other"), "count": n})
            if _custom_xml_has_text(zf):
                out.append({"where": "自定义 XML 数据", "kind": "custom_xml", "count": 1})
    except (zipfile.BadZipFile, OSError):
        return []
    return out
