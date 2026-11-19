"""墨隐 InkVeil sidecar — 脱敏写回模块。

根据「全文字符偏移」的 replacements 对 PDF / DOCX / TXT / MD 执行脱敏写回，
输出到独立目录，原文件绝不修改。

PDF：复用 spike（pypdfium2 定位 + pikepdf 内容流级删除 + 白底覆盖 + 掩码重绘）。
  - 能在内容流文本块内精确删除敏感子串的命中标记 is_deep=True；
  - 跨块等无法精确删除的命中退化为「白底覆盖 + 掩码重绘」（is_deep=False）；
  - 同时清除文档元数据（trailer Info / XMP）。
DOCX：按偏移定位到段落中的 run，逐 run 重建文本（保留格式，必要时跨 run 拆分），
  并清除 core properties。
TXT/MD：全文按偏移替换后以 UTF-8 写新文件。
"""

from __future__ import annotations

import hashlib
import os
import uuid
from collections import defaultdict
from typing import Optional

# 注意：pikepdf / pypdfium2 为延迟导入（见 redact_pdf），降低空闲内存。

from doc_coverage import uncovered_parts
from parsers import (
    PDFIUM_LOCK,
    _collect_docx_paragraphs,
    _collect_xlsx_items,
    _docx_run_text,
    detect_format,
    read_text_file,
)

# ===========================================================================
# PDF 内容流工具（源自 spike/pdf-redaction/test_redact.py，已参数化）
# ===========================================================================


def read_pdf_string(stream, i):
    """解析从 i 开始的 '(' 字符串，返回 (原始字节, 结束下标)。支持 \\ddd 八进制与 \\n 等转义。"""
    n = len(stream)
    buf = bytearray()
    j = i + 1
    while j < n:
        ch = stream[j]
        if ch == 0x5C:  # backslash
            if j + 1 >= n:
                break
            nxt = stream[j + 1]
            mapping = {ord("n"): 10, ord("r"): 13, ord("t"): 9, ord("b"): 8, ord("f"): 12,
                       ord("("): 40, ord(")"): 41, ord("\\"): 92}
            if nxt in mapping:
                buf.append(mapping[nxt])
                j += 2
            elif 48 <= nxt <= 55:  # octal 1-3 位
                val, k = 0, 0
                while j + 1 < n and k < 3 and 48 <= stream[j + 1] <= 55:
                    val = val * 8 + (stream[j + 1] - 48)
                    j += 1
                    k += 1
                buf.append(val & 0xFF)
                j += 1
            else:  # 未知转义：丢弃反斜杠
                j += 2
        elif ch == 0x29:  # ')'
            j += 1
            break
        else:
            buf.append(ch)
            j += 1
    return bytes(buf), j


def read_pdf_array(stream, i):
    """解析从 i 开始的 '[' 数组，返回 (元素列表[(kind,value)], 结束下标)。"""
    n = len(stream)
    items = []
    j = i + 1
    while j < n:
        ch = stream[j]
        if ch in b" \t\r\n\f":
            j += 1
            continue
        if ch == 0x25:  # %
            while j < n and stream[j] not in (0x0A, 0x0D):
                j += 1
            continue
        if ch == 0x5D:  # ]
            return items, j + 1
        if ch == 0x28:  # (
            s, j = read_pdf_string(stream, j)
            items.append(("str", s))
            continue
        if ch == 0x5B:  # [
            sub, j = read_pdf_array(stream, j)
            items.append(("array", sub))
            continue
        if ch in b"+-.0123456789":
            j0 = j
            while j < n and stream[j] in b"+-.0123456789":
                j += 1
            items.append(("num", stream[j0:j]))
            continue
        if ch == 0x3C:  # < 十六进制串（TJ 数组中常见）或 << 字典：原样保留
            if stream[j:j + 2] == b"<<":
                k = _skip_dict(stream, j)
            else:
                k = stream.find(b">", j + 1)
                k = n if k < 0 else k + 1
            items.append(("raw", stream[j:k]))
            j = k
            continue
        # 其它意外字符（名称、布尔/null 字面量等）：原样保留并前进，绝不抛错中断整页
        k = j + 1
        while k < n and stream[k] not in b" \t\r\n\f()<>[]{}/%":
            k += 1
        items.append(("raw", stream[j:k]))
        j = k
        continue
    raise ValueError("内容流数组未闭合")


def _skip_dict(stream, i):
    """从 '<<' 起扫到配对的 '>>'（处理嵌套与字符串），返回结束下标（不含）。未闭合则到末尾。"""
    n = len(stream)
    depth = 0
    j = i
    while j < n:
        if stream[j:j + 2] == b"<<":
            depth += 1
            j += 2
        elif stream[j:j + 2] == b">>":
            depth -= 1
            j += 2
            if depth == 0:
                return j
        elif stream[j] == 0x28:  # ( 字符串里可能含 >>，整体跳过
            try:
                _, j = read_pdf_string(stream, j)
            except ValueError:
                return n
        else:
            j += 1
    return n


def _skip_inline_image(stream, i):
    """从 'BI' 起扫到 'EI'（含），保持二进制数据原样。找不到 EI 则到末尾。"""
    n = len(stream)
    k = stream.find(b"ID", i)
    if k < 0:
        return n
    j = k + 2
    while True:
        e = stream.find(b"EI", j)
        if e < 0:
            return n
        before_ok = e == 0 or stream[e - 1] in b" \t\r\n\f"
        after_ok = e + 2 >= n or stream[e + 2] in b" \t\r\n\f"
        if before_ok and after_ok:
            return e + 2
        j = e + 2


def tokenize(stream):
    """将内容流字节切分为 token 列表 [(kind, value)]，kind ∈ str/array/name/num/op"""
    tokens = []
    n = len(stream)
    i = 0
    while i < n:
        ch = stream[i]
        if ch in b" \t\r\n\f":
            i += 1
            continue
        if ch == 0x25:  # %
            while i < n and stream[i] not in (0x0A, 0x0D):
                i += 1
            continue
        if ch == 0x28:  # (
            s, i = read_pdf_string(stream, i)
            tokens.append(("str", s))
            continue
        if ch == 0x5B:  # [
            arr, i = read_pdf_array(stream, i)
            tokens.append(("array", arr))
            continue
        if ch == 0x3C:  # <
            # '<<...>>' 字典 / '<hex>' 十六进制串：整体作为 raw 原样保留。
            # 注意：这里若不消费字符会令 i 不前进，造成死循环并无限吃内存。
            if stream[i:i + 2] == b"<<":
                j = _skip_dict(stream, i)
            else:
                j = stream.find(b">", i + 1)
                j = n if j < 0 else j + 1
            tokens.append(("raw", stream[i:j]))
            i = j
            continue
        if ch == 0x2F:  # /Name
            j = i + 1
            while j < n and stream[j] not in b" \t\r\n\f()<>[]{}/%":
                j += 1
            tokens.append(("name", stream[i + 1:j].decode("latin-1")))
            i = j
            continue
        if ch in b"+-.0123456789":
            j = i
            while j < n and stream[j] in b"+-.0123456789":
                j += 1
            tokens.append(("num", stream[i:j]))
            i = j
            continue
        if ch >= 0x21 and ch <= 0x7E and not stream[i:i + 1].isspace():
            # 操作符（字母或单个符号）
            j = i
            while j < n and stream[j] >= 0x21 and stream[j] <= 0x7E and stream[j] not in b"()[]/<>{}":
                if stream[j] in b"+-.0123456789":
                    break
                j += 1
            if j == i:
                # 孤立的 ] ) > { } 等定界符：原样保留并强制前进，保证 tokenizer 必定终止
                tokens.append(("raw", stream[i:i + 1]))
                i += 1
                continue
            op = stream[i:j].decode("latin-1")
            if op == "BI":
                # 内联图片：BI ... ID <二进制> EI，整体原样保留，避免二进制被破坏
                j = _skip_inline_image(stream, i)
                tokens.append(("raw", stream[i:j]))
                i = j
                continue
            tokens.append(("op", op))
            i = j
            continue
        i += 1  # 其它字节（>0x7E 的非 ASCII 等）原样跳过
    return tokens


def analyze_text_blocks(tokens):
    """在 token 流上模拟文本状态机（BT/Tf/Tm/Td/TL/Tj/TJ），返回文本块列表。"""
    blocks = []
    state = {"font": None, "size": None, "tx": 0.0, "ty": 0.0, "tl": 0.0}
    i = 0
    n = len(tokens)
    while i < n:
        kind, val = tokens[i]
        if kind == "op":
            if val == "BT":
                state = {"font": None, "size": None, "tx": 0.0, "ty": 0.0, "tl": 0.0}
            elif val == "Tf" and i >= 2:
                nm, sz = tokens[i - 2], tokens[i - 1]
                if nm[0] == "name" and sz[0] == "num":
                    state["font"], state["size"] = nm[1], float(sz[1])
            elif val == "Tm" and i >= 6 and all(tokens[i - k][0] == "num" for k in range(6, 0, -1)):
                nums = [float(tokens[i - k][1]) for k in range(6, 0, -1)]
                state["tx"], state["ty"] = nums[4], nums[5]
            elif val in ("Td", "TD") and i >= 2 and tokens[i - 2][0] == "num" and tokens[i - 1][0] == "num":
                state["tx"] += float(tokens[i - 2][1])
                state["ty"] += float(tokens[i - 1][1])
            elif val == "T*":
                state["ty"] -= state.get("tl", 0.0)
            elif val == "TL" and i >= 1 and tokens[i - 1][0] == "num":
                state["tl"] = float(tokens[i - 1][1])
            elif val == "Tj" and i >= 1 and tokens[i - 1][0] == "str":
                blocks.append({"op": "Tj", "str_tok_idx": i - 1, "op_tok_idx": i,
                               "strs": [tokens[i - 1][1]], **state})
            elif val == "TJ" and i >= 1 and tokens[i - 1][0] == "array":
                strs = [v for k2, v in tokens[i - 1][1] if k2 == "str"]
                blocks.append({"op": "TJ", "str_tok_idx": i - 1, "op_tok_idx": i,
                               "strs": strs, **state})
        i += 1
    return blocks


def escape_pdf_bytes(data):
    """PDF 字符串转义（字节级）：特殊字符用八进制/反斜杠转义，保证往返无损。"""
    out = bytearray()
    for b in data:
        if b in (0x28, 0x29, 0x5C):  # ( ) \
            out.extend(b"\\")
            out.append(b)
        elif b < 0x20:
            out.extend(("\\%03o" % b).encode("ascii"))
        else:
            out.append(b)
    return bytes(out)


def serialize(tokens):
    """token 流 -> 内容流字节（元素间加空格）"""
    def one(tok):
        k, v = tok
        if k == "str":
            return b"(" + escape_pdf_bytes(v) + b")"
        if k == "array":
            return b"[" + b" ".join(one(item) for item in v) + b"]"
        if k == "name":
            return b"/" + v.encode("latin-1")
        if k == "num":
            return v if isinstance(v, bytes) else str(v).encode("ascii")
        if k == "op":
            return v.encode("latin-1")
        if k == "raw":
            return v
        raise ValueError(f"未知 token: {tok}")
    return b" ".join(one(t) for t in tokens)


def redact_page_stream(stream_bytes: bytes, hits: list[dict], fallback_fonts: list[str] | None = None):
    """对单页内容流执行脱敏。

    hits: [{"needle","replacement","bbox"}]（bbox 为 (l,b,r,t) PDF 坐标，非 None）
    返回 (新内容流字节, 每命中处理结果 [{"deep","needle","replacement","bbox"}])。

    深度删除：内容流 UTF-16BE 字符串中删掉敏感子串（删除后不可再提取）；
    无论是否深度删除，都追加白底矩形 + 掩码重绘（双保险 / 兜底）。
    """
    tokens = tokenize(stream_bytes)
    blocks = analyze_text_blocks(tokens)
    deep_ids: set[int] = set()
    mask_meta: dict[int, tuple] = {}

    for blk in blocks:
        text = "".join(s.decode("utf-16-be", "replace") for s in blk["strs"])
        new_text = text
        for hit in hits:
            needle = hit["needle"]
            if needle and needle in new_text:
                new_text = new_text.replace(needle, "")
                deep_ids.add(id(hit))
                mask_meta.setdefault(id(hit), (blk["font"], blk["size"]))
        if new_text == text:
            continue
        enc = new_text.encode("utf-16-be")
        tokens[blk["str_tok_idx"]] = ("str", enc)
        if blk["op"] == "TJ":
            tokens[blk["op_tok_idx"]] = ("op", "Tj")

    # fallback 字体：优先页内文本块字体，其次页面字体资源，最后硬编码
    block_fonts = [(b["font"], b["size"]) for b in blocks if b.get("font")]
    default_font: Optional[tuple] = None
    if block_fonts:
        default_font = block_fonts[0]
    elif fallback_fonts:
        default_font = (fallback_fonts[0], 12.0)
    if default_font is None:
        default_font = ("F2", 12.0)
    for hit in hits:
        mask_meta.setdefault(id(hit), default_font)

    # 追加：白底矩形覆盖 + 掩码重绘（先画矩形，再写掩码，保证掩码在上层可见）
    suffix = []
    for hit in hits:
        l, b, r, t = hit["bbox"]
        pad = 0.5
        x, y, w, h = l - pad, b - pad, (r - l) + 2 * pad, (t - b) + 2 * pad
        suffix.append(f"q 1 1 1 rg {x:.2f} {y:.2f} {w:.2f} {h:.2f} re f Q".encode("ascii"))
    for hit in hits:
        l, b, _, _ = hit["bbox"]
        font, size = mask_meta.get(id(hit), default_font)
        if not font:
            continue
        mask_bytes = hit["replacement"].encode("utf-16-be")
        suffix.append(
            f"BT /{font} {size:.1f} Tf 1 0 0 1 {l:.2f} {b:.2f} Tm ("
            .encode("ascii") + escape_pdf_bytes(mask_bytes) + b") Tj ET"
        )

    new_stream = serialize(tokens)
    if suffix:
        new_stream += b"\n" + b"\n".join(suffix)

    results = [
        {
            "deep": id(h) in deep_ids,
            "needle": h["needle"],
            "replacement": h["replacement"],
            "bbox": list(h["bbox"]),
        }
        for h in hits
    ]
    return new_stream, results


# ===========================================================================
# PDF 脱敏主流程
# ===========================================================================


def _locate_page_hits(tp, page_text: str, page_offset: int, replacements: list[tuple]):
    """把全文偏移的 replacements 映射到单页，返回 [{"needle","replacement","bbox"}]。"""
    hits = []
    for (s, e, repl) in replacements:
        ls = s - page_offset
        le = e - page_offset
        if 0 <= ls and le <= len(page_text) and ls < le:
            needle = page_text[ls:le]
            xs, ys = [], []
            for i in range(ls, le):
                b = tp.get_charbox(i)
                if b[2] > b[0] and b[3] > b[1]:  # 非零尺寸才纳入
                    xs.extend((b[0], b[2]))
                    ys.extend((b[1], b[3]))
            if xs and ys:
                hits.append({
                    "needle": needle,
                    "replacement": repl,
                    "bbox": (min(xs), min(ys), max(xs), max(ys)),
                })
    return hits


def _page_font_names(page) -> list[str]:
    """枚举页面字体资源中的字体名（无斜杠前缀），供 fallback 掩码使用。"""
    names = []
    try:
        fonts = page.Resources.get("/Font", {})
        for key in fonts.keys():
            s = str(key)
            names.append(s[1:] if s.startswith("/") else s)
    except Exception:
        pass
    return names


def _clear_pdf_metadata(pdf):
    """清除 PDF 元数据：删除 trailer Info 并清空 XMP（尽力而为）。"""
    import pikepdf  # lazy

    try:
        if pikepdf.Name.Info in pdf.trailer:
            del pdf.trailer.Info
    except Exception:
        pass
    try:
        with pdf.open_metadata() as meta:
            for k in list(meta.keys()):
                del meta[k]
    except Exception:
        pass


def redact_pdf(input_path: str, replacements: list[tuple], output_path: str) -> dict:
    """PDF 脱敏入口：默认安全模式（命中页栅格化+涂黑+不可见文字层），见 pdf_safe。

    旧的“内容流内删字”实现（redact_pdf_legacy）对子集化/Identity-H 字体基本无效，
    已不再作为默认路径，仅保留以便对照。
    """
    from pdf_safe import redact_pdf_safe

    return redact_pdf_safe(input_path, replacements, output_path, pdfium_lock=PDFIUM_LOCK)


def redact_pdf_legacy(input_path: str, replacements: list[tuple], output_path: str) -> dict:
    """replacements: [(start, end, replacement)]，基于 parse_pdf 的全文偏移。"""
    import pikepdf  # noqa: F401  lazy
    import pypdfium2 as pdfium  # lazy

    # 1. pypdfium2 定位每页命中（重新打开以获取字符级 bbox）
    #    PDFium 非线程安全，与 parsers.parse_pdf 共用 PDFIUM_LOCK 串行化。
    with PDFIUM_LOCK:
        pdf_src = pdfium.PdfDocument(input_path)
        per_page_hits = []
        page_offsets = []
        page_texts = []
        offset = 0
        try:
            for pno in range(len(pdf_src)):
                page = pdf_src[pno]
                tp = None
                try:
                    tp = page.get_textpage()
                    page_text = tp.get_text_bounded()
                    page_offsets.append(offset)
                    page_texts.append(page_text)
                    offset += len(page_text) + 1  # 与 parse_pdf 一致：页间 1 个 "\n"
                    hits = _locate_page_hits(tp, page_text, page_offsets[pno], replacements)
                    per_page_hits.append(hits)
                finally:
                    if tp is not None:
                        tp.close()
                    page.close()
        finally:
            pdf_src.close()

    # 2. pikepdf 内容流编辑 + 元数据清理 + 保存
    total_replaced = 0
    deep_flags: list[bool] = []
    warnings: list[str] = []
    shallow_needles: list[str] = []
    handled = [False] * len(replacements)
    with pikepdf.open(input_path) as pdf:
        for pno, page in enumerate(pdf.pages):
            hits = per_page_hits[pno]
            if not hits:
                continue
            if isinstance(page.Contents, pikepdf.Array):
                stream_bytes = b"\n".join(s.read_bytes() for s in page.Contents)
            else:
                stream_bytes = page.Contents.read_bytes()
            new_stream, results = redact_page_stream(
                stream_bytes, hits, _page_font_names(page)
            )
            page.Contents = pikepdf.Stream(pdf, new_stream)
            total_replaced += len(results)
            deep_flags.extend(r["deep"] for r in results)
            shallow_needles.extend(r["needle"] for r in results if not r["deep"])
        _clear_pdf_metadata(pdf)
        pdf.save(output_path)

    # 3. 深删失败逐条可见：内容流（UTF-16BE）中删不掉的命中，原文字节仍可被
    #    提取器还原，仅做了视觉遮盖——必须逐条明示，不能只给一个聚合布尔。
    for needle in shallow_needles:
        warnings.append(
            f"命中 [{needle}] 的 PDF 字体编码不受支持，内容流中原文仍可提取（已视觉遮盖，请人工确认）"
        )

    # 4. 统计未落页的替换（敏感词跨页/被排版拆开时发生）
    for (s, e, repl) in replacements:
        if not any(po <= s and e <= po + len(pt)
                   for po, pt in zip(page_offsets, page_texts)):
            warnings.append(f"替换 [{s},{e}) 未映射到任何页面，原文仍保留在输出文件中（请人工处理）")

    unplaced = len(shallow_needles)  # 深删失败按未完成处理，用于 is_deep 聚合
    is_deep = bool(deep_flags) and all(deep_flags)
    return {
        "is_deep": is_deep,
        "replaced_count": total_replaced,
        "warnings": warnings,
        "unplaced_count": unplaced,
    }


# ===========================================================================
# DOCX 脱敏
# ===========================================================================


def _build_docx_layout(doc):
    """返回 [(block, paragraph, para_start, para_text)]，para 全文偏移与 parse_docx 一致。"""
    layout = []
    offset = 0
    for block, p in _collect_docx_paragraphs(doc):
        para_text = "".join(_docx_run_text(r) for r in p.runs)
        # 与 parse_docx 保持一致：空段落（无 run 无文本）不占用全文偏移。
        # 若此处跳过逻辑缺失，含空段落的文档会让后续所有段落偏移错位，
        # 导致表格/页眉等位置的敏感词无法映射（漏脱敏）。
        if not para_text and not p.runs:
            continue
        layout.append((block, p, offset, para_text))
        offset += len(para_text) + 1
    return layout


def apply_replacements_to_paragraph(p, repls_para: list[tuple]) -> bool:
    """对段落应用段内替换（[lo, hi, repl]，互不重叠）。逐 run 重建文本，保留原格式。"""
    runs = p.runs
    if not runs:
        return False
    starts = []
    pos = 0
    for r in runs:
        starts.append(pos)
        pos += len(_docx_run_text(r))
    changed = False
    for i, r in enumerate(runs):
        rt = _docx_run_text(r)
        if not rt:
            continue
        rs = starts[i]
        re_ = rs + len(rt)
        parts = []
        cursor = 0
        for (lo, hi, repl) in repls_para:
            if hi <= rs or lo >= re_:
                continue
            del_start = max(lo - rs, 0)
            del_end = min(hi - rs, len(rt))
            if del_end <= del_start:
                continue
            parts.append(rt[cursor:del_start])
            if rs <= lo < re_:  # 替换起点落在本 run：掩码文本并入本 run（格式随本 run）
                parts.append(repl)
            cursor = del_end
        parts.append(rt[cursor:])
        new_text = "".join(parts)
        if new_text != rt:
            r.text = new_text
            changed = True
    return changed


def _clear_docx_core_properties(doc):
    cp = doc.core_properties
    for attr in ("author", "title", "subject", "comments", "keywords",
                 "category", "content_status", "identifier", "language", "last_modified_by"):
        try:
            setattr(cp, attr, "")
        except Exception:
            pass


def redact_docx(input_path: str, replacements: list[tuple], output_path: str) -> dict:
    """XML 层写回（见 ooxml_docx）：只改文本节点，其余部件字节级保留，并清理作者/元数据。"""
    from ooxml_docx import load_docx, write_docx

    model = load_docx(input_path)
    replaced_count, unplaced = write_docx(model, replacements, output_path)
    # 跨界命中无法映射到段落：原文仍保留在输出文件中，必须显性告知（is_deep=False）
    warnings = [
        f"替换 [{s},{e}) 未映射到任何段落，原文仍保留在输出文件中（请人工处理）"
        for (s, e, repl) in unplaced
    ]
    return {
        "is_deep": not unplaced,
        "replaced_count": replaced_count,
        "warnings": warnings,
        "unplaced_count": len(unplaced),
    }


# ===========================================================================
# TXT / MD 脱敏
# ===========================================================================


def redact_text(input_path: str, replacements: list[tuple], output_path: str) -> dict:
    text, enc = read_text_file(input_path)
    new_text = text
    for (s, e, repl) in sorted(replacements, key=lambda x: -x[0]):  # 从后往前，避免偏移漂移
        if 0 <= s < e <= len(text):
            new_text = new_text[:s] + repl + new_text[e:]
    warnings: list[str] = []
    # 按原编码写回（保留 BOM），以二进制写入避免平台换行转换（Windows 下 \r\n 变 \r\r\n）
    try:
        data = new_text.encode(enc)
    except UnicodeEncodeError:
        data = new_text.encode("utf-8")
        warnings.append(f"替换文本无法用原编码 {enc} 表示，输出已改用 UTF-8")
    with open(output_path, "wb") as f:
        f.write(data)
    return {"is_deep": True, "replaced_count": len(replacements), "warnings": warnings}


# ===========================================================================
# XLSX 脱敏
# ===========================================================================


def _clear_xlsx_core_properties(wb):
    """清空工作簿 core properties（尽力而为，与 DOCX 元数据清理一致）。"""
    props = wb.properties
    for attr in ("creator", "lastModifiedBy", "title", "subject", "description",
                 "keywords", "category", "contentStatus", "identifier", "language"):
        try:
            setattr(props, attr, "")
        except Exception:
            pass


def redact_xlsx(input_path: str, replacements: list[tuple], output_path: str) -> dict:
    """XML 层写回（见 ooxml_xlsx）：不再经 openpyxl 保存，图表/图片/批注等不丢失、不遗漏。"""
    from ooxml_xlsx import redact_xlsx_xml

    return redact_xlsx_xml(input_path, replacements, output_path)


# ===========================================================================
# 统一入口
# ===========================================================================


def _unique_output_path(output_dir: str, input_path: str, ext: str, keep_name: bool = False) -> str:
    """生成不覆盖已有文件的输出路径。

    默认不使用原文件名（文件名本身常含当事人姓名等个人信息）：
    用输入绝对路径的短哈希命名 `anonkit-<hash8>_脱敏.ext`，同一输入稳定可对应。
    keep_name=True 时沿用 `<原名>_脱敏.ext`（调用方须明示风险）。
    """
    if keep_name:
        base = os.path.splitext(os.path.basename(input_path))[0] + "_脱敏"
    else:
        h = hashlib.sha256(os.path.abspath(input_path).encode("utf-8")).hexdigest()[:8]
        base = f"anonkit-{h}_脱敏"
    cand = os.path.join(output_dir, base + ext)
    n = 2
    while os.path.exists(cand):
        cand = os.path.join(output_dir, f"{base}-{n}{ext}")
        n += 1
    return cand


def redact_file(input_path: str, replacements: list[dict], output_dir: str, keep_name: bool = False) -> dict:
    """统一脱敏入口。replacements: [{"start","end","replacement"}]，全文偏移，互不重叠。

    返回 {"output_path", "is_deep", "replaced_count", "warnings"}。
    """
    if not os.path.isfile(input_path):
        raise FileNotFoundError(f"文件不存在: {input_path}")
    fmt = detect_format(input_path)
    os.makedirs(output_dir, exist_ok=True)

    repls: list[tuple] = []
    for r in replacements:
        start, end, replacement = int(r["start"]), int(r["end"]), str(r["replacement"])
        if start < 0 or end < start:
            raise ValueError(f"非法替换区间: [{start},{end})")
        repls.append((start, end, replacement))
    repls.sort(key=lambda x: x[0])
    for a, b in zip(repls, repls[1:]):
        if b[0] < a[1]:
            raise ValueError(f"replacements 重叠: [{a[0]},{a[1]}) 与 [{b[0]},{b[1]})")

    _, ext = os.path.splitext(os.path.basename(input_path))
    final_path = _unique_output_path(output_dir, input_path, ext, keep_name=keep_name)
    # 先写同目录临时文件，成功后原子改名：中途崩溃不会留下写了一半的“脱敏”文件
    tmp_path = os.path.join(output_dir, f".anonkit-{uuid.uuid4().hex}.tmp{ext}")
    try:
        if fmt == "pdf":
            result = redact_pdf(input_path, repls, tmp_path)
        elif fmt == "docx":
            result = redact_docx(input_path, repls, tmp_path)
        elif fmt == "xlsx":
            result = redact_xlsx(input_path, repls, tmp_path)
        else:
            result = redact_text(input_path, repls, tmp_path)
        os.replace(tmp_path, final_path)
    finally:
        if os.path.exists(tmp_path):
            try:
                os.remove(tmp_path)
            except OSError:
                pass
    output_path = final_path

    # 如实标注覆盖范围：存在未处理部位时，原文可能仍留在输出里，不得标记为“深度”
    if fmt in ("docx", "xlsx"):
        gaps = uncovered_parts(input_path, fmt)
        if gaps:
            result = dict(result)
            result["is_deep"] = False
            result["warnings"] = list(result.get("warnings", [])) + [
                "以下部位未被处理，其中的原文可能仍保留在输出文件中，外发前请人工核对: "
                + "、".join(gaps)
            ]

    return {
        "output_path": output_path,
        "is_deep": result["is_deep"],
        "replaced_count": result["replaced_count"],
        "warnings": result.get("warnings", []),
        "unplaced_count": result.get("unplaced_count", 0),
    }
