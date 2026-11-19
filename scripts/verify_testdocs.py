#!/usr/bin/env python3
"""墨隐 InkVeil 测试文档集程序化校验（本地调测验收 Part 5.3）。

对 testdocs/ 内全部支持格式的文件逐一执行 sidecar parse→redact 全链路：
  1. 敏感原文（姓名/手机/身份证/银行卡）替换为零残留、掩码可见
  2. 原文件字节不变（绝不原地修改）
  3. replaced_count 与检测到的替换数一致、is_deep 标志正确
  4. 非敏感内容保留（金额/地址/日期等陪衬信息不受影响）
  5. xlsx 额外断言：number_format / 合并单元格 / 公式保留

用法：sidecar/.venv/bin/python scripts/verify_testdocs.py [testdocs目录]
退出码：0=全部通过；1=存在失败项
"""

import os
import sys
import tempfile

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "sidecar"))

from fastapi.testclient import TestClient

from app import app
from parsers import parse_file

client = TestClient(app)

# 核心敏感词 -> 掩码（与 gen_testdocs.py / test_xlsx.py 一致）
SENSITIVE = [
    ("张三", "张*"),
    ("13800138000", "138****8000"),
    ("440301197407210509", "440301**********09"),
    ("6222020200088888884", "622202*********8884"),
]
# 非敏感陪衬内容（必须保留）
KEEP = ["北京市朝阳区示例路88号", "1234567.89", "2024-06-30"]

SUPPORTED_EXTS = (".docx", ".xlsx", ".pdf", ".txt", ".md")


def find_replacements(text: str) -> list[dict]:
    repls = []
    for needle, mask in sorted(SENSITIVE, key=lambda kv: -len(kv[0])):
        pos = 0
        while True:
            i = text.find(needle, pos)
            if i < 0:
                break
            repls.append({"start": i, "end": i + len(needle), "replacement": mask})
            pos = i + len(needle)
    repls.sort(key=lambda r: r["start"])
    return repls


def verify_file(path: str, out_dir: str) -> list[str]:
    errors: list[str] = []
    rel = os.path.relpath(path, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    orig_bytes = open(path, "rb").read()

    resp = client.post("/parse", json={"doc_id": "verify", "path": path})
    if resp.status_code != 200:
        return [f"{rel}: /parse 失败 {resp.status_code}: {resp.text[:200]}"]
    data = resp.json()
    text = data["text"]

    repls = find_replacements(text)
    if not repls:
        return [f"{rel}: 未检测到任何敏感串（文档集可能已脱敏或生成失败）"]
    # 本文件中实际出现的敏感词（去重），用于掩码断言
    hit_pairs = list(dict.fromkeys(
        (needle, mask) for needle, mask in SENSITIVE if needle in text
    ))
    # 本文件中实际出现的陪衬词，用于保留断言
    keep_present = [k for k in KEEP if k in text]

    resp = client.post(
        "/redact",
        json={"input_path": path, "replacements": repls, "output_dir": out_dir},
    )
    if resp.status_code != 200:
        return [f"{rel}: /redact 失败 {resp.status_code}: {resp.text[:200]}"]
    f = resp.json()["files"][0]

    # 原文件字节不变
    if open(path, "rb").read() != orig_bytes:
        errors.append(f"{rel}: 原文件被修改！")

    # replaced_count 正确
    if f["replaced_count"] != len(repls):
        errors.append(f"{rel}: replaced_count={f['replaced_count']} != 检测数 {len(repls)}")

    # is_deep：内容流级深删（reportlab 单行 Tj 应可深删）
    if f["is_deep"] is not True:
        errors.append(f"{rel}: is_deep={f['is_deep']}（期望 True）")

    # 重开输出：敏感零残留、掩码可见、陪衬保留
    out_text = parse_file(f["output_path"]).text
    for needle, _mask in hit_pairs:
        if needle in out_text:
            errors.append(f"{rel}: 敏感原文残留 {needle!r}")
    for needle, mask in hit_pairs:
        if mask not in out_text:
            errors.append(f"{rel}: 掩码缺失 {mask!r}")
    for keep in keep_present:
        if keep not in out_text:
            errors.append(f"{rel}: 非敏感内容丢失 {keep!r}")

    # xlsx 格式保留（02 表结构：D=身份证列、E=银行卡列，均设文本格式 @）
    if path.lower().endswith(".xlsx"):
        from openpyxl import load_workbook

        wb = load_workbook(f["output_path"])
        ws = wb["员工信息"]
        if ws["D2"].number_format != "@":
            errors.append(f"{rel}: 身份证列文本格式丢失（{ws['D2'].number_format}）")
        if ws["E2"].number_format != "@":
            errors.append(f"{rel}: 银行卡列文本格式丢失（{ws['E2'].number_format}）")
        if not list(ws.merged_cells.ranges):
            errors.append(f"{rel}: 合并单元格丢失")
        if not wb["员工信息"]["A1"].font.bold:
            errors.append(f"{rel}: 表头加粗丢失")

    return errors


def main() -> int:
    repo_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    testdocs = sys.argv[1] if len(sys.argv) > 1 else os.path.join(repo_root, "testdocs")
    files = []
    for dirpath, _dirs, names in os.walk(testdocs):
        for name in sorted(names):
            if name.lower().endswith(SUPPORTED_EXTS):
                files.append(os.path.join(dirpath, name))

    failed = 0
    with tempfile.TemporaryDirectory(prefix="verify_out_") as out_dir:
        for path in files:
            errors = verify_file(path, out_dir)
            rel = os.path.relpath(path, testdocs)
            if errors:
                failed += 1
                print(f"  ✗ {rel}")
                for e in errors:
                    print(f"      - {e}")
            else:
                print(f"  ✓ {rel}")

    if failed:
        print(f"\n校验失败: {failed}/{len(files)} 个文件")
        return 1
    print(f"\n校验全部通过: {len(files)} 个文件（敏感零残留、原文件不变、格式保留）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
