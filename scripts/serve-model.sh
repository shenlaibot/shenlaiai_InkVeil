#!/usr/bin/env bash
# 本地模型下载自测：用 HTTP 服务托管本地 INT8 NER 模型目录，
# 供应用设置 ANK_MODEL_BASE_URL 指向本服务，验证「下载→校验→安装→NER 生效」全链路。
#
# 用法：
#   bash scripts/serve-model.sh [端口]     # 默认 8765
# 然后启动应用：
#   ANK_MODEL_BASE_URL=http://127.0.0.1:8765 npm run tauri dev
set -euo pipefail

cd "$(dirname "$0")/.."

PORT="${1:-8765}"
DIR="sidecar/models/ner/int8"
if [ ! -f "$DIR/model.onnx" ]; then
  echo "未找到本地模型目录: $DIR（请先准备 INT8 模型文件）" >&2
  exit 1
fi

echo "== 本地模型服务: http://127.0.0.1:${PORT}  <-  $DIR =="
echo "启动应用时请设置: ANK_MODEL_BASE_URL=http://127.0.0.1:${PORT}"
exec python3 -m http.server "$PORT" --directory "$DIR" --bind 127.0.0.1
