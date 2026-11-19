"""墨隐 InkVeil sidecar — 文档解析模块。

将 PDF / DOCX / TXT / MD 解析为：
  - 全文文本（text，基于字符的完整拼接，供 Rust 侧正则/词典/NER 检测）
  - span 流（spans，每个 span 携带基于全文的字符偏移 [span_start, span_end)，
    供 UI 定位 / Rust 侧检测结果落回文档）
  - 页面/块级结构（structure，供 UI 做页面级定位）

span 协议字段：
  doc_id     str    文档唯一标识（调用方传入）
  page       int    PDF 页号（0 起）；非 PDF 为 0
  block      str    章节/块标识（PDF: 页号；DOCX: 正文/表格/页眉/页脚/文本框；
                    TXT/MD: 行号）
  line       int    行号（0 起，段落/行维度）
  span_start int    该 span 文本在全文中的起始字符偏移
  span_end   int    该 span 文本在全文中的结束字符偏移（不含）
  text       str    span 文本（必须满足 full_text[start:end] == text）
  bbox       list|None  PDF 命中区域 [x0,y0,x1,y1]（PDF 坐标，左下原点），其余格式为 None

说明：
  - 全文偏移一律基于「本解析器产出的全文文本」，与文件原始编码无关；
  - PDF 全文 = 各页文本用 "\n" 拼接；页内行分隔沿用 PDFium 的 "\r\n"；
  - DOCX 全文 = 正文段落 → 表格单元格 → 页眉 → 页脚 → 文本框 各段以 "\n" 拼接。
"""

from __future__ import annotations

import os
import threading
from dataclasses import dataclass, field
from typing import Optional

# 注意：pypdfium2 为延迟导入（见 _parse_pdf_impl），使 sidecar 空闲内存保持低位。

# PDFium 全局锁：pypdfium2/PDFium 本身非线程安全（并发解析/定位同一进程会
# 触发 segfault，实测 4 线程并发即崩溃）。墨隐 InkVeil 批量处理会以线程池并发
# 调用 sidecar，因此所有 PDFium 访问必须经此锁串行化。
PDFIUM_LOCK = threading.Lock()

# ---------------------------------------------------------------------------
# 数据结构
# ---------------------------------------------------------------------------


@dataclass
class Span:
    doc_id: str
    page: int
    block: str
    line: int
    span_start: int
    span_end: int
    text: str
    bbox: Optional[list] = None

    def to_dict(self) -> dict:
        return {
            "doc_id": self.doc_id,
            "page": self.page,
            "block": self.block,
            "line": self.line,
            "span_start": self.span_start,
            "span_end": self.span_end,
            "text": self.text,
            "bbox": self.bbox,
        }


@dataclass
class ParseResult:
    doc_id: str
    fmt: str
    text: str
    spans: list = field(default_factory=list)
    structure: dict = field(default_factory=dict)
    # 内部使用：PDF 每页文本及其在全文中的起始偏移（供 /redact 复用）
    page_offsets: Optional[list] = None
    page_texts: Optional[list] = None
    # DOCX 内部使用：段落收集结果（供 /redact 复用）
    docx_items: Optional[list] = None

    def to_dict(self) -> dict:
        return {
            "ok": True,
            "doc_id": self.doc_id,
            "format": self.fmt,
            "text": self.text,
            "spans": [s.to_dict() for s in self.spans],
            "structure": self.structure,
        }


# ---------------------------------------------------------------------------
# 格式识别
# ---------------------------------------------------------------------------

SUPPORTED_EXTS = (".pdf", ".docx", ".xlsx", ".txt", ".md")


def detect_format(path: str) -> str:
    ext = os.path.splitext(path)[1].lower()
    if ext == ".pdf":
        return "pdf"
    if ext == ".docx":
        return "docx"
    if ext == ".xlsx":
        return "xlsx"
    if ext in (".txt", ".md", ".markdown"):
        return "md" if ext in (".md", ".markdown") else "txt"
    raise ValueError(f"不支持的文档格式: {ext or '(无扩展名)'}（支持: PDF/DOCX/XLSX/TXT/MD）")


def parse_file(path: str, doc_id: Optional[str] = None) -> ParseResult:
    """按扩展名分派解析。doc_id 缺省时使用文件主名。"""
    if not os.path.isfile(path):
        raise FileNotFoundError(f"文件不存在: {path}")
    fmt = detect_format(path)
    doc_id = doc_id or os.path.splitext(os.path.basename(path))[0]
    if fmt == "pdf":
        return parse_pdf(path, doc_id)
    if fmt == "docx":
        return parse_docx(path, doc_id)
    if fmt == "xlsx":
        return parse_xlsx(path, doc_id)
    return parse_text(path, doc_id, fmt)


# ---------------------------------------------------------------------------
# PDF 解析
# ---------------------------------------------------------------------------


def parse_pdf(path: str, doc_id: str) -> ParseResult:
    """PDF 解析（经 PDFIUM_LOCK 串行化，见模块注释）。"""
    with PDFIUM_LOCK:
        return _parse_pdf_impl(path, doc_id)


def _parse_pdf_impl(path: str, doc_id: str) -> ParseResult:
    import pypdfium2 as pdfium  # lazy：PDF 解析时才加载（降低空闲内存）

    pdf = pdfium.PdfDocument(path)
    spans: list[Span] = []
    page_offsets: list[int] = []
    page_texts: list[str] = []
    text_parts: list[str] = []
    offset = 0  # 全文累计偏移（页间以 1 个 "\n" 分隔）
    page_info: list[dict] = []

    try:
        for pno in range(len(pdf)):
            page = pdf[pno]
            tp = None
            try:
                tp = page.get_textpage()
                full = tp.get_text_bounded()

                page_offsets.append(offset)
                page_texts.append(full)
                text_parts.append(full)

                # 行分割：以 \r\n / \n / \r 为分隔符；行文本不含分隔符
                start = 0
                i = 0
                n = len(full)
                line_no = 0
                while i < n:
                    ch = full[i]
                    if ch in "\r\n":
                        line_text = full[start:i]
                        if line_text:
                            spans.append(
                                Span(
                                    doc_id=doc_id,
                                    page=pno,
                                    block=f"页{pno + 1}",
                                    line=line_no,
                                    span_start=offset + start,
                                    span_end=offset + i,
                                    text=line_text,
                                    bbox=_merge_char_bboxes(tp, start, i),
                                )
                            )
                            line_no += 1
                        if ch == "\r" and i + 1 < n and full[i + 1] == "\n":
                            i += 2
                        else:
                            i += 1
                        start = i
                    else:
                        i += 1
                if start < n:
                    spans.append(
                        Span(
                            doc_id=doc_id,
                            page=pno,
                            block=f"页{pno + 1}",
                            line=line_no,
                            span_start=offset + start,
                            span_end=offset + n,
                            text=full[start:n],
                            bbox=_merge_char_bboxes(tp, start, n),
                        )
                    )

                page_info.append({"page": pno, "offset_start": offset, "char_count": len(full)})
                offset += len(full) + 1  # 页间 "\n"
            finally:
                if tp is not None:
                    tp.close()
                page.close()
    finally:
        pdf.close()

    # 注释/链接的文字字段（保留在文件里但不在页面文字中）：接在全文末尾，使其进入检测
    from pdf_safe import pdf_aux_segments

    for j, (label, atext) in enumerate(pdf_aux_segments(path)):
        text_parts.append(atext)
        spans.append(
            Span(doc_id=doc_id, page=0, block=label, line=10**6 + j,
                 span_start=offset, span_end=offset + len(atext), text=atext, bbox=None)
        )
        offset += len(atext) + 1

    text = "\n".join(text_parts)
    structure = {
        "type": "pdf",
        "page_count": len(page_info),
        "pages": page_info,
        "line_count": len(spans),
    }
    return ParseResult(
        doc_id=doc_id,
        fmt="pdf",
        text=text,
        spans=spans,
        structure=structure,
        page_offsets=page_offsets,
        page_texts=page_texts,
    )


def _merge_char_bboxes(tp, start: int, end: int) -> Optional[list]:
    """合并 [start, end) 内各字符 bbox（跳过零尺寸占位符，如 \r\n 等），返回 [x0,y0,x1,y1]。"""
    xs: list[float] = []
    ys: list[float] = []
    for i in range(start, end):
        b = tp.get_charbox(i)
        if b[2] > b[0] and b[3] > b[1]:  # 非零尺寸才纳入
            xs.extend((b[0], b[2]))
            ys.extend((b[1], b[3]))
    if not xs or not ys:
        return None
    return [round(min(xs), 2), round(min(ys), 2), round(max(xs), 2), round(max(ys), 2)]


# ---------------------------------------------------------------------------
# DOCX 解析
# ---------------------------------------------------------------------------


def _docx_run_text(run) -> str:
    """取 run 文本（python-docx 1.x 的 run.text 已包含 w:tab -> \\t、w:br -> \\n）。"""
    return run.text


def _collect_docx_paragraphs(doc):
    """按「正文 → 表格 → 页眉 → 页脚 → 文本框」顺序收集 (block_label, paragraph) 列表。"""
    from docx.text.paragraph import Paragraph

    items: list[tuple[str, Paragraph]] = []
    # 1. 正文顶层段落
    for p in doc.paragraphs:
        items.append(("正文", p))
    # 2. 表格单元格
    for ti, table in enumerate(doc.tables):
        for ri, row in enumerate(table.rows):
            for ci, cell in enumerate(row.cells):
                for p in cell.paragraphs:
                    items.append((f"表格:表{ti + 1}行{ri + 1}列{ci + 1}", p))
    # 3. 页眉 / 页脚（含首页、偶数页变体）
    for si, section in enumerate(doc.sections):
        for kind in (
            "header",
            "footer",
            "even_page_header",
            "even_page_footer",
            "first_page_header",
            "first_page_footer",
        ):
            hf = getattr(section, kind, None)
            if hf is None or hf.is_linked_to_previous:
                continue
            label = "页眉" if "header" in kind else "页脚"
            for p in hf.paragraphs:
                items.append((label, p))
    # 4. 文本框（w:txbxContent 内的段落；body 与各 header/footer part 均遍历）
    parts = [doc.part]
    for section in doc.sections:
        for attr in ("header", "footer", "even_page_header", "even_page_footer",
                     "first_page_header", "first_page_footer"):
            hf = getattr(section, attr, None)
            if hf is not None and hf.part is not None:
                parts.append(hf.part)
    seen_p: set[int] = set()  # 防止文本框段落与上面的收集重复（理论上不会，保险）
    for part in parts:
        element = part.element
        for txbx in element.iter(qn("w:txbxContent")):
            for p_el in txbx.iter(qn("w:p")):
                if id(p_el) in seen_p:
                    continue
                seen_p.add(id(p_el))
                items.append(("文本框", Paragraph(p_el, doc)))
    return items


def qn(tag: str):
    """w: 命名空间限定名（lazy import，避免与 docx 主流程耦合）。"""
    from docx.oxml.ns import qn as _qn

    return _qn(tag)


def parse_docx(path: str, doc_id: str) -> ParseResult:
    """XML 层解析（见 ooxml_docx）：覆盖超链接/修订/批注/脚注尾注/页眉页脚/文本框/嵌套表格等。"""
    from ooxml_docx import build_spans, load_docx

    model = load_docx(path)
    spans = [
        Span(
            doc_id=doc_id,
            page=0,
            block=d["block"],
            line=d["line"],
            span_start=d["span_start"],
            span_end=d["span_end"],
            text=d["text"],
            bbox=None,
        )
        for d in build_spans(model, doc_id)
    ]
    block_counts: dict[str, int] = {}
    for para in model.paras:
        block_counts[para.label] = block_counts.get(para.label, 0) + 1
    structure = {
        "type": "docx",
        "paragraph_count": len(model.paras),
        "table_count": model.table_count,
        "blocks": [{"type": k, "count": v} for k, v in sorted(block_counts.items())],
        "span_count": len(spans),
    }
    return ParseResult(
        doc_id=doc_id,
        fmt="docx",
        text=model.text,
        spans=spans,
        structure=structure,
    )


# ---------------------------------------------------------------------------
# XLSX 解析
# ---------------------------------------------------------------------------

# 全程惰性加载策略与 PDF/DOCX 一致：openpyxl 仅在首次解析/写回 xlsx 时 import，
# 保持 sidecar 空闲内存低位。


def _xlsx_cell_text(cell) -> str:
    """单元格的『全文可见文本』（parse 与 redact 必须使用同一实现以保证偏移一致）。

    - 公式单元格：取公式字符串本身（脱敏时跳过，见 redact_xlsx）；
    - 数值：整数型 float（如 1.0）输出 "1"，避免 "1.0" 破坏规则匹配；
    - 日期/其它：str() 兜底。
    """
    v = cell.value
    if v is None:
        return ""
    if cell.data_type == "f":
        return v if isinstance(v, str) else str(v)
    if isinstance(v, bool):
        return "TRUE" if v else "FALSE"
    if isinstance(v, float):
        return str(int(v)) if v.is_integer() else str(v)
    if isinstance(v, int):
        return str(v)
    return str(v)


def _collect_xlsx_items(wb) -> list:
    """按「sheet 顺序 → 行序 → 列序」收集非空单元格。

    返回 [(sheet_name, row(1 基), col(1 基), cell, text)]，文本与全文偏移一一对应
    （两个相邻非空 cell 的全文之间以 1 个 "\n" 分隔，见 parse_xlsx）。
    """
    items: list = []
    for sheet_name in wb.sheetnames:
        ws = wb[sheet_name]
        for row in ws.iter_rows():
            for cell in row:
                text = _xlsx_cell_text(cell)
                if not text:
                    continue
                items.append((sheet_name, cell.row, cell.column, cell, text))
    return items


def parse_xlsx(path: str, doc_id: str) -> ParseResult:
    from openpyxl import load_workbook

    wb = load_workbook(path, read_only=False, data_only=False)
    items = _collect_xlsx_items(wb)

    spans: list[Span] = []
    text_parts: list[str] = []
    offset = 0
    formula_cells: list[str] = []
    # 网格单元（sheet, row, col) -> (text, start, end)，供前端表格化预览高亮
    grid: dict = {}
    for sheet_name, row_idx, col_idx, cell, text in items:
        if cell.data_type == "f":
            formula_cells.append(f"{sheet_name}!{cell.coordinate}")
        text_parts.append(text)
        spans.append(
            Span(
                doc_id=doc_id,
                page=0,
                block=sheet_name,
                line=row_idx - 1,
                span_start=offset,
                span_end=offset + len(text),
                text=text,
                bbox=None,
            )
        )
        grid.setdefault(sheet_name, {})[(row_idx, col_idx)] = (text, offset, offset + len(text))
        offset += len(text) + 1  # cell 间 "\n"

    # 批注/页眉页脚/链接目标/绘图文字：不在网格里，但必须进入检测（否则这些部位的原文永远不会被发现）
    from ooxml_xlsx import xlsx_aux_segments

    for j, (label, atext) in enumerate(xlsx_aux_segments(path)):
        text_parts.append(atext)
        spans.append(
            Span(doc_id=doc_id, page=0, block=label, line=10**6 + j,
                 span_start=offset, span_end=offset + len(atext), text=atext, bbox=None)
        )
        offset += len(atext) + 1

    text = "\n".join(text_parts)

    # 表格网格（过大时省略，前端回退为全文预览）
    sheets_struct: list = []
    for n in wb.sheetnames:
        ws = wb[n]
        sheet_info = {"name": n, "max_row": ws.max_row, "max_col": ws.max_column}
        if ws.max_row * ws.max_column <= 20000:
            cells = grid.get(n, {})
            rows: list = []
            for r in range(1, ws.max_row + 1):
                row_cells: list = []
                for c in range(1, ws.max_column + 1):
                    cell = cells.get((r, c))
                    row_cells.append(list(cell) if cell else None)
                rows.append(row_cells)
            sheet_info["rows"] = rows
        sheets_struct.append(sheet_info)

    structure = {
        "type": "xlsx",
        "sheet_count": len(wb.sheetnames),
        "sheets": sheets_struct,
        "cell_count": len(spans),
        "span_count": len(spans),
        "formula_cells": formula_cells,
    }
    return ParseResult(doc_id=doc_id, fmt="xlsx", text=text, spans=spans, structure=structure)


# ---------------------------------------------------------------------------
# TXT / MD 解析
# ---------------------------------------------------------------------------


def read_text_file(path: str) -> tuple[str, str]:
    """读取文本文件，返回 (文本, 编码名)。

    以字节解码，**不做换行转换**（\r\n 原样保留，保证解析偏移与写回一致）。
    顺序：UTF-8 BOM → UTF-16 BOM → UTF-8 → GBK → latin-1（永不失败）。
    编码名可直接用于写回：'utf-8-sig'（带 BOM）、'utf-16'（写回时带 BOM）等。
    """
    with open(path, "rb") as f:
        raw = f.read()
    if raw.startswith(b"\xef\xbb\xbf"):
        return raw.decode("utf-8-sig"), "utf-8-sig"
    if raw.startswith((b"\xff\xfe", b"\xfe\xff")):
        return raw.decode("utf-16"), "utf-16"
    for enc in ("utf-8", "gbk", "latin-1"):
        try:
            return raw.decode(enc), enc
        except UnicodeDecodeError:
            continue
    return raw.decode("latin-1"), "latin-1"  # pragma: no cover


def parse_text(path: str, doc_id: str, fmt: str = "txt") -> ParseResult:
    text, enc = read_text_file(path)
    spans: list[Span] = []
    line_no = 0
    start = 0
    i = 0
    n = len(text)
    while i < n:
        ch = text[i]
        if ch in "\r\n":
            line_text = text[start:i]
            spans.append(
                Span(
                    doc_id=doc_id,
                    page=0,
                    block=f"行{line_no + 1}",
                    line=line_no,
                    span_start=start,
                    span_end=start + len(line_text),
                    text=line_text,
                    bbox=None,
                )
            )
            line_no += 1
            if ch == "\r" and i + 1 < n and text[i + 1] == "\n":
                i += 2
            else:
                i += 1
            start = i
        else:
            i += 1
    if start < n:
        spans.append(
            Span(
                doc_id=doc_id,
                page=0,
                block=f"行{line_no + 1}",
                line=line_no,
                span_start=start,
                span_end=n,
                text=text[start:n],
                bbox=None,
            )
        )
    structure = {
        "type": fmt,
        "encoding": enc,
        "line_count": len(spans),
        "char_count": len(text),
    }
    return ParseResult(doc_id=doc_id, fmt=fmt, text=text, spans=spans, structure=structure)
