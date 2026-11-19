#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
PDF 深度脱敏技术可行性验证（spike）
====================================
目标：从 PDF 页面内容流中真正删除命中的敏感文本（删除后不可再被选中/复制），
      而不是仅仅在页面上画白框覆盖。

技术路线：
  1. reportlab 生成含中文敏感文本的测试 PDF（CID 字体 STSong-Light）
  2. pypdfium2 提取文本与字符级 bbox（FPDFText_* 底层 API），定位敏感文本
  3. pikepdf 读取页面内容流，解析 Tj/TJ 算子，将敏感文本字节级替换为掩码文本
     （reportlab CID 字体字符串 = UTF-16BE，可无损双向转换）
  4. 追加内容流指令：白色矩形覆盖原 bbox + 掩码文本重绘（双保险）
  5. 用 pypdfium2 重新提取文本验证：原敏感文本消失、掩码可见、版面保留

允许依赖（均可商用）：pypdfium2 (BSD-3-Clause)、pikepdf (MPL-2.0)、reportlab (BSD)。
禁止依赖：PyMuPDF (AGPL)、任何 GPL/AGPL 组件。
"""
import os
import sys
import time
import io

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')

import pypdfium2 as pdfium
import pikepdf

from reportlab.lib.pagesizes import A4
from reportlab.pdfgen import canvas
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.cidfonts import UnicodeCIDFont

# ---------------------------------------------------------------------------
# 全局配置
# ---------------------------------------------------------------------------
BASE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(BASE, "output")
SRC_PDF = os.path.join(OUT, "sample_before.pdf")
REDACTED_PDF = os.path.join(OUT, "sample_after.pdf")
TXT_BEFORE = os.path.join(OUT, "before.txt")
TXT_AFTER = os.path.join(OUT, "after.txt")

# 敏感项 -> 掩码（spike 演示用；真实产品中由脱敏规则引擎产出）
SENSITIVE = [
    ("张三", "张*"),
    ("13800138000", "138****8000"),
    ("110101198403201234", "110101**********34"),
]
# 敏感性从长到短排序，避免短串先命中造成干扰
SENSITIVE = sorted(SENSITIVE, key=lambda kv: -len(kv[0]))

# ---------------------------------------------------------------------------
# 1. 生成测试 PDF（reportlab + CID 字体渲染中文）
# ---------------------------------------------------------------------------
def gen_test_pdf(path):
    pdfmetrics.registerFont(UnicodeCIDFont("STSong-Light"))
    c = canvas.Canvas(path, pagesize=A4)
    lines = [
        "标题：个人信息登记表",
        "姓名：张三",
        "手机号：13800138000",
        "身份证号：110101198403201234",
        "住址：北京市朝阳区示例路 88 号",
        "备注：无",
    ]
    y = 760
    for line in lines:
        c.setFont("STSong-Light", 14)
        c.drawString(80, y, line)
        y -= 40
    # 第二页：多页验证 + 与其它文本混排的敏感文本
    c.showPage()
    c.setFont("STSong-Light", 14)
    c.drawString(80, 760, "第二页：普通内容，无敏感信息")
    c.drawString(80, 720, "张三的合同编号：A-2024-0012")
    c.setFont("STSong-Light", 12)
    c.drawString(80, 660, "联系手机 13800138000 已登记")
    c.showPage()
    c.save()
    print(f"[gen] 已生成测试 PDF: {path}")

# ---------------------------------------------------------------------------
# 2. pypdfium2 定位敏感文本：全页文本 + 字符级 bbox
# ---------------------------------------------------------------------------
def extract_page_texts(path):
    """返回 [(page, full_text), ...]"""
    pdf = pdfium.PdfDocument(path)
    result = []
    try:
        for pno in range(len(pdf)):
            page = pdf[pno]
            tp = page.get_textpage()
            try:
                result.append((pno, tp.get_text_bounded()))
            finally:
                tp.close()
                page.close()
    finally:
        pdf.close()
    return result

def locate_hits(tp, full_text, sensitive):
    """
    对每页：找出每个敏感项的所有出现位置（字符索引范围）并计算合并 bbox。
    返回 [ {'needle','mask','start','end','bbox':(l,b,r,t)}, ... ]
    """
    hits = []
    for needle, mask in sensitive:
        pos = 0
        while True:
            start = full_text.find(needle, pos)
            if start < 0:
                break
            end = start + len(needle)
            # 合并命中范围内每个字符的 bbox（跳过 \r\n 等零尺寸占位字符）
            xs, ys = [], []
            for i in range(start, end):
                b = tp.get_charbox(i)
                if b[2] > b[0] and b[3] > b[1]:  # 非零尺寸才纳入
                    xs.extend((b[0], b[2]))
                    ys.extend((b[1], b[3]))
            if xs and ys:
                hits.append({
                    "needle": needle,
                    "mask": mask,
                    "start": start,
                    "end": end,
                    "bbox": (min(xs), min(ys), max(xs), max(ys)),
                })
            pos = end  # 不重叠地继续向后找
    return hits

# ---------------------------------------------------------------------------
# 3. PDF 内容流解析（tokenizer + 文本块状态机）
# ---------------------------------------------------------------------------
def read_pdf_string(stream, i):
    """解析从 i 开始的 '(' 字符串，返回 (原始字节, 结束下标)。支持 \ddd 八进制与 \n 等转义。"""
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
        raise ValueError(f"内容流数组内出现意外字符: {chr(ch)} @{j}")
    raise ValueError("内容流数组未闭合")

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
        if ch >= 0x21 and ch <= 0x7E and not stream[i:i+1].isspace():
            # 操作符（字母或单个符号）
            j = i
            while j < n and stream[j] >= 0x21 and stream[j] <= 0x7E and stream[j] not in b"()[]/<>{}":
                if stream[j] in b"+-.0123456789":
                    break
                j += 1
            tokens.append(("op", stream[i:j].decode("latin-1")))
            i = j
            continue
        i += 1  # 其它字节跳过（一般不会出现）
    return tokens

def analyze_text_blocks(tokens):
    """
    在 token 流上模拟文本状态机（BT/Tf/Tm/Td/TL/Tj/TJ），
    返回文本块列表: {'op','str_tok_idx','op_tok_idx','strs','font','size','tx','ty'}
    """
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
            elif val == "Tm" and i >= 6:
                nums = [float(tokens[i - k][1]) for k in range(6, 0, -1)]
                state["tx"], state["ty"] = nums[4], nums[5]
            elif val in ("Td", "TD") and i >= 2:
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
        raise ValueError(f"未知 token: {tok}")
    return b" ".join(one(t) for t in tokens)

# ---------------------------------------------------------------------------
# 4. 深度脱敏：内容流级删除敏感 Tj/TJ 算子 + 白矩形覆盖 + 掩码重绘
# ---------------------------------------------------------------------------
def redact_page_stream(stream_bytes, hits):
    """
    返回 (新内容流字节, 处理的掩码绘制列表)。
    对每个文本块：
      - 将 Tj/TJ 字符串 UTF-16BE 解码
      - 块内出现的敏感子串【删除】（替换为空字符串，UTF-16BE 重新编码）
      - 若整块为 TJ 数组且被修改，则改写为单个字符串 Tj
    最后在内容流末尾追加：白矩形覆盖命中 bbox + 掩码文本重绘（掩码只出现一次）。
    """
    tokens = tokenize(stream_bytes)
    blocks = analyze_text_blocks(tokens)
    draws = []            # 需要白矩形覆盖 + 掩码重绘的命中（去重）
    mask_meta = {}        # id(hit) -> (font, size) 供掩码重绘使用

    for blk in blocks:
        text = "".join(s.decode("utf-16-be", "replace") for s in blk["strs"])
        new_text = text
        for needle, mask in SENSITIVE:
            if needle in new_text:
                new_text = new_text.replace(needle, "")  # 深度删除
                for hit in hits:
                    if hit["needle"] == needle and hit not in draws:
                        draws.append(hit)
                        mask_meta[id(hit)] = (blk["font"], blk["size"])
        if new_text == text:
            continue
        # 命中块：重写字符串 token（敏感字符已删除）
        enc = new_text.encode("utf-16-be")
        tokens[blk["str_tok_idx"]] = ("str", enc)
        if blk["op"] == "TJ":
            tokens[blk["op_tok_idx"]] = ("op", "Tj")

    # 追加：白矩形覆盖 + 掩码文本重绘（先画矩形，再写掩码，保证掩码在上层可见）
    suffix = []
    for hit in draws:
        l, b, r, t = hit["bbox"]
        pad = 0.5
        x, y, w, h = l - pad, b - pad, (r - l) + 2 * pad, (t - b) + 2 * pad
        suffix.append(f"q 1 1 1 rg {x:.2f} {y:.2f} {w:.2f} {h:.2f} re f Q".encode("ascii"))
    for hit in draws:
        l, b, _, _ = hit["bbox"]
        mask_bytes = hit["mask"].encode("utf-16-be")
        font, size = mask_meta.get(id(hit), ("F2", 14))
        suffix.append(
            f"BT /{font} {size:.1f} Tf 1 0 0 1 {l:.2f} {b:.2f} Tm ("
            .encode("ascii") + escape_pdf_bytes(mask_bytes) + b") Tj ET"
        )

    new_stream = serialize(tokens)
    if suffix:
        new_stream += b"\n" + b"\n".join(suffix)
    return new_stream, draws

def redact_pdf(src_path, dst_path):
    """逐页读取内容流 -> 定位命中 -> 深度编辑 -> 写回。返回耗时与统计。"""
    t0 = time.perf_counter()
    # ---- 第一步：pypdfium2 定位每页命中 ----
    pdf_src = pdfium.PdfDocument(src_path)
    per_page_hits = []
    try:
        for pno in range(len(pdf_src)):
            page = pdf_src[pno]
            tp = page.get_textpage()
            try:
                full = tp.get_text_bounded()
                hits = locate_hits(tp, full, SENSITIVE)
                per_page_hits.append(hits)
                print(f"[locate] page {pno}: hits={hits}")
            finally:
                tp.close()
                page.close()
    finally:
        pdf_src.close()

    # ---- 第二步：pikepdf 内容流编辑 ----
    total_draws = 0
    with pikepdf.open(src_path) as pdf:
        for pno, page in enumerate(pdf.pages):
            hits = per_page_hits[pno]
            if not hits:
                continue
            if isinstance(page.Contents, pikepdf.Array):
                stream_bytes = b"\n".join(s.read_bytes() for s in page.Contents)
            else:
                stream_bytes = page.Contents.read_bytes()
            new_stream, draws = redact_page_stream(stream_bytes, hits)
            total_draws += len(draws)
            page.Contents = pikepdf.Stream(pdf, new_stream)
            print(f"[edit] page {pno}: 重写内容流 {len(stream_bytes)}B -> {len(new_stream)}B, 命中 {len(draws)} 处")
        pdf.save(dst_path)
    elapsed = time.perf_counter() - t0
    print(f"[save] 已保存脱敏 PDF: {dst_path}  (总耗时 {elapsed*1000:.1f} ms)")
    return elapsed, per_page_hits, total_draws

# ---------------------------------------------------------------------------
# 5. 验证：重新提取文本 + 断言 + 渲染对比图
# ---------------------------------------------------------------------------
def verify(dst_path):
    texts = extract_page_texts(dst_path)
    all_text = "\n".join(t for _, t in texts)
    with open(TXT_AFTER, "w", encoding="utf-8") as f:
        f.write(all_text)

    results = {}
    for needle, mask in SENSITIVE:
        results[f"原敏感文本已删除:{needle}"] = needle not in all_text
    for needle, mask in SENSITIVE:
        results[f"掩码文本可见:{mask}"] = mask in all_text
    for keep in ["标题：个人信息登记表", "姓名：", "手机号：", "住址：北京市朝阳区示例路 88 号", "备注：无", "第二页：普通内容，无敏感信息"]:
        results[f"版面保留:{keep}"] = keep in all_text

    all_ok = True
    for name, ok in results.items():
        print(f"  [verify] {'PASS' if ok else 'FAIL'}  {name}")
        all_ok = all_ok and ok
    return all_ok, texts

def render_pngs():
    """渲染 before/after 对比图（人工目检佐证）"""
    for src, dst in ((SRC_PDF, "before_page0.png"), (SRC_PDF, "before_page1.png"),
                     (REDACTED_PDF, "after_page0.png"), (REDACTED_PDF, "after_page1.png")):
        pno = int(dst[-5])
        pdf = pdfium.PdfDocument(src)
        try:
            page = pdf[pno]
            img = page.render(scale=1.5).to_pil()
            out_path = os.path.join(OUT, dst)
            img.save(out_path)
            print(f"[render] {out_path}")
            page.close()
        finally:
            pdf.close()

# ---------------------------------------------------------------------------
# main
# ---------------------------------------------------------------------------
def main():
    os.makedirs(OUT, exist_ok=True)

    print("=" * 60)
    print("STEP 1: 生成测试 PDF")
    print("=" * 60)
    gen_test_pdf(SRC_PDF)

    print("\n" + "=" * 60)
    print("STEP 2: 脱敏前文本提取（before.txt）")
    print("=" * 60)
    before = extract_page_texts(SRC_PDF)
    with open(TXT_BEFORE, "w", encoding="utf-8") as f:
        f.write("\n".join(t for _, t in before))
    for pno, t in before:
        print(f"--- page {pno} ---\n{t}")

    print("\n" + "=" * 60)
    print("STEP 3: 深度脱敏（内容流编辑）")
    print("=" * 60)
    elapsed, per_page_hits, total_draws = redact_pdf(SRC_PDF, REDACTED_PDF)
    n_pages = len(before)
    print(f"[perf] 处理 {n_pages} 页，共耗时 {elapsed*1000:.1f} ms，平均每页 {elapsed/n_pages*1000:.1f} ms")

    print("\n" + "=" * 60)
    print("STEP 4: 验证（after.txt）")
    print("=" * 60)
    ok, after_texts = verify(REDACTED_PDF)
    for pno, t in after_texts:
        print(f"--- page {pno} ---\n{t}")

    print("\n" + "=" * 60)
    print("STEP 5: 渲染对比图")
    print("=" * 60)
    render_pngs()

    print("\n" + "=" * 60)
    print(f"结论: {'全部断言通过 —— 深度脱敏可行' if ok else '存在失败断言 —— 请检查上方 FAIL 项'}")
    print("=" * 60)
    return 0 if ok else 1

if __name__ == "__main__":
    sys.exit(main())
