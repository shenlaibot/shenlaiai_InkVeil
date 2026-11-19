"""PDF 安全模式：命中页栅格化 + 不可见文字层。

为什么：PDF 内容流里的字符多为子集化字体的字形号（Identity-H）或单字节编码，无法可靠地按“原文”
在内容流里删除，字符串匹配会失败，导致原文仍可被提取（基准中深度脱敏 0/8）。

做法（命中页）：
1. 用 PDFium 渲染整页为位图，在命中字符的包围盒上涂黑（原文像素被抹掉）；
2. 用位图页替换原页：原内容流、字体、XObject、注释、缩略图等不再被引用，qpdf 保存时不会写出；
3. 重建**不可见**文字层（渲染模式 3 + 无字形字体 + ToUnicode），仅包含**未命中**的字符，
   使页面仍可搜索/复制，而被脱敏的文字彻底不存在；
4. 无命中的页保持原样（混合模式）。文档级再清理：Info/XMP、书签、附件、表单、结构树等。

局限：被栅格化的页失去矢量与原有文字层；旋转页不生成文字层（仅涂黑位图）。
"""

from __future__ import annotations

import io

# 渲染分辨率（dpi）：200 在可读性与体积之间折中
RENDER_DPI = 200
JPEG_QUALITY = 85


def _to_display(x: float, y: float, rot: int, cb_w: float, cb_h: float):
    """用户空间点(相对 CropBox 原点) -> 显示空间点（原点左下，随页面旋转）。"""
    if rot == 90:
        return y, cb_w - x
    if rot == 180:
        return cb_w - x, cb_h - y
    if rot == 270:
        return cb_h - y, x
    return x, y


def _box_to_px(box, rot, cbox, disp_h, scale, pad=1.0):
    l, b, r, t = box
    cl, cb_, cr, ct = cbox
    cb_w, cb_h = cr - cl, ct - cb_
    pts = [
        _to_display(x - cl, y - cb_, rot, cb_w, cb_h)
        for x, y in ((l, b), (r, b), (l, t), (r, t))
    ]
    xs = [p[0] for p in pts]
    ys = [p[1] for p in pts]
    x0, x1 = (min(xs) - pad) * scale, (max(xs) + pad) * scale
    # 显示空间 y 向上；像素行向下
    y0, y1 = (disp_h - (max(ys) + pad)) * scale, (disp_h - (min(ys) - pad)) * scale
    return int(max(x0, 0)), int(max(y0, 0)), int(x1) + 1, int(y1) + 1


def _hex16(ch: str) -> str:
    b = ch.encode("utf-16-be")
    return b.hex().upper()


class _GlyphlessFont:
    """文档级共享的无字形 Type0 字体：CID 由字符动态分配，ToUnicode 记录映射。"""

    def __init__(self):
        self.cid_of: dict = {}

    def cid(self, ch: str) -> int:
        c = self.cid_of.get(ch)
        if c is None:
            c = len(self.cid_of) + 1
            if c > 0xFFFE:
                raise ValueError("字符种类过多")
            self.cid_of[ch] = c
        return c

    def build(self, pdf):
        import pikepdf
        from pikepdf import Array, Dictionary, Name, String

        lines = [
            "/CIDInit /ProcSet findresource begin",
            "12 dict begin",
            "begincmap",
            "/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def",
            "/CMapName /Adobe-Identity-UCS def",
            "/CMapType 2 def",
            "1 begincodespacerange",
            "<0000> <FFFF>",
            "endcodespacerange",
        ]
        items = sorted(self.cid_of.items(), key=lambda kv: kv[1])
        for i in range(0, len(items), 100):
            chunk = items[i : i + 100]
            lines.append(f"{len(chunk)} beginbfchar")
            for ch, cid in chunk:
                lines.append(f"<{cid:04X}> <{_hex16(ch)}>")
            lines.append("endbfchar")
        lines += ["endcmap", "CMapName currentdict /CMap defineresource pop", "end", "end"]
        tounicode = pikepdf.Stream(pdf, "\n".join(lines).encode("ascii"))

        descriptor = pdf.make_indirect(
            Dictionary(
                Type=Name.FontDescriptor,
                FontName=Name("/AnonKitGlyphless"),
                Flags=4,
                FontBBox=Array([0, 0, 1000, 1000]),
                ItalicAngle=0,
                Ascent=1000,
                Descent=0,
                CapHeight=1000,
                StemV=0,
            )
        )
        cidfont = pdf.make_indirect(
            Dictionary(
                Type=Name.Font,
                Subtype=Name.CIDFontType2,
                BaseFont=Name("/AnonKitGlyphless"),
                CIDSystemInfo=Dictionary(
                    Registry=String("Adobe"), Ordering=String("Identity"), Supplement=0
                ),
                FontDescriptor=descriptor,
                DW=1000,
                CIDToGIDMap=Name.Identity,
            )
        )
        return pdf.make_indirect(
            Dictionary(
                Type=Name.Font,
                Subtype=Name.Type0,
                BaseFont=Name("/AnonKitGlyphless"),
                Encoding=Name("/Identity-H"),
                DescendantFonts=Array([cidfont]),
                ToUnicode=tounicode,
            )
        )


def _text_runs(chars: list):
    """chars: [(idx, ch, (l,b,r,t))]（已排除命中字符）-> 连续同基线的 run 列表。"""
    runs, cur = [], []
    for item in chars:
        idx, ch, box = item
        if cur:
            pidx, _, pbox = cur[-1]
            same_line = abs(box[1] - pbox[1]) < 0.4 * max(box[3] - box[1], 1e-6)
            contiguous = idx == pidx + 1 and box[0] >= pbox[0] - 0.5 * (pbox[2] - pbox[0])
            if not (same_line and contiguous):
                runs.append(cur)
                cur = []
        cur.append(item)
    if cur:
        runs.append(cur)
    return runs


def _invisible_text_stream(items: list, font: _GlyphlessFont, tx) -> bytes:
    """items: [(text, (l,b,r,t))]，box 为页面用户空间；tx(x, y) -> 显示空间坐标（含页面旋转）。

    每项按其自身朝向生成一个 Tm（含旋转），因此旋转页的文字层与位图方向一致。
    """
    import math

    out = [b"BT", b"3 Tr"]
    for text, (l, b, r, t) in items:
        if not text:
            continue
        h = max(t - b, 1.0)
        width = max(r - l, 0.1)
        ox, oy = tx(l, b)
        x1, y1 = tx(l + 1.0, b)
        x2, y2 = tx(l, b + 1.0)
        dx, dy = x1 - ox, y1 - oy
        ux, uy = x2 - ox, y2 - oy
        dn = math.hypot(dx, dy) or 1.0
        un = math.hypot(ux, uy) or 1.0
        dx, dy, ux, uy = dx / dn, dy / dn, ux / un, uy / un
        tz = min(max(width / (len(text) * h) * 100.0, 5.0), 600.0)
        cids = "".join(f"{font.cid(ch):04X}" for ch in text)
        out.append(
            f"/F1 {h:.2f} Tf {tz:.1f} Tz {dx:.4f} {dy:.4f} {ux:.4f} {uy:.4f} {ox:.2f} {oy:.2f} Tm <{cids}> Tj".encode("ascii")
        )
    out.append(b"ET")
    return b"\n".join(out)


def _strip_document_extras(pdf):
    """文档级清理：Info/XMP、书签、附件、表单、结构树、JS、页面缩略图等可能携带原文的部位。"""
    import pikepdf
    from pikepdf import Name

    try:
        if Name.Info in pdf.trailer:
            del pdf.trailer.Info
    except Exception:
        pass
    try:
        with pdf.open_metadata() as meta:
            for k in list(meta.keys()):
                del meta[k]
    except Exception:
        pass
    root = pdf.Root
    for key in ("/Outlines", "/AcroForm", "/StructTreeRoot", "/MarkInfo", "/PieceInfo",
                "/Metadata", "/OpenAction", "/AA", "/Threads", "/PageLabels",
                "/AF"):  # /AF：PDF 2.0 关联文件，会引用附件内容流
        try:
            if key in root:
                del root[key]
        except Exception:
            pass
    try:
        names = root.get("/Names")
        if names is not None:
            for k in ("/EmbeddedFiles", "/JavaScript"):
                if k in names:
                    del names[k]
    except Exception:
        pass
    for page in pdf.pages:
        for k in ("/Thumb", "/PieceInfo", "/Metadata", "/AF"):
            try:
                if k in page:
                    del page[k]
            except Exception:
                pass


_ANNOT_TEXT_KEYS = ("/Contents", "/T", "/Subj", "/TU", "/RC")


def _annot_strings(annot) -> list:
    """注释里可能携带敏感文字的字符串字段（正文、作者、主题、链接 URI）。"""
    out = []
    try:
        for k in _ANNOT_TEXT_KEYS:
            if k in annot:
                out.append(str(annot[k]))
        a = annot.get("/A")
        if a is not None and "/URI" in a:
            out.append(str(a["/URI"]))
    except Exception:
        pass
    return [x for x in out if x]


def pdf_aux_segments(path: str) -> list:
    """不在页面文字里、但保留在文件里的文本：注释/链接的文字字段。返回 [(label, text)]（顺序固定，parse 与 redact 共用）。"""
    import pikepdf

    out = []
    try:
        with pikepdf.open(path) as pdf:
            for pno, page in enumerate(pdf.pages):
                for annot in page.get("/Annots", []) or []:
                    try:
                        for t in _annot_strings(annot):
                            out.append((f"页{pno + 1}注释", t))
                    except Exception:
                        continue
    except Exception:
        return []
    return out


def redact_pdf_safe(input_path: str, replacements: list, output_path: str, pdfium_lock=None) -> dict:
    """replacements: [(start, end, replacement)]，基于 parse_pdf 的全文偏移（与 redact_pdf 一致）。"""
    import numpy as np
    import pikepdf
    import pypdfium2 as pdfium
    from PIL import Image
    from pikepdf import Array, Dictionary, Name

    import contextlib

    lock = pdfium_lock if pdfium_lock is not None else contextlib.nullcontext()
    scale = RENDER_DPI / 72.0

    warnings: list = []
    page_data: dict = {}  # pno -> dict(jpeg, w_px, h_px, w_pt, h_pt, runs|None)
    total_replaced = 0
    page_offsets, page_texts = [], []

    with lock:
        src = pdfium.PdfDocument(input_path)
        try:
            offset = 0
            for pno in range(len(src)):
                page = src[pno]
                tp = None
                try:
                    tp = page.get_textpage()
                    text = tp.get_text_bounded()
                    page_offsets.append(offset)
                    page_texts.append(text)
                    ps = offset
                    offset += len(text) + 1

                    hit_ranges = []
                    for (s, e, repl) in replacements:
                        ls, le = s - ps, e - ps
                        if 0 <= ls < le <= len(text):
                            hit_ranges.append((ls, le))
                    if not hit_ranges:
                        continue

                    rot = page.get_rotation() % 360
                    cbox = page.get_cropbox()
                    cb_w, cb_h = cbox[2] - cbox[0], cbox[3] - cbox[1]
                    disp_w, disp_h = (cb_h, cb_w) if rot in (90, 270) else (cb_w, cb_h)

                    hit_idx: set = set()
                    boxes = []
                    for ls, le in hit_ranges:
                        for i in range(ls, le):
                            hit_idx.add(i)
                            b = tp.get_charbox(i)
                            if b[2] > b[0] and b[3] > b[1]:
                                boxes.append(b)

                    bitmap = page.render(scale=scale, rev_byteorder=True)
                    arr = np.array(bitmap.to_numpy())[:, :, :3].copy()
                    h_px, w_px = arr.shape[0], arr.shape[1]
                    # 涂黑命中字符的包围盒（逐字符，含行内空隙，不暴露被遮盖文本的长度分布以外的信息）
                    for box in boxes:
                        x0, y0, x1, y1 = _box_to_px(box, rot, cbox, disp_h, scale)
                        arr[y0 : min(y1, h_px), x0 : min(x1, w_px), :] = 0
                    buf = io.BytesIO()
                    Image.fromarray(arr).save(buf, format="JPEG", quality=JPEG_QUALITY)

                    # 文字层：未命中字符（原位置）+ 命中处的替换文本（占位符/掩码），
                    # 使页面可检索、AI 能读到占位符、映射表还原也成立；原文本身不进入文字层
                    chars = []
                    for i, ch in enumerate(text):
                        if i in hit_idx or ch in "\r\n\t":
                            continue
                        b = tp.get_charbox(i)
                        if b[2] > b[0] and b[3] > b[1]:
                            chars.append((i, ch, (b[0], b[1], b[2], b[3])))
                    items = []
                    for run in _text_runs(chars):
                        items.append(
                            (
                                "".join(c[1] for c in run),
                                (
                                    min(c[2][0] for c in run),
                                    min(c[2][1] for c in run),
                                    max(c[2][2] for c in run),
                                    max(c[2][3] for c in run),
                                ),
                            )
                        )
                    for (s_, e_, repl) in replacements:
                        ls, le = s_ - ps, e_ - ps
                        if not (0 <= ls < le <= len(text)) or not repl:
                            continue
                        hb = []
                        for i in range(ls, le):
                            b = tp.get_charbox(i)
                            if b[2] > b[0] and b[3] > b[1]:
                                hb.append((i, text[i], (b[0], b[1], b[2], b[3])))
                        runs_h = _text_runs(hb)
                        if runs_h:
                            r0 = runs_h[0]
                            items.append(
                                (
                                    repl,
                                    (
                                        min(c[2][0] for c in r0),
                                        min(c[2][1] for c in r0),
                                        max(c[2][2] for c in r0),
                                        max(c[2][3] for c in r0),
                                    ),
                                )
                            )
                    runs = items
                    page_data[pno] = dict(
                        jpeg=buf.getvalue(), w_px=w_px, h_px=h_px, w_pt=disp_w, h_pt=disp_h, runs=runs,
                        rot=rot, cbox=tuple(cbox),
                    )
                    total_replaced += len(hit_ranges)
                finally:
                    if tp is not None:
                        tp.close()
                    page.close()
        finally:
            src.close()

    # 注释/链接文字（parse 把它们接在全文末尾）：命中只登记为“需清理的原文”
    aux_segs = pdf_aux_segments(input_path)
    aux_ranges, pos = [], offset
    for _, atext in aux_segs:
        aux_ranges.append((pos, pos + len(atext), atext))
        pos += len(atext) + 1
    needles: set = set()
    for (s_, e_, repl) in replacements:
        for a0, a1, atext in aux_ranges:
            if a0 <= s_ and e_ <= a1:
                needles.add(atext[s_ - a0 : e_ - a0])
        for po, pt in zip(page_offsets, page_texts):
            if po <= s_ and e_ <= po + len(pt):
                needles.add(pt[s_ - po : e_ - po])
    needles = {n for n in needles if len(n) >= 2}

    def _placed(s_, e_):
        return any(po <= s_ and e_ <= po + len(pt) for po, pt in zip(page_offsets, page_texts)) or any(
            a0 <= s_ and e_ <= a1 for a0, a1, _ in aux_ranges
        )

    # 全文偏移落不到任何页/注释（跨页/排版拆开）：原文仍在输出里，必须明示
    unplaced = 0
    for (s_, e_, repl) in replacements:
        if not _placed(s_, e_):
            unplaced += 1
            warnings.append(f"替换 [{s_},{e_}) 未映射到任何页面，原文仍保留在输出文件中（请人工处理）")

    with pikepdf.open(input_path) as pdf:
        font_obj = _GlyphlessFont()
        # 先生成所有页内容（需要先分配 CID），最后统一构建字体
        page_streams: dict = {}
        for pno, d in page_data.items():
            content = [f"q {d['w_pt']:.2f} 0 0 {d['h_pt']:.2f} 0 0 cm /Im0 Do Q".encode("ascii")]
            if d["runs"]:
                cl, cb_, cr, ct = d["cbox"]
                cw, ch_ = cr - cl, ct - cb_

                def tx(x, y, _r=d["rot"], _cl=cl, _cb=cb_, _cw=cw, _ch=ch_):
                    return _to_display(x - _cl, y - _cb, _r, _cw, _ch)

                content.append(_invisible_text_stream(d["runs"], font_obj, tx))
            page_streams[pno] = b"\n".join(content)
        font_ref = font_obj.build(pdf) if font_obj.cid_of else None

        for pno, d in page_data.items():
            page = pdf.pages[pno]
            img = pikepdf.Stream(pdf, d["jpeg"])
            img.Type = Name.XObject
            img.Subtype = Name.Image
            img.Width = d["w_px"]
            img.Height = d["h_px"]
            img.ColorSpace = Name.DeviceRGB
            img.BitsPerComponent = 8
            img.Filter = Name.DCTDecode
            res = Dictionary(XObject=Dictionary(Im0=pdf.make_indirect(img)))
            if font_ref is not None and d["runs"]:
                res.Font = Dictionary(F1=font_ref)
            page.Resources = res
            page.Contents = pikepdf.Stream(pdf, page_streams[pno])
            page.MediaBox = Array([0, 0, d["w_pt"], d["h_pt"]])
            page.CropBox = Array([0, 0, d["w_pt"], d["h_pt"]])
            for k in ("/Rotate", "/Annots", "/Group"):
                if k in page:
                    del page[k]
        # 未栅格化的页：含已确认原文的注释/链接直接删除（其余注释保持原样，不破坏正常链接）
        if needles:
            for pno, page in enumerate(pdf.pages):
                if pno in page_data or "/Annots" not in page:
                    continue
                keep = []
                for annot in page.Annots:
                    if any(n in t for t in _annot_strings(annot) for n in needles):
                        continue
                    keep.append(annot)
                if len(keep) != len(page.Annots):
                    page.Annots = Array(keep)
        _strip_document_extras(pdf)
        pdf.save(output_path)

    warnings.append(
        "命中页已栅格化并涂黑：这些页不再是矢量/原文字层；输出为脱敏副本，不能替代原件"
    ) if page_data else None
    return {
        "is_deep": unplaced == 0,
        "replaced_count": total_replaced,
        "warnings": warnings,
        "unplaced_count": unplaced,
    }
