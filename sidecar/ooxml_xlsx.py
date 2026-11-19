"""XLSX 的 XML 层写回引擎。

为什么不再用 openpyxl 整体读写：openpyxl 保存时会丢掉图表、图片、形状、切片器、部分透视表和扩展格式，
并把被改的数值单元格变成字符串，同时批注/页眉页脚/绘图文字等部位原文原样保留。

做法：解析阶段仍用 openpyxl 只读取（保证偏移与 parse_xlsx 完全一致），
写回阶段直接改 XML：
- 命中的单元格改为 inlineStr（保留样式），共享字符串表中因此失去引用的条目清空；
- 公式单元格：改写公式文本并清除缓存值，标记打开时重算；
- 批注 / 线程批注 / 绘图与图表文字 / 页眉页脚 / 超链接与数据验证属性：按“原文→替换文本”同步改写；
- 作者、元数据、缩略图清理；未涉及的部件按字节原样保留（图表、图片等不再丢失）。
"""

from __future__ import annotations

import re
import zipfile
from collections import defaultdict
from xml.sax.saxutils import escape as _xml_escape

from lxml import etree

from ooxml_docx import (
    Para,
    Segment,
    _apply_to_para,
    _scrub_app,
    _scrub_core,
    _scrub_custom,
)

NS_MAIN = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
NS_R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
NS_REL = "http://schemas.openxmlformats.org/package/2006/relationships"
NS_A = "http://schemas.openxmlformats.org/drawingml/2006/main"
XML_SPACE = "{http://www.w3.org/XML/1998/namespace}space"

_C = f"{{{NS_MAIN}}}c"
_V = f"{{{NS_MAIN}}}v"
_F = f"{{{NS_MAIN}}}f"
_IS = f"{{{NS_MAIN}}}is"
_T = f"{{{NS_MAIN}}}t"
_SI = f"{{{NS_MAIN}}}si"
_A_T = f"{{{NS_A}}}t"
_A_P = f"{{{NS_A}}}p"

_HF_TAGS = {
    f"{{{NS_MAIN}}}{n}"
    for n in ("oddHeader", "oddFooter", "evenHeader", "evenFooter", "firstHeader", "firstFooter")
}

_SHEET_PART = re.compile(r"^xl/worksheets/[^/]+\.xml$")
_COMMENT_PART = re.compile(r"^xl/comments(?:/[^/]+|[^/]*)\.xml$")
_THREADED_PART = re.compile(r"^xl/threadedComments/[^/]+\.xml$")
_DRAWING_PART = re.compile(r"^xl/(drawings|charts)/[^/]+\.xml$")
_PERSON_PART = re.compile(r"^xl/persons/[^/]+\.xml$")


def _parser():
    return etree.XMLParser(resolve_entities=False, no_network=True)


def _ser(root) -> bytes:
    return etree.tostring(root, xml_declaration=True, encoding="UTF-8", standalone=True)


def _col_to_letters(col: int) -> str:
    s = ""
    while col:
        col, rem = divmod(col - 1, 26)
        s = chr(65 + rem) + s
    return s


def _sheet_paths(zf: zipfile.ZipFile) -> dict:
    """工作表名 -> 部件路径。"""
    wb = etree.fromstring(zf.read("xl/workbook.xml"), _parser())
    rels = etree.fromstring(zf.read("xl/_rels/workbook.xml.rels"), _parser())
    rid_to_target = {}
    for r in rels:
        t = r.get("Target", "")
        t = t.lstrip("/")
        rid_to_target[r.get("Id")] = t if t.startswith("xl/") else "xl/" + t
    out = {}
    for sh in wb.iter(f"{{{NS_MAIN}}}sheet"):
        rid = sh.get(f"{{{NS_R}}}id")
        if rid in rid_to_target:
            out[sh.get("name")] = rid_to_target[rid]
    return out


def xlsx_aux_segments(path: str) -> list:
    """不在单元格网格里、但可能含敏感信息的部位文本：[(label, text)]（顺序固定，parse 与 redact 共用）。

    批注/线程批注、页眉页脚、外部超链接目标、绘图与图表里的文字。
    """
    out: list = []
    with zipfile.ZipFile(path) as zf:
        names = sorted(zf.namelist())
        for name in names:
            try:
                if _COMMENT_PART.match(name):
                    root = etree.fromstring(zf.read(name), _parser())
                    for c in root.iter(f"{{{NS_MAIN}}}comment"):
                        t = "".join((e.text or "") for e in c.iter(_T))
                        if t:
                            out.append(("批注", t))
                elif _THREADED_PART.match(name):
                    root = etree.fromstring(zf.read(name), _parser())
                    for e in root.iter():
                        if isinstance(e.tag, str) and etree.QName(e).localname == "text" and e.text:
                            out.append(("批注", e.text))
                elif _SHEET_PART.match(name):
                    root = etree.fromstring(zf.read(name), _parser())
                    for e in root.iter():
                        if e.tag in _HF_TAGS and e.text:
                            out.append(("页眉页脚", e.text))
                        elif isinstance(e.tag, str):
                            ln = etree.QName(e).localname
                            if ln in ("formula1", "formula2") and e.text:
                                out.append(("数据验证", e.text))
                            elif ln in ("hyperlink", "dataValidation"):
                                for k in ("display", "tooltip", "location", "prompt", "promptTitle", "error", "errorTitle"):
                                    if e.get(k):
                                        out.append(("超链接/验证提示", e.get(k)))
                elif name.endswith(".rels") and name.startswith("xl/"):
                    rels = etree.fromstring(zf.read(name), _parser())
                    for r in rels:
                        if r.get("TargetMode") == "External" and r.get("Target"):
                            out.append(("链接目标", r.get("Target")))
                elif _DRAWING_PART.match(name):
                    root = etree.fromstring(zf.read(name), _parser())
                    for p in root.iter(_A_P):
                        t = "".join((e.text or "") for e in p.iter(_A_T))
                        if t:
                            out.append(("图表/绘图文字", t))
            except Exception:
                continue
    return out


def _string_literal_spans(formula: str) -> list:
    """公式文本中双引号字符串字面量的 [start, end) 区间（"" 为转义的引号）。"""
    spans, i, n = [], 0, len(formula)
    while i < n:
        if formula[i] == '"':
            j = i + 1
            while j < n:
                if formula[j] == '"':
                    if j + 1 < n and formula[j + 1] == '"':
                        j += 2
                        continue
                    break
                j += 1
            spans.append((i + 1, j))
            i = j + 1
        else:
            i += 1
    return spans


def _replace_pairs_in_containers(containers: list, pairs: dict) -> bool:
    """containers: [[text 节点元素...]]；同一容器内文本拼接后查找，可跨节点（rich text 被拆成多个 run）。"""
    changed = False
    for nodes in containers:
        segs, pos = [], 0
        for el in nodes:
            t = el.text or ""
            segs.append(Segment(el, t, pos))
            pos += len(t)
        text = "".join(s.text for s in segs)
        if not text:
            continue
        found = []
        for orig, repl in pairs.items():
            for m in re.finditer(re.escape(orig), text):
                found.append((m.start(), m.end(), repl))
        if not found:
            continue
        found.sort()
        merged, last = [], -1
        for lo, hi, repl in found:
            if lo >= last:
                merged.append((lo, hi, repl))
                last = hi
        para = Para("", "", None, segs, text, 0)
        if _apply_to_para(para, merged):
            changed = True
    return changed


def _raw_replace(data: bytes, pairs: dict) -> bytes:
    """在原始 XML 字节里直接替换（含属性值、数值缓存等）；用 XML 转义后的形态匹配。"""
    s = data.decode("utf-8", "ignore")
    for orig, repl in pairs.items():
        s = s.replace(_xml_escape(orig, {'"': "&quot;"}), _xml_escape(repl, {'"': "&quot;"}))
        s = s.replace(_xml_escape(orig), _xml_escape(repl))
    return s.encode("utf-8")


def redact_xlsx_xml(input_path: str, replacements: list, output_path: str) -> dict:
    from openpyxl import load_workbook

    from parsers import _collect_xlsx_items

    wb = load_workbook(input_path, read_only=False, data_only=False)
    items = _collect_xlsx_items(wb)
    cells: list = []
    offset = 0
    for sheet_name, row_idx, col_idx, cell, text in items:
        cells.append((offset, offset + len(text), sheet_name, row_idx, col_idx, cell.data_type, text))
        offset += len(text) + 1
    aux_segs = xlsx_aux_segments(input_path)
    aux_start = offset
    aux_ranges = []
    for label, atext in aux_segs:
        aux_ranges.append((aux_start, aux_start + len(atext)))
        aux_start += len(atext) + 1
    full_text = "\n".join([c[6] for c in cells] + [t for _, t in aux_segs])

    per_cell: dict = defaultdict(list)
    unplaced: list = []
    pairs: dict = {}
    replaced_count_aux = 0
    for (s, e, repl) in replacements:
        placed = False
        for (cs, ce, sh, r, c, dt, text) in cells:
            if cs <= s and e <= ce:
                per_cell[(sh, r, c)].append((s - cs, e - cs, repl))
                placed = True
                break
        if not placed and any(a <= s and e <= b for a, b in aux_ranges):
            # 批注/页眉页脚/链接目标/绘图文字里的命中：只登记“原文→替换文本”，由后面的同步改写处理
            pairs[full_text[s:e]] = repl
            replaced_count_aux += 1
            continue
        if placed:
            orig = full_text[s:e]
            if orig:
                pairs[orig] = repl
        else:
            unplaced.append((s, e, repl))

    warnings: list = []
    replaced_count = replaced_count_aux
    changed_parts: dict = {}  # 部件名 -> 新字节
    zin = zipfile.ZipFile(input_path)
    try:
        sheet_paths = _sheet_paths(zin)
        # 按工作表分组待改单元格
        by_sheet: dict = defaultdict(dict)
        for (cs, ce, sh, r, c, dt, text) in cells:
            repls = per_cell.get((sh, r, c))
            if repls:
                repls.sort(key=lambda x: x[0])
                if dt == "f":
                    # 公式：只有落在双引号字符串字面量内的命中才允许改写；
                    # 命中公式的其它部分（函数名/引用）改写会破坏公式，跳过并明示
                    spans = _string_literal_spans(text)
                    ok = [x for x in repls if any(a <= x[0] and x[1] <= b for a, b in spans)]
                    if len(ok) != len(repls):
                        warnings.append(
                            f"公式单元格 {sh}!{_col_to_letters(c)}{r} 的命中不在字符串字面量内，已跳过（请人工确认）"
                        )
                    repls = ok
                    if not repls:
                        continue
                new_text = text
                for lo, hi, repl in sorted(repls, key=lambda x: -x[0]):
                    new_text = new_text[:lo] + repl + new_text[hi:]
                by_sheet[sh][(r, c)] = (dt, new_text, len(repls))

        # 共享字符串引用计数（编辑前）
        ss_name = "xl/sharedStrings.xml"
        has_ss = ss_name in zin.namelist()
        released: set = set()
        sheet_roots: dict = {}
        for sh, edits in by_sheet.items():
            part = sheet_paths.get(sh)
            if not part:
                for _ in edits:
                    warnings.append(f"找不到工作表 {sh} 的部件，其中的命中未写回")
                continue
            root = etree.fromstring(zin.read(part), _parser())
            sheet_roots[part] = root
            coord_map = {c.get("r"): c for c in root.iter(_C)}
            need_recalc = False
            for (r, c), (dt, new_text, n) in edits.items():
                coord = f"{_col_to_letters(c)}{r}"
                el = coord_map.get(coord)
                if el is None:
                    warnings.append(f"{sh}!{coord} 在 XML 中找不到，未写回")
                    continue
                f_el = el.find(_F)
                if f_el is not None:
                    # 公式：改写公式文本（去掉开头的 =），清缓存值，打开时重算
                    f_el.text = new_text[1:] if new_text.startswith("=") else new_text
                    v = el.find(_V)
                    if v is not None:
                        el.remove(v)
                    need_recalc = True
                    warnings.append(f"公式单元格 {sh}!{coord} 内的文字已替换，请核对公式与计算结果")
                else:
                    if el.get("t") == "s":
                        v = el.find(_V)
                        if v is not None and (v.text or "").strip().isdigit():
                            released.add(int(v.text.strip()))
                    for ch in list(el):
                        el.remove(ch)
                    el.set("t", "inlineStr")
                    is_el = etree.SubElement(el, _IS)
                    t_el = etree.SubElement(is_el, _T)
                    t_el.text = new_text
                    t_el.set(XML_SPACE, "preserve")
                replaced_count += n
            if need_recalc:
                changed_parts["__recalc__"] = b"1"

        # 引用计数 & 孤儿共享字符串清空（仍被其他单元格引用的条目保持不变）
        if has_ss and released:
            refs = defaultdict(int)
            for part in [p for p in zin.namelist() if _SHEET_PART.match(p)]:
                root = sheet_roots.get(part)
                data = None if root is not None else zin.read(part)
                it = root.iter(_C) if root is not None else etree.fromstring(data, _parser()).iter(_C)
                for c in it:
                    if c.get("t") == "s":
                        v = c.find(_V)
                        if v is not None and (v.text or "").strip().isdigit():
                            refs[int(v.text.strip())] += 1
            orphans = {i for i in released if refs.get(i, 0) == 0}
            if orphans:
                ss_root = etree.fromstring(zin.read(ss_name), _parser())
                sis = [e for e in ss_root if e.tag == _SI]
                for i in orphans:
                    if i < len(sis):
                        for ch in list(sis[i]):
                            sis[i].remove(ch)
                        etree.SubElement(sis[i], _T)
                changed_parts[ss_name] = _ser(ss_root)

        # 其余部件：按“原文→替换文本”同步改写
        for name in zin.namelist():
            if name in changed_parts:
                continue
            if _COMMENT_PART.match(name):
                root = etree.fromstring(zin.read(name), _parser())
                dirty = False
                for a in root.iter(f"{{{NS_MAIN}}}author"):
                    if a.text and a.text != "Author":
                        a.text = "Author"
                        dirty = True
                containers = [
                    list(c.iter(_T)) for c in root.iter(f"{{{NS_MAIN}}}comment")
                ]
                if pairs and _replace_pairs_in_containers(containers, pairs):
                    dirty = True
                if dirty:
                    changed_parts[name] = _ser(root)
            elif _THREADED_PART.match(name) or _PERSON_PART.match(name):
                data = zin.read(name)
                if _PERSON_PART.match(name):
                    root = etree.fromstring(data, _parser())
                    for el in root.iter():
                        if isinstance(el.tag, str) and "displayName" in el.attrib:
                            el.set("displayName", "Author")
                        if isinstance(el.tag, str) and "userId" in el.attrib:
                            el.set("userId", "")
                    changed_parts[name] = _ser(root)
                elif pairs:
                    new = _raw_replace(data, pairs)
                    if new != data:
                        changed_parts[name] = new
            elif name.endswith(".rels") and name.startswith("xl/"):
                data = zin.read(name)
                if pairs:
                    new = _raw_replace(data, pairs)
                    if new != data:
                        changed_parts[name] = new
            elif _DRAWING_PART.match(name):
                data = zin.read(name)
                if pairs:
                    new = _raw_replace(data, pairs)
                    dirty = new != data
                    root = etree.fromstring(new, _parser())
                    # 文字被拆成多个 a:t 的情况：按段落拼接后再替换一次
                    if _replace_pairs_in_containers([list(p.iter(_A_T)) for p in root.iter(_A_P)], pairs):
                        dirty = True
                    if dirty:
                        changed_parts[name] = _ser(root)
            elif _SHEET_PART.match(name):
                root = sheet_roots.get(name)
                if root is None:
                    root = etree.fromstring(zin.read(name), _parser())
                dirty = name in sheet_roots
                if pairs:
                    for el in root.iter():
                        if not isinstance(el.tag, str):
                            continue
                        if el.tag in _HF_TAGS and el.text:
                            new = el.text
                            for o, r_ in pairs.items():
                                new = new.replace(o, r_)
                            if new != el.text:
                                el.text = new
                                dirty = True
                        ln = etree.QName(el).localname
                        if ln in ("hyperlink", "dataValidation"):
                            for k in ("display", "tooltip", "location", "prompt", "promptTitle", "error", "errorTitle"):
                                if k in el.attrib:
                                    new = el.get(k)
                                    for o, r_ in pairs.items():
                                        new = new.replace(o, r_)
                                    if new != el.get(k):
                                        el.set(k, new)
                                        dirty = True
                        if ln in ("formula1", "formula2") and el.text:
                            new = el.text
                            for o, r_ in pairs.items():
                                new = new.replace(o, r_)
                            if new != el.text:
                                el.text = new
                                dirty = True
                if dirty:
                    changed_parts[name] = _ser(root)

        # 工作表名里含原文：不改名（会破坏公式引用），如实告警
        for o in pairs:
            if any(o in n for n in sheet_paths):
                warnings.append("工作表名称中含已确认的敏感文字，未改名（改名会破坏公式引用），请人工处理")
                break

        if "__recalc__" in changed_parts:
            del changed_parts["__recalc__"]
            wbxml = zin.read("xl/workbook.xml")
            root = etree.fromstring(wbxml, _parser())
            calc = root.find(f"{{{NS_MAIN}}}calcPr")
            if calc is None:
                calc = etree.SubElement(root, f"{{{NS_MAIN}}}calcPr")
            calc.set("fullCalcOnLoad", "1")
            changed_parts["xl/workbook.xml"] = _ser(root)

        with zipfile.ZipFile(output_path, "w", zipfile.ZIP_DEFLATED) as zout:
            for info in zin.infolist():
                name = info.filename
                data = zin.read(name)
                if name in changed_parts:
                    data = changed_parts[name]
                elif name == "docProps/core.xml":
                    data = _scrub_core(data)
                elif name == "docProps/app.xml":
                    data = _scrub_app(data)
                elif name == "docProps/custom.xml":
                    data = _scrub_custom(data)
                elif re.match(r"^docProps/thumbnail\.\w+$", name):
                    continue
                elif name == "_rels/.rels":
                    data = re.sub(rb"<Relationship\b[^>]*thumbnail[^>]*/>", b"", data, flags=re.I)
                zi = zipfile.ZipInfo(name, date_time=info.date_time)
                zi.compress_type = zipfile.ZIP_DEFLATED
                zi.external_attr = info.external_attr
                zout.writestr(zi, data)
    finally:
        zin.close()

    warnings.extend(
        f"替换 [{s},{e}) 未映射到任何单元格，原文仍保留在输出文件中（请人工处理）"
        for (s, e, repl) in unplaced
    )
    return {
        "is_deep": not unplaced,
        "replaced_count": replaced_count,
        "warnings": warnings,
        "unplaced_count": len(unplaced),
    }
