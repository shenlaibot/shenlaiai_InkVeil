"""导出后残留复检（Residue Verifier v0）。

对**输出文件**的所有可提取内容反查“已确认要脱敏的原文”：
- OOXML（docx/xlsx）：遍历压缩包内全部 part（正文、批注、页眉页脚、元数据、sheet 名、缩略图旁数据等），
  同时检查原始 XML 与“去标签后的拼接文本”（敏感词被拆进多个 run 时也能发现）；
- PDF：遍历全部对象（含 XObject、注释、元数据）解压后的字节，搜索 UTF-8 / UTF-16BE / GBK 编码形态，
  并用 PDFium 提取各页文字再搜索；
- 文本：按原编码解码后搜索。

检查范围的局限（必须如实告知用户）：只能证明“已知原文没有残留”，
不能证明“没有漏检”；图片/扫描件内的文字不在检查范围。
结果只含位置与长度，**绝不回传原文**。
"""

from __future__ import annotations

import html
import re
import unicodedata
import zipfile

_TAG = re.compile(r"<[^>]+>")
_SEP = re.compile(r"[\s\-·_.　]+")


def _norm(s: str) -> str:
    """规范化：NFKC（全角→半角等）+ 去空白与常见分隔符，用于发现被改写格式的残留。"""
    return _SEP.sub("", unicodedata.normalize("NFKC", s))


def _variants(needle: str) -> list[str]:
    out = [needle]
    n = _norm(needle)
    if n and n != needle:
        out.append(n)
    return out


def _hit(haystack: str, needle: str) -> int:
    """返回 needle 在 haystack 中（含规范化形态）出现的次数。"""
    cnt = haystack.count(needle)
    if cnt:
        return cnt
    nn = _norm(needle)
    if len(nn) >= 2:
        return _norm(haystack).count(nn)
    return 0


def _scan_text_blob(label: str, blob: str, needles: list[str], found: dict) -> None:
    for i, nd in enumerate(needles):
        c = _hit(blob, nd)
        if c:
            found.setdefault((i, label), 0)
            found[(i, label)] += c


def _scan_zip(path: str, needles: list[str]) -> dict:
    found: dict = {}
    with zipfile.ZipFile(path) as zf:
        for name in zf.namelist():
            try:
                data = zf.read(name)
            except Exception:
                continue
            # 部件名本身也可能含原文（如以姓名命名的附件）
            _scan_text_blob(f"{name}（部件名）", name, needles, found)
            if not name.lower().endswith((".xml", ".rels", ".txt", ".csv", ".json")):
                continue
            try:
                raw = data.decode("utf-8", "ignore")
            except Exception:
                continue
            _scan_text_blob(name, html.unescape(raw), needles, found)
            stripped = html.unescape(_TAG.sub("", raw))
            _scan_text_blob(f"{name}（文本）", stripped, needles, found)
    return found


def _scan_pdf(path: str, needles: list[str]) -> dict:
    found: dict = {}
    with open(path, "rb") as f:
        whole = f.read()
    # 1) 原始字节与解压后的流字节：UTF-8 / UTF-16BE / GBK 三种编码形态
    blobs: list[tuple[str, bytes]] = [("文件原始字节", whole)]
    try:
        import pikepdf

        with pikepdf.open(path) as pdf:
            for obj in pdf.objects:
                try:
                    if isinstance(obj, pikepdf.Stream):
                        blobs.append((f"对象 {obj.objgen[0]} 流", obj.read_bytes()))
                except Exception:
                    continue
            try:
                for k, v in (pdf.docinfo or {}).items():
                    blobs.append((f"文档信息 {k}", str(v).encode("utf-8", "ignore")))
            except Exception:
                pass
            try:
                with pdf.open_metadata() as meta:
                    blobs.append(("XMP 元数据", str(meta).encode("utf-8", "ignore")))
            except Exception:
                pass
    except Exception:
        pass
    for label, b in blobs:
        for i, nd in enumerate(needles):
            for enc in ("utf-8", "utf-16-be", "gbk"):
                try:
                    pat = nd.encode(enc)
                except UnicodeEncodeError:
                    continue
                if pat and pat in b:
                    found.setdefault((i, label), 0)
                    found[(i, label)] += b.count(pat)
                    break
    # 2) PDFium 实际可提取的文字（用户复制/AI 读到的内容）
    try:
        import pypdfium2 as pdfium

        doc = pdfium.PdfDocument(path)
        try:
            for pno in range(len(doc)):
                page = doc[pno]
                tp = page.get_textpage()
                txt = tp.get_text_range()
                _scan_text_blob(f"第 {pno + 1} 页文字", txt, needles, found)
        finally:
            doc.close()
    except Exception:
        pass
    return found


def _scan_plain(path: str, needles: list[str]) -> dict:
    from parsers import read_text_file

    found: dict = {}
    text, _ = read_text_file(path)
    _scan_text_blob("正文", text, needles, found)
    return found


def find_residue(path: str, fmt: str, needles: list[str]) -> list[dict]:
    """返回 [{"index", "length", "where", "count"}]；index 为 needles 下标，不含原文。"""
    needles = [n for n in dict.fromkeys(needles) if n and len(n) >= 2]
    if not needles:
        return []
    if fmt in ("docx", "xlsx"):
        found = _scan_zip(path, needles)
    elif fmt == "pdf":
        found = _scan_pdf(path, needles)
    else:
        found = _scan_plain(path, needles)
    return [
        {"index": i, "length": len(needles[i]), "where": where, "count": c}
        for (i, where), c in sorted(found.items())
    ]
