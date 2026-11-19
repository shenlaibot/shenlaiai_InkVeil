#!/usr/bin/env python3
"""墨隐 InkVeil 性能基准（sidecar 端到端）。

对照 spec 性能目标：
  1) 批量：8 个 20 页 PDF 并行脱敏 < 20s（解析 + 重写；规则检测耗时由 Rust
     bench_scan 单独覆盖，此处用固定敏感串构造替换）。
  2) NER：约 10 万字合同 < 10s（模型已发布时实测；未发布则记录 skip）。

以真实 HTTP 形式调用 sidecar（TestClient ASGI 内存调用，等价真实链路且零外传）。
用法：
  python bench.py --pdf-dir <dir> [--count 8] [--pages 20]   # 批量 + NER
  python bench.py --ner-only                                  # 只跑 NER
"""

import argparse
import os
import sys
import time
from concurrent.futures import ThreadPoolExecutor

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
SIDECAR = os.path.join(ROOT, "sidecar")
sys.path.insert(0, SIDECAR)

SENSITIVE = [
    ("13800138000", "138****8000"),
    ("110101198403201234", "110101**********34"),
    ("6222020200088888884", "622202*********8884"),
    ("contact@example.com", "con***@example.com"),
]


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


def _ner_sample_text() -> str:
    """构造约 10 万字合同文本（含人名/机构/地址，逼近真实 100 页合同密度）。"""
    chunk = (
        "乙方北京星河科技有限公司（以下简称乙方）与甲方上海晨光律师事务所就"
        "陈晓明先生提出的知识产权纠纷一案达成如下协议。北京市朝阳区建国路88号"
        "为本协议签署地。2024年6月30日双方在深圳福田区确认补充条款。"
    )
    # 每块约 110 字，900 块 ≈ 10 万字
    return chunk * 900


def _ner_bench(client) -> int:
    print("== NER 约 10 万字合同（门槛 10s）==")
    ner_body = _ner_sample_text()
    t0 = time.perf_counter()
    try:
        r = client.post("/ner", json={"text": ner_body, "threshold": 0.5})
        data = r.json()
    except Exception as e:  # noqa: BLE001
        print(f"ner: 请求异常 {e!r}")
        data = {"ok": False, "error": repr(e)}
    ner_elapsed = time.perf_counter() - t0

    if data.get("ok") and data.get("model_loaded"):
        print(f"ner: entities={len(data['entities'])} 耗时={ner_elapsed:.2f}s")
        if ner_elapsed > 10.0:
            print("ner: FAIL（超出 10s 门槛）")
            return 1
    else:
        print(f"ner: SKIP（模型未发布: {data.get('error')}）")
    return 0


def _batch_bench(client, args) -> int:
    print("== 批量 PDF 并行脱敏（%d × %d 页，门槛 20s）==" % (args.count, args.pages))
    pdfs = [os.path.join(args.pdf_dir, f"bench-{i}.pdf") for i in range(args.count)]
    for p in pdfs:
        if not os.path.isfile(p):
            print(f"缺失样本: {p}（先运行 gen_samples.py pdfs）")
            return 2
    t0 = time.perf_counter()

    def run_one(pdf: str) -> tuple[int, int]:
        pr = client.post("/parse", json={"path": pdf}).json()
        repls = find_replacements(pr["text"])
        rr = client.post(
            "/redact",
            json={"input_path": pdf, "replacements": repls, "output_dir": args.pdf_dir + "_out"},
        )
        f = rr.json()["files"][0]
        return f["replaced_count"], f["is_deep"]

    with ThreadPoolExecutor(max_workers=args.count) as ex:
        results = list(ex.map(run_one, pdfs))
    batch_elapsed = time.perf_counter() - t0
    total_replaced = sum(r[0] for r in results)
    print(
        f"batch: {args.count} 文件 并行耗时={batch_elapsed:.2f}s "
        f"总替换={total_replaced} 深度脱敏={sum(1 for _, d in results if d)}/{args.count}"
    )
    if batch_elapsed > 20.0:
        print("batch: FAIL（超出 20s 门槛）")
        return 1
    return 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--pdf-dir")
    ap.add_argument("--count", type=int, default=8)
    ap.add_argument("--pages", type=int, default=20)
    ap.add_argument("--ner-only", action="store_true", help="只跑 NER 基准")
    args = ap.parse_args()

    from fastapi.testclient import TestClient

    from app import app

    client = TestClient(app)

    if args.ner_only:
        return _ner_bench(client)
    rc = _batch_bench(client, args)
    if rc:
        return rc
    return _ner_bench(client)


if __name__ == "__main__":
    sys.exit(main())
