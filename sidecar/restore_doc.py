"""文档还原：把 docx / xlsx 里的占位符按映射表还原为原文，另存新文件（不覆盖输入）。

匹配规则与 Rust `MappingTable::restore_fuzzy` 保持一致（修改时两边同步）：
- 精确：与标准写法 `[标签n]` 完全一致；
- 模糊：全角/六角括号、括号内空格、全角数字、大小写、Markdown 转义等；
- 疑似未还原：形如 `[xxx数字]` 但不在映射表中。

docx：沿用 ooxml_docx 的段落文本模型，占位符被 Word 拆进多个 run 时跨 run 匹配并写回
（替换文字落在命中起点所在 run，沿用其格式），其余结构原样保留；
xlsx：直接改 XML —— 共享字符串、inlineStr、批注、绘图文字；公式单元格不动。
映射表经请求体传入（内存），不落盘。
"""

from __future__ import annotations

import os
import re
import zipfile
from typing import Optional

from lxml import etree

from ooxml_docx import Para, Segment, _apply_to_para, load_docx

_OPENERS = "[［【〔「『(（{｛<＜"
_CLOSERS = "]］】〕」』)）}｝>＞"
_SUSPECT_OPENERS = "[［【〔"
_MAX_INNER = 40
_ZERO_WIDTH = "​‌‍﻿"


def _normalize_inner(s: str) -> str:
    out = []
    for c in s:
        if (c.isspace() and c not in "\n\t") or c in _ZERO_WIDTH or c == "\\":
            continue
        if "！" <= c <= "～":
            c = chr(ord(c) - 0xFEE0)
        out.append(c.lower())
    return "".join(out)


def _looks_like_placeholder(norm: str) -> bool:
    m = re.search(r"\d+$", norm)
    if not m or len(m.group()) > 6:
        return False
    prefix = norm[: m.start()]
    return 0 < len(prefix) <= 12 and not any(ch.isdigit() or ch.isspace() for ch in prefix)


def _scan_brackets(text: str):
    """生成 (start, end, open, close, inner)；与 Rust scan_brackets 一致。"""
    i, n = 0, len(text)
    while i < n:
        if text[i] in _OPENERS:
            j, found = i + 1, None
            while j < n and j - i - 1 <= _MAX_INNER:
                c = text[j]
                if c in _CLOSERS:
                    found = j
                    break
                if c in _OPENERS:
                    break
                j += 1
            if found is not None:
                inner = text[i + 1 : found]
                if not any(ch in inner for ch in "\n\t\r"):
                    start = i - 1 if i > 0 and text[i - 1] == "\\" else i
                    yield start, found + 1, text[i], text[found], inner
                i = found + 1
                continue
        i += 1


class Restorer:
    def __init__(self, mapping: dict[str, str]):
        self.mapping = dict(mapping)
        idx: dict[str, Optional[str]] = {}
        for p in self.mapping:
            key = _normalize_inner(p[1:-1] if p.startswith("[") and p.endswith("]") else p)
            idx[key] = None if key in idx else p
        self.index = {k: v for k, v in idx.items() if v is not None}
        self.exact_count = 0
        self.fuzzy_hits: list[dict] = []
        self.suspects: list[dict] = []

    def find(self, text: str, where: str = "") -> list[tuple[int, int, str]]:
        """返回 [(lo, hi, 原文)]，同时累计统计；where 是命中所在位置的描述。"""
        repls = []
        for start, end, op, cl, inner in _scan_brackets(text):
            raw = text[start:end]
            std = f"[{inner}]"
            if raw == std and std in self.mapping:
                ph = std
            else:
                norm = _normalize_inner(inner)
                ph = self.index.get(norm)
            if ph is None:
                if op in _SUSPECT_OPENERS and _looks_like_placeholder(_normalize_inner(inner)):
                    self.suspects.append({"text": raw, "where": where})
                continue
            original = self.mapping[ph]
            repls.append((start, end, original))
            if raw == ph:
                self.exact_count += 1
            else:
                self.fuzzy_hits.append(
                    {"raw": raw, "placeholder": ph, "original": original, "where": where}
                )
        return repls

    def restore_text(self, text: str, where: str = "") -> str:
        repls = self.find(text, where)
        out, cur = [], 0
        for lo, hi, rep in repls:
            out.append(text[cur:lo])
            out.append(rep)
            cur = hi
        out.append(text[cur:])
        return "".join(out)

    def stats(self) -> dict:
        return {
            "exact_count": self.exact_count,
            "fuzzy_count": len(self.fuzzy_hits),
            "fuzzy_hits": self.fuzzy_hits,
            "suspects": self.suspects,
        }


# ---------------------------------------------------------------------------
# 通用：文本节点容器内跨节点匹配并写回
# ---------------------------------------------------------------------------


def _restore_container(rs: Restorer, nodes: list, where: str) -> bool:
    segs, pos = [], 0
    for el in nodes:
        t = el.text or ""
        segs.append(Segment(el, t, pos))
        pos += len(t)
    text = "".join(s.text for s in segs)
    if not text:
        return False
    repls = rs.find(text, where)
    if not repls:
        return False
    return _apply_to_para(Para("", "", None, segs, text, 0), repls)


def _unique_output(input_path: str, output_dir: str) -> str:
    os.makedirs(output_dir, exist_ok=True)
    stem, ext = os.path.splitext(os.path.basename(input_path))
    out = os.path.join(output_dir, f"{stem}_已还原{ext}")
    n = 2
    while os.path.exists(out):
        out = os.path.join(output_dir, f"{stem}_已还原({n}){ext}")
        n += 1
    return out


def _write_zip(input_path: str, output_path: str, changed: dict[str, bytes]) -> None:
    tmp = output_path + ".tmp"
    with zipfile.ZipFile(input_path) as zin, zipfile.ZipFile(tmp, "w", zipfile.ZIP_DEFLATED) as zout:
        for info in zin.infolist():
            data = changed.get(info.filename)
            if data is None:
                data = zin.read(info.filename)
            zout.writestr(info, data, compress_type=zipfile.ZIP_DEFLATED)
    os.replace(tmp, output_path)


def _ser(root) -> bytes:
    return etree.tostring(root, xml_declaration=True, encoding="UTF-8", standalone=True)


# ---------------------------------------------------------------------------
# docx
# ---------------------------------------------------------------------------


def restore_docx(input_path: str, rs: Restorer, output_path: str) -> None:
    model = load_docx(input_path)
    touched: set[str] = set()
    jobs = [(p, rs.find(p.text, p.label)) for p in model.paras]
    jobs += [(p, _quiet_find(rs, p.text)) for p in model.fallback_paras]
    for para, repls in jobs:
        if repls and _apply_to_para(para, repls):
            touched.add(para.part)
    changed = {name: _ser(model.roots[name]) for name in touched}
    _write_zip(input_path, output_path, changed)


def _quiet_find(rs: Restorer, text: str):
    """回退副本（文本框 VML 重复副本）要同步改写，但不重复计数。"""
    saved = (rs.exact_count, len(rs.fuzzy_hits), len(rs.suspects))
    repls = rs.find(text)
    rs.exact_count = saved[0]
    del rs.fuzzy_hits[saved[1] :]
    del rs.suspects[saved[2] :]
    return repls


# ---------------------------------------------------------------------------
# xlsx
# ---------------------------------------------------------------------------

_NS_MAIN = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
_NS_A = "http://schemas.openxmlformats.org/drawingml/2006/main"
_SHEET = re.compile(r"^xl/worksheets/[^/]+\.xml$")
_COMMENTS = re.compile(r"^xl/(comments[^/]*\.xml|comments/[^/]+\.xml|threadedComments/[^/]+\.xml)$")
_DRAWING = re.compile(r"^xl/(drawings|charts)/[^/]+\.xml$")
_SST = "xl/sharedStrings.xml"


def _tag(ns: str, name: str) -> str:
    return f"{{{ns}}}{name}"


def _own_t(container, t_tag: str, skip_tags=()) -> list:
    """容器内所有 t 节点（文档顺序）；忽略拼音注音 rPh。"""
    out = []
    for el in container.iter(t_tag):
        a, skip = el.getparent(), False
        while a is not None and a is not container:
            if a.tag in skip_tags:
                skip = True
                break
            a = a.getparent()
        if not skip:
            out.append(el)
    return out


def restore_xlsx(input_path: str, rs: Restorer, output_path: str) -> None:
    parser = etree.XMLParser(resolve_entities=False, no_network=True)
    changed: dict[str, bytes] = {}
    with zipfile.ZipFile(input_path) as zin:
        for name in zin.namelist():
            if not (name == _SST or _SHEET.match(name) or _COMMENTS.match(name) or _DRAWING.match(name)):
                continue
            root = etree.fromstring(zin.read(name), parser)
            dirty = False
            t_main, rph = _tag(_NS_MAIN, "t"), _tag(_NS_MAIN, "rPh")
            if name == _SST:
                for si in root.iter(_tag(_NS_MAIN, "si")):
                    dirty |= _restore_container(rs, _own_t(si, t_main, (rph,)), "共享字符串")
            elif _SHEET.match(name):
                for c in root.iter(_tag(_NS_MAIN, "c")):
                    is_el = c.find(_tag(_NS_MAIN, "is"))
                    if is_el is not None:
                        coord = c.get("r") or ""
                        dirty |= _restore_container(rs, _own_t(is_el, t_main, (rph,)), f"单元格 {coord}")
            elif _COMMENTS.match(name):
                for holder in root.iter(_tag(_NS_MAIN, "text")):
                    dirty |= _restore_container(rs, _own_t(holder, t_main, (rph,)), "批注")
                for holder in root.iter():
                    if isinstance(holder.tag, str) and etree.QName(holder).localname == "text" and holder.tag != _tag(_NS_MAIN, "text"):
                        # threadedComment 的 <text> 直接含文本
                        t = holder.text or ""
                        if t:
                            new = rs.restore_text(t, "批注")
                            if new != t:
                                holder.text = new
                                dirty = True
            else:  # drawing / chart：a:p 为段落
                for p in root.iter(_tag(_NS_A, "p")):
                    dirty |= _restore_container(rs, list(p.iter(_tag(_NS_A, "t"))), "图形文字")
            if dirty:
                changed[name] = _ser(root)
    _write_zip(input_path, output_path, changed)


# ---------------------------------------------------------------------------
# 入口
# ---------------------------------------------------------------------------


def restore_file(input_path: str, mapping: dict[str, str], output_dir: str) -> dict:
    if not os.path.isfile(input_path):
        raise FileNotFoundError("输入文件不存在")
    ext = os.path.splitext(input_path)[1].lower()
    if ext not in (".docx", ".xlsx"):
        raise ValueError("仅支持 .docx / .xlsx 文件的直接还原")
    out = _unique_output(input_path, output_dir)
    if os.path.abspath(out) == os.path.abspath(input_path):
        raise ValueError("输出路径不能等于输入文件")
    rs = Restorer(mapping)
    (restore_docx if ext == ".docx" else restore_xlsx)(input_path, rs, out)
    return {"output_path": out, **rs.stats(), "mapping_entries": len(mapping)}
