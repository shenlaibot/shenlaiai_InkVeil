#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""补充验证：内容流使用 TJ 数组（真实 PDF 常见写法）时，脱敏逻辑是否仍然有效"""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import test_redact as tr
import pikepdf

TJ_PDF = os.path.join(tr.OUT, "sample_tj.pdf")
TJ_AFTER = os.path.join(tr.OUT, "sample_tj_after.pdf")

# ---- 1) 把第一页"手机号：13800138000"那行改写为 TJ 数组形式 ----
with pikepdf.open(tr.SRC_PDF) as pdf:
    page = pdf.pages[0]
    data = page.Contents.read_bytes()
    tokens = tr.tokenize(data)
    blocks = tr.analyze_text_blocks(tokens)
    done = False
    for blk in blocks:
        text = "".join(s.decode("utf-16-be", "replace") for s in blk["strs"])
        if "13800138000" in text:
            prefix = text[: text.find("13800138000")]
            pref_bytes = prefix.encode("utf-16-be")
            arr = [("str", pref_bytes[i : i + 2]) for i in range(0, len(pref_bytes), 2)]
            arr.append(("num", b"-30"))
            arr.append(("str", "13800138000".encode("utf-16-be")))
            tokens[blk["str_tok_idx"]] = ("array", arr)
            tokens[blk["op_tok_idx"]] = ("op", "TJ")
            done = True
            break
    assert done, "未找到含敏感文本的块"
    page.Contents = pikepdf.Stream(pdf, tr.serialize(tokens))
    pdf.save(TJ_PDF)
print("[tj] 已生成 TJ 数组版:", TJ_PDF)

# ---- 2) 用同一套脱敏逻辑处理 TJ 版 ----
elapsed, per_page_hits, total_draws = tr.redact_pdf(TJ_PDF, TJ_AFTER)
print(f"[tj] 脱敏耗时 {elapsed*1000:.1f} ms, 命中 {total_draws} 处")

# ---- 3) 验证 ----
texts = tr.extract_page_texts(TJ_AFTER)
all_text = "\n".join(t for _, t in texts)
print("--- after(TJ) 文本 ---")
for pno, t in texts:
    print(f"page {pno}:\n{t}")
ok = True
for k in ["13800138000", "张三", "110101198403201234"]:
    r = k not in all_text
    print(f"  [{'PASS' if r else 'FAIL'}] 原敏感文本已删除: {k}")
    ok = ok and r
for m in ["138****8000", "张*", "110101**********34"]:
    r = m in all_text
    print(f"  [{'PASS' if r else 'FAIL'}] 掩码文本可见: {m}")
    ok = ok and r
for keep in ["姓名：", "手机号：", "住址：北京市朝阳区示例路 88 号"]:
    r = keep in all_text
    print(f"  [{'PASS' if r else 'FAIL'}] 版面保留: {keep}")
    ok = ok and r
print("TJ 数组路径结论:", "可行" if ok else "失败")
