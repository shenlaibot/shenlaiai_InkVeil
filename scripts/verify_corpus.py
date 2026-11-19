#!/usr/bin/env python3
"""复杂语料端到端判定。

用法: verify_corpus.py <corpus_dir> <manifest.json> <restored_dir>
sidecar 模块路径通过脚本相对位置解析（sidecar/）。

判定项（失败则退出码 1）：
1. 残留：truth 里 detectable=True 的敏感值不得出现在输出文件的任何可提取部位（复检器）；
2. 作者/属性类(kind=author)：不得残留（属性、批注作者等必须被清理）；
3. 输出可打开：docx/xlsx/pdf 能被对应库重新读取；
4. 非敏感内容保留：docx 标题、pdf 可检索的普通条款等；
5. 还原：已被替换的敏感值能通过映射表还原回原文。
"""
import json
import os
import sys
from collections import defaultdict

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, "sidecar"))

from verifier import find_residue  # noqa: E402
from parsers import detect_format, parse_file  # noqa: E402


def main():
    corpus, manifest_path, restored_dir = sys.argv[1:4]
    truth = json.load(open(os.path.join(corpus, "truth.json"), encoding="utf-8"))
    manifest = {m["file"]: m for m in json.load(open(manifest_path, encoding="utf-8"))}
    by_file = defaultdict(list)
    for t in truth:
        by_file[t["file"]].append(t)

    fails = []          # (类别, 文件, 说明)
    gaps = []           # 已知盲区（不判失败）
    stats = defaultdict(int)

    for fn, items in sorted(by_file.items()):
        m = manifest.get(fn)
        if not m or not m.get("output"):
            fails.append(("无输出", fn, (m or {}).get("error") or "未产出脱敏文件"))
            continue
        out = m["output"]
        fmt = detect_format(out)
        # 1/2 残留
        secrets = list(dict.fromkeys(t["secret"] for t in items if t["detectable"] or t["kind"] == "author"))
        res = find_residue(out, fmt, secrets) if secrets else []
        residual_idx = {r["index"] for r in res}
        where = defaultdict(set)
        for r in res:
            where[r["index"]].add(r["where"])
        # find_residue 内部会去重/过滤短串，重建索引
        needles = [n for n in dict.fromkeys(secrets) if n and len(n) >= 2]
        kind_of = {}
        for t in items:
            kind_of.setdefault(t["secret"], t["kind"])
        for i in sorted(residual_idx):
            s = needles[i]
            fails.append(("残留", fn, f"[{kind_of.get(s)}] {s}  位置: {sorted(where[i])[:3]}"))
        for t in items:
            stats["detectable_total"] += 1 if t["detectable"] else 0
        # 盲区统计（不判失败）
        gap_secrets = [t["secret"] for t in items if not t["detectable"] and t["kind"] != "author"]
        if gap_secrets:
            r2 = find_residue(out, fmt, list(dict.fromkeys(gap_secrets)))
            if r2:
                gaps.append((fn, len(r2)))
        # 3 可打开 / 4 保留非敏感内容
        try:
            if fmt == "docx" and fn.startswith("contract_complex"):
                from docx import Document
                d = Document(out)
                if not any("技术服务合同" in p.text for p in d.paragraphs):
                    fails.append(("内容丢失", fn, "标题“技术服务合同”丢失"))
            elif fmt == "xlsx" and fn == "roster_complex.xlsx":
                from openpyxl import load_workbook
                wb = load_workbook(out)
                if "明细" not in wb.sheetnames:
                    fails.append(("内容丢失", fn, "工作表丢失"))
                import zipfile
                with zipfile.ZipFile(out) as z:
                    if not any(n.startswith("xl/charts/") for n in z.namelist()):
                        fails.append(("内容丢失", fn, "图表丢失"))
            elif fmt == "pdf":
                import pypdfium2 as pdfium
                doc = pdfium.PdfDocument(out)
                try:
                    texts = [doc[i].get_textpage().get_text_bounded() for i in range(len(doc))]
                finally:
                    doc.close()
                allt = "\n".join(texts)
                if fn == "contract_cjk.pdf":
                    if "普通条款" not in allt or "没有任何敏感信息" not in allt:
                        fails.append(("内容丢失", fn, "非敏感文字不可检索"))
                elif fn == "annots.pdf":
                    if "Plain page" not in allt:
                        fails.append(("内容丢失", fn, "非敏感文字不可检索"))
                else:
                    if "Ordinary clause" not in allt:
                        fails.append(("内容丢失", fn, "非敏感文字不可检索"))
        except Exception as e:  # noqa: BLE001
            fails.append(("无法打开", fn, repr(e)))
        # 5 还原
        rp = os.path.join(restored_dir, fn + ".txt")
        if os.path.isfile(rp):
            back = open(rp, encoding="utf-8").read()
            # 只核对“原始解析全文里本来就有”的敏感值（属性/自定义 XML 等被直接清理的值不在全文里）
            parsed = parse_file(os.path.join(corpus, fn))
            # PDF 的注释/链接文字接在全文末尾；脱敏时未栅格化页上含原文的注释会被直接删除（不是占位符化），
            # 因此还原核对只看页面正文
            orig_text = "\n".join(parsed.page_texts) if fmt == "pdf" and parsed.page_texts else parsed.text
            # 逐次出现核对：还原后的出现次数不得少于原文（只看“是否出现过”会掩盖部分位置没还原的问题）
            # 简称(company_alias)与全称共用占位符，还原为全称，不按字面核对
            lost = [t["secret"] for t in items
                    if t["detectable"] and t["kind"] != "company_alias" and t["secret"] in orig_text
                    and back.count(t["secret"]) < orig_text.count(t["secret"])]
            # 还原后不得残留占位符（占位符被还原函数漏掉，如紧贴汉字时）
            import re as _re
            left = [x for x in _re.findall(r"\[[\u4e00-\u9fa5A-Za-z]+\d+\]", back) if x not in orig_text]
            if left:
                fails.append(("还原", fn, f"还原后仍残留占位符 {len(left)} 处，例: {left[:3]}"))
            if lost:
                stats["restore_missing"] += len(lost)
                fails.append(("还原", fn, f"{len(lost)} 个值未能还原，例: {lost[:3]}"))

    # 异常文件：必须“不崩溃、不产出半成品”，并给出错误或警告
    for fn in ("truncated.docx", "garbage.xlsx"):
        m = manifest.get(fn)
        if m is None:
            fails.append(("容错", fn, "没有处理记录"))
        elif m.get("output"):
            fails.append(("容错", fn, f"损坏文件却产出了输出: {m['output']}"))
        elif not (m.get("error") or m.get("warnings")):
            fails.append(("容错", fn, "损坏文件没有给出错误或警告"))
    for fn in ("empty.txt", "empty.docx", "scan_like.pdf"):
        m = manifest.get(fn)
        if m is None:
            fails.append(("容错", fn, "没有处理记录"))
        elif m.get("output"):
            fails.append(("容错", fn, "无内容文件不应产出脱敏副本"))

    print("=" * 70)
    print(f"文件数: {len(by_file)}  标准答案: {len(truth)}  可检出项: {stats['detectable_total']}")
    for fn, m in sorted(manifest.items()):
        print(f"  {fn:28s} 命中{m['hit_count']:3d} 未落位{m['unplaced']} deep={m['is_deep']} 警告{len(m['warnings'])} 错误={m['error']}")
    if gaps:
        print("已知盲区仍存在（不判失败）:", gaps)
    by_cat = defaultdict(list)
    for c, f, d in fails:
        by_cat[c].append((f, d))
    if fails:
        print(f"\n失败 {len(fails)} 项:")
        for c, lst in by_cat.items():
            print(f"\n[{c}] {len(lst)} 项")
            for f, d in lst[:60]:
                print(f"  - {f}: {d}")
        sys.exit(1)
    print("\n全部通过")


if __name__ == "__main__":
    main()
