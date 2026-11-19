"""DOCX 的 XML 层引擎：解析与写回共用同一套文本模型。

为什么不用 python-docx 的高层对象：它只暴露 `paragraph.runs`（只含 w:p 的直接子 w:r），
超链接 / 修订(ins、del) / 内容控件(sdt) / 智能标签内的文字、嵌套表格、脚注尾注、批注、
被删除的修订文本 (w:delText)、域代码 (w:instrText) 都会漏掉，原文原样留在输出里。

本模块直接遍历 OOXML：
- 文本部件：document / header* / footer* / footnotes / endnotes / comments；
- 段落内文字 = 沿文档顺序收集的 w:t、w:delText、w:instrText（含 hyperlink/ins/del/sdt/ruby 等任意嵌套，
  跳过 mc:Fallback 重复副本；嵌套段落（文本框）作为独立段落处理）；
- 替换只改文本节点，其余结构、格式、未涉及的部件按字节原样保留；
- 同时清理作者/元数据（w:author、core/app/custom 属性、缩略图）。
"""

from __future__ import annotations

import re
import zipfile
from dataclasses import dataclass, field
from typing import Optional

from lxml import etree

W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
MC = "http://schemas.openxmlformats.org/markup-compatibility/2006"
XML_NS = "http://www.w3.org/XML/1998/namespace"

_T = f"{{{W}}}t"
_DEL_T = f"{{{W}}}delText"
_INSTR = f"{{{W}}}instrText"
_P = f"{{{W}}}p"
_TC = f"{{{W}}}tc"
_TBL = f"{{{W}}}tbl"
_TXBX = f"{{{W}}}txbxContent"
_TAB = f"{{{W}}}tab"
_BR = f"{{{W}}}br"
_CR = f"{{{W}}}cr"
_FALLBACK = f"{{{MC}}}Fallback"
_AUTHOR = f"{{{W}}}author"
_INITIALS = f"{{{W}}}initials"

_TEXT_PARTS = re.compile(
    r"^word/(document|header\d*|footer\d*|footnotes|endnotes|comments)\.xml$"
)
_EDITABLE = (_T, _DEL_T, _INSTR)


def _part_sort_key(name: str):
    base = name.split("/")[-1]
    order = (
        ["document", "header", "footer", "footnotes", "endnotes", "comments"]
    )
    for i, k in enumerate(order):
        if base.startswith(k):
            m = re.search(r"(\d+)", base)
            return (i, int(m.group(1)) if m else 0)
    return (99, 0)


def _label_for(part: str, p) -> str:
    base = part.split("/")[-1]
    if base.startswith("header"):
        return "页眉"
    if base.startswith("footer"):
        return "页脚"
    if base.startswith("footnotes"):
        return "脚注"
    if base.startswith("endnotes"):
        return "尾注"
    if base.startswith("comments"):
        return "批注"
    # document.xml：文本框 > 表格 > 正文
    a = p.getparent()
    in_tbl = False
    while a is not None:
        if a.tag == _TXBX:
            return "文本框"
        if a.tag == _TC:
            in_tbl = True
        a = a.getparent()
    return "表格" if in_tbl else "正文"


@dataclass
class Segment:
    elem: Optional[etree._Element]  # None = 不可编辑的伪字符（tab / 换行）
    text: str
    start: int = 0  # 段内字符偏移


@dataclass
class Para:
    part: str
    label: str
    elem: etree._Element
    segments: list = field(default_factory=list)
    text: str = ""
    start: int = 0  # 全文偏移


def _in_fallback(el) -> bool:
    a = el.getparent()
    while a is not None:
        if a.tag == _FALLBACK:
            return True
        a = a.getparent()
    return False


def _collect_segments(p) -> list:
    segs: list = []

    def walk(el):
        for ch in el:
            tag = ch.tag
            if not isinstance(tag, str):  # 注释 / 处理指令
                continue
            if tag == _P or tag == _FALLBACK:
                continue  # 嵌套段落（文本框）独立处理；Fallback 是重复副本
            if tag in _EDITABLE:
                segs.append(Segment(ch, ch.text or ""))
            elif tag == _TAB:
                segs.append(Segment(None, "\t"))
            elif tag == _BR or tag == _CR:
                segs.append(Segment(None, "\n"))
            else:
                walk(ch)

    walk(p)
    pos = 0
    for s in segs:
        s.start = pos
        pos += len(s.text)
    return segs


@dataclass
class DocxModel:
    path: str
    roots: dict  # part name -> lxml root
    paras: list
    text: str
    table_count: int
    fallback_paras: list = field(default_factory=list)  # mc:Fallback 内的重复副本段落
    aux: list = field(default_factory=list)  # [(label, text, start)]：不在正文模型里但可能含敏感信息的部位


_REL_NS = "http://schemas.openxmlformats.org/package/2006/relationships"
_ALT_ATTRS = ("descr", "title")
_ALT_ELEMS = ("docPr", "cNvPr")


def _aux_collect(zf, roots) -> list:
    """辅助部位文本：外部链接目标（mailto:/tel:/URL）、图片替代文字。返回 [(label, text)]。"""
    out = []
    for name in zf.namelist():
        if name.endswith(".rels") and name.startswith("word/"):
            try:
                rels = etree.fromstring(zf.read(name))
            except Exception:
                continue
            for r in rels:
                if r.get("TargetMode") == "External" and r.get("Target"):
                    out.append(("链接目标", r.get("Target")))
    for part, root in roots.items():
        for el in root.iter():
            if isinstance(el.tag, str) and etree.QName(el).localname in _ALT_ELEMS:
                for a in _ALT_ATTRS:
                    v = el.get(a)
                    if v:
                        out.append(("图片替代文字", v))
    return out


def load_docx(path: str) -> DocxModel:
    roots: dict = {}
    with zipfile.ZipFile(path) as zf:
        names = sorted(
            (n for n in zf.namelist() if _TEXT_PARTS.match(n)), key=_part_sort_key
        )
        for n in names:
            parser = etree.XMLParser(resolve_entities=False, no_network=True, huge_tree=False)
            roots[n] = etree.fromstring(zf.read(n), parser)
    paras: list = []
    offset = 0
    parts_text: list = []
    table_count = 0
    fallback_paras: list = []
    for n in names:
        root = roots[n]
        table_count += sum(1 for _ in root.iter(_TBL))
        for p in root.iter(_P):
            if _in_fallback(p):
                # 文本框的 VML 回退副本：不进入全文（避免重复），但替换时必须同步改写，
                # 否则原文会留在副本里（Word 打开时会显示/或被旧版 Word 读取）
                fsegs = _collect_segments(p)
                ftext = "".join(x.text for x in fsegs)
                if ftext:
                    fallback_paras.append(Para(n, "回退副本", p, fsegs, ftext, 0))
                continue
            segs = _collect_segments(p)
            text = "".join(s.text for s in segs)
            if not text:
                continue  # 空段落不占偏移（parse / redact 共用本逻辑，保证一致）
            para = Para(n, _label_for(n, p), p, segs, text, offset)
            paras.append(para)
            parts_text.append(text)
            offset += len(text) + 1
    aux: list = []
    with zipfile.ZipFile(path) as zf2:
        aux_items = _aux_collect(zf2, roots)
    for label, text in aux_items:
        aux.append((label, text, offset))
        parts_text.append(text)
        offset += len(text) + 1
    return DocxModel(path, roots, paras, "\n".join(parts_text), table_count, fallback_paras, aux)


def _apply_to_para(para: Para, repls: list) -> bool:
    """段内替换：[(lo, hi, repl)] 互不重叠。起点所在文本节点并入替换文本，其余节点删去被覆盖的字符。"""
    changed = False
    for seg in para.segments:
        if seg.elem is None or not seg.text:
            continue
        rs, re_ = seg.start, seg.start + len(seg.text)
        parts, cursor = [], 0
        for lo, hi, repl in repls:
            if hi <= rs or lo >= re_:
                continue
            ds, de = max(lo - rs, 0), min(hi - rs, len(seg.text))
            if de <= ds:
                continue
            parts.append(seg.text[cursor:ds])
            if rs <= lo < re_:
                parts.append(repl)
            cursor = de
        parts.append(seg.text[cursor:])
        new = "".join(parts)
        if new != seg.text:
            seg.elem.text = new
            # 保留首尾空白
            seg.elem.set(f"{{{XML_NS}}}space", "preserve")
            changed = True
    return changed


def build_spans(model: DocxModel, doc_id: str) -> list:
    """每个可编辑文本节点一个 span（偏移为全文字符偏移）。返回 dict 列表，由 parsers 转为 Span。"""
    out = []
    for i, para in enumerate(model.paras):
        for seg in para.segments:
            if seg.elem is None or not seg.text:
                continue
            out.append(
                {
                    "block": para.label,
                    "line": i,
                    "span_start": para.start + seg.start,
                    "span_end": para.start + seg.start + len(seg.text),
                    "text": seg.text,
                }
            )
    for j, (label, text, start) in enumerate(model.aux):
        out.append(
            {
                "block": label,
                "line": len(model.paras) + j,
                "span_start": start,
                "span_end": start + len(text),
                "text": text,
            }
        )
    return out


# ---------------------------------------------------------------------------
# 元数据 / 作者清理
# ---------------------------------------------------------------------------

_CORE_CLEAR = (
    "{http://purl.org/dc/elements/1.1/}creator",
    "{http://purl.org/dc/elements/1.1/}title",
    "{http://purl.org/dc/elements/1.1/}subject",
    "{http://purl.org/dc/elements/1.1/}description",
    "{http://purl.org/dc/elements/1.1/}identifier",
    "{http://purl.org/dc/elements/1.1/}language",
    "{http://schemas.openxmlformats.org/package/2006/metadata/core-properties}lastModifiedBy",
    "{http://schemas.openxmlformats.org/package/2006/metadata/core-properties}keywords",
    "{http://schemas.openxmlformats.org/package/2006/metadata/core-properties}category",
    "{http://schemas.openxmlformats.org/package/2006/metadata/core-properties}contentStatus",
)
_APP_CLEAR = ("Company", "Manager", "HyperlinkBase", "Template")


def _scrub_authors(root) -> int:
    n = 0
    for el in root.iter():
        if not isinstance(el.tag, str):
            continue
        if _AUTHOR in el.attrib:
            el.set(_AUTHOR, "Author")
            n += 1
        if _INITIALS in el.attrib:
            el.set(_INITIALS, "A")
    return n


def _scrub_core(data: bytes) -> bytes:
    root = etree.fromstring(data)
    for el in root.iter():
        if el.tag in _CORE_CLEAR:
            el.text = ""
    return etree.tostring(root, xml_declaration=True, encoding="UTF-8", standalone=True)


def _scrub_app(data: bytes) -> bytes:
    root = etree.fromstring(data)
    for el in root.iter():
        if isinstance(el.tag, str) and etree.QName(el).localname in _APP_CLEAR:
            el.text = ""
    return etree.tostring(root, xml_declaration=True, encoding="UTF-8", standalone=True)


def _scrub_custom(data: bytes) -> bytes:
    root = etree.fromstring(data)
    for ch in list(root):
        root.remove(ch)
    return etree.tostring(root, xml_declaration=True, encoding="UTF-8", standalone=True)


def _scrub_people(data: bytes) -> bytes:
    root = etree.fromstring(data)
    _scrub_authors(root)
    for el in root.iter():
        if isinstance(el.tag, str) and etree.QName(el).localname == "presenceInfo":
            el.attrib.clear()
    return etree.tostring(root, xml_declaration=True, encoding="UTF-8", standalone=True)


_THUMB_REL = re.compile(rb"<Relationship\b[^>]*thumbnail[^>]*/>", re.I)


def write_docx(model: DocxModel, replacements: list, output_path: str):
    """应用替换并写出新 docx。返回 (replaced_count, unplaced 列表)。"""
    per_para: dict = {}
    unplaced: list = []
    aux_pairs: dict = {}
    for (s, e, repl) in replacements:
        placed = False
        for idx, para in enumerate(model.paras):
            pe = para.start + len(para.text)
            if para.start <= s and e <= pe:
                per_para.setdefault(idx, []).append((s - para.start, e - para.start, repl))
                placed = True
                break
        if not placed:
            for (label, atext, astart) in model.aux:
                if astart <= s and e <= astart + len(atext):
                    aux_pairs[model.text[s:e]] = repl
                    placed = True
                    break
        if not placed:
            unplaced.append((s, e, repl))

    replaced = len(aux_pairs)
    touched_parts: set = set()
    # 图片替代文字（属性值）：按“原文→替换文本”改写
    if aux_pairs:
        for name, root in model.roots.items():
            for el in root.iter():
                if isinstance(el.tag, str) and etree.QName(el).localname in _ALT_ELEMS:
                    for a in _ALT_ATTRS:
                        v = el.get(a)
                        if v:
                            nv = v
                            for o, r_ in aux_pairs.items():
                                nv = nv.replace(o, r_)
                            if nv != v:
                                el.set(a, nv)
                                touched_parts.add(name)
    for idx, repls in per_para.items():
        repls.sort(key=lambda x: x[0])
        if _apply_to_para(model.paras[idx], repls):
            replaced += len(repls)
            touched_parts.add(model.paras[idx].part)

    # 回退副本（mc:Fallback）：按“原文→替换文本”逐段同步改写
    pairs: dict = {}
    for (s_, e_, repl) in replacements:
        orig = model.text[s_:e_]
        if orig and (s_, e_, repl) not in unplaced:
            pairs[orig] = repl
    if pairs:
        for fp in model.fallback_paras:
            local: list = []
            for orig, repl in pairs.items():
                for m in re.finditer(re.escape(orig), fp.text):
                    local.append((m.start(), m.end(), repl))
            if local:
                local.sort()
                # 去重叠（保留先出现者）
                merged, last = [], -1
                for lo, hi, repl in local:
                    if lo >= last:
                        merged.append((lo, hi, repl))
                        last = hi
                if _apply_to_para(fp, merged):
                    touched_parts.add(fp.part)

    # 作者信息清理：所有文本部件
    for name, root in model.roots.items():
        if _scrub_authors(root):
            touched_parts.add(name)

    with zipfile.ZipFile(model.path) as zin, zipfile.ZipFile(
        output_path, "w", zipfile.ZIP_DEFLATED
    ) as zout:
        for info in zin.infolist():
            name = info.filename
            data = zin.read(name)
            if name in model.roots and name in touched_parts:
                data = etree.tostring(
                    model.roots[name], xml_declaration=True, encoding="UTF-8", standalone=True
                )
            elif name == "docProps/core.xml":
                data = _scrub_core(data)
            elif name == "docProps/app.xml":
                data = _scrub_app(data)
            elif name == "docProps/custom.xml":
                data = _scrub_custom(data)
            elif name.startswith("word/") and name.endswith(".rels") and aux_pairs:
                from xml.sax.saxutils import escape as _esc
                txt = data.decode("utf-8", "ignore")
                for o, r_ in aux_pairs.items():
                    txt = txt.replace(_esc(o, {'"': "&quot;"}), _esc(r_, {'"': "&quot;"})).replace(_esc(o), _esc(r_))
                data = txt.encode("utf-8")
            elif name == "word/people.xml":
                data = _scrub_people(data)
            elif re.match(r"^docProps/thumbnail\.\w+$", name):
                continue  # 缩略图可能含首页内容：直接丢弃
            elif name == "_rels/.rels":
                data = _THUMB_REL.sub(b"", data)
            zi = zipfile.ZipInfo(name, date_time=info.date_time)
            zi.compress_type = zipfile.ZIP_DEFLATED
            zi.external_attr = info.external_attr
            zout.writestr(zi, data)
    return replaced, unplaced
