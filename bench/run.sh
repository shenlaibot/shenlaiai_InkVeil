#!/usr/bin/env bash
# 墨隐 InkVeil 性能基准（spec 性能目标验收）。
#
# 覆盖：
#   1) 规则扫描：10MB 纯文本 < 1s（Rust，bench_scan example）
#   2) 批量脱敏：8 × 20 页 PDF 并行 < 20s（sidecar，解析 + 写回）
#   3) NER：约 10 万字合同 < 10s（模型已发布时实测，未发布记录 skip）
#   4) 冷启动/内存：依赖打包产物手动验收（本脚本不测）
#
# 用法：bash bench/run.sh [--release]
set -euo pipefail

cd "$(dirname "$0")/.."

PY="${PY:-sidecar/.venv/bin/python}"
if [ ! -x "$PY" ]; then
  PY=python3
fi

OUT="bench/results"
mkdir -p "$OUT"
STAMP="$(date +%Y%m%d-%H%M%S)"
RESULT="$OUT/bench-$STAMP.txt"
TMP="bench/.tmp"
rm -rf "$TMP"; mkdir -p "$TMP"

echo "墨隐 InkVeil 性能基准 @ $(date '+%Y-%m-%d %H:%M:%S')" | tee "$RESULT"
echo "平台: $(uname -sm) / $(sw_vers -productVersion 2>/dev/null || true)" | tee -a "$RESULT"

# ---- 1) 规则扫描 ----
echo | tee -a "$RESULT"
echo "== [1/4] 规则扫描：10MB 文本 < 1s ==" | tee -a "$RESULT"
"$PY" bench/gen_samples.py text "$TMP/scan-10mb.txt" 10 >/dev/null
if cargo run -q --release --example bench_scan -- "$TMP/scan-10mb.txt" 2>&1 | tee -a "$RESULT"; then
  :
else
  echo "规则扫描未达门槛" | tee -a "$RESULT"
fi

# ---- 2) 批量 PDF ----
echo | tee -a "$RESULT"
echo "== [2/4] 批量脱敏：8 × 20 页 PDF < 20s ==" | tee -a "$RESULT"
"$PY" bench/gen_samples.py pdfs "$TMP/pdfs" 8 20 >/dev/null
"$PY" bench/bench.py --pdf-dir "$TMP/pdfs" --count 8 --pages 20 2>&1 | tee -a "$RESULT"

# ---- 3) NER ----
echo | tee -a "$RESULT"
echo "== [3/4] NER：约 10 万字合同 < 10s（模型未发布则 skip）==" | tee -a "$RESULT"
"$PY" bench/bench.py --ner-only 2>&1 | tee -a "$RESULT"

# ---- 4) 性能门槛对照 ----
echo | tee -a "$RESULT"
echo "== [4/4] 门槛对照 ==" | tee -a "$RESULT"
grep -E 'scan: |batch: |ner:' "$RESULT" | tee -a "$RESULT" || true

rm -rf "$TMP"
echo | tee -a "$RESULT"
echo "结果已写入: $RESULT" | tee -a "$RESULT"
