#!/usr/bin/env bash
# 将 Python sidecar（FastAPI）打包为独立二进制，产出 Tauri externalBin。
#
# 产物：src-tauri/binaries/anonkit-sidecar-<host-triple>（Windows 为 .exe），
# 供 tauri.conf.json 的 `externalBin` 随包分发；应用发布期由
# src-tauri/src/sidecar.rs 直接执行该二进制（不再依赖系统 Python）。
#
# 用法：
#   bash scripts/build-sidecar.sh            # 默认 --onefile（现行 externalBin 方案）
#   bash scripts/build-sidecar.sh --onedir   # 变体：目录模式，产出 binaries/anonkit-sidecar-<triple>-onedir/
#                                            #（启动更快、杀软误报更少；需 Tauri/Rust 侧配合，见 docs/release.md）
#
# 依赖：一律从带哈希的锁文件 sidecar/requirements-build.lock 安装（pip --require-hashes），
# PyInstaller 版本由该锁文件固定。锁文件目标为 Python 3.11；本地 venv 版本不符时会拒绝，
# 可用 ALLOW_NONLOCK_PYTHON=1 / SKIP_LOCK_INSTALL=1 跳过（仅限本地调试，发布构建禁止）。
set -euo pipefail

MODE="onefile"
case "${1:-}" in
  "" ) ;;
  --onefile) MODE="onefile" ;;
  --onedir) MODE="onedir" ;;
  *) echo "未知参数: $1（可选 --onefile | --onedir）" >&2; exit 2 ;;
esac

cd "$(dirname "$0")/.."

TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
PY="${SIDECAR_PY:-}"   # 可用环境变量指定 Python（例如 CI 外的本地 3.11 venv）
for candidate in "sidecar/.venv/bin/python" "sidecar/.venv/Scripts/python.exe"; do
  if [ -z "$PY" ] && [ -x "$candidate" ]; then PY="$candidate"; break; fi
done
if [ -z "$PY" ]; then
  echo "未找到 sidecar venv Python，请先创建 sidecar venv 并安装依赖（见 sidecar/requirements*.txt）" >&2
  exit 1
fi

WORK="bench/.tmp/pyinstaller"
rm -rf "$WORK"
mkdir -p "$WORK"

echo "== 安装依赖（锁文件 + 哈希校验）=="
PYVER="$("$PY" -c 'import sys; print("%d.%d" % sys.version_info[:2])')"
if [ "${SKIP_LOCK_INSTALL:-0}" = 1 ]; then
  echo "  ! SKIP_LOCK_INSTALL=1：跳过锁文件安装（仅本地调试）"
else
  if [ "$PYVER" != "3.11" ] && [ "${ALLOW_NONLOCK_PYTHON:-0}" != 1 ]; then
    echo "锁文件目标为 Python 3.11，当前 venv 为 ${PYVER}。请用 3.11 重建 venv，或设置 SKIP_LOCK_INSTALL=1 仅做本地调试。" >&2
    exit 1
  fi
  "$PY" -m pip install -q --require-hashes -r sidecar/requirements-build.lock
fi

echo "== 收集 python-docx 包数据 =="
# python-docx 的 parts/*.py 通过 os.path.join(__file__, "..", "templates", ...) 读取模板。
# PyInstaller 把纯 Python 模块收进 PYZ 后，磁盘 docx/ 下不存在 parts 目录，
# 导致 '../templates/default-header.xml' 这种路径解析失败（FileNotFoundError）。
# 解法：把整个 docx 包目录作为 data 打进 _MEIPASS/docx/，保证 parts/ 目录真实存在。
DOCX_DIR="$("$PY" -c 'import docx, os; print(os.path.dirname(docx.__file__))')"
DATA_SEP="$("$PY" -c 'import os; print(os.pathsep)')"

echo "== 打包 sidecar（target: ${TRIPLE}）=="
# 收集运行时二进制扩展（onnxruntime / pypdfium2 / pikepdf 均为 C 扩展），
# 防止 PyInstaller 静态分析漏检；模型文件不打包（发布期由模型市场下载到应用数据目录）。
# Windows 上 --noconsole 隐藏控制台窗口（sidecar 是后台服务，不应弹黑窗）；
# macOS/Linux 上该参数无副作用（忽略）。
NOCONSOLE_FLAG="--noconsole"
"$PY" -m PyInstaller \
  --"${MODE}" \
  $NOCONSOLE_FLAG \
  --name anonkit-sidecar \
  --distpath "$WORK/dist" \
  --workpath "$WORK/build" \
  --specpath "$WORK" \
  --collect-all onnxruntime \
  --collect-all pypdfium2 \
  --collect-all pikepdf \
  --collect-all openpyxl \
  --collect-all tokenizers \
  --add-data "${DOCX_DIR}${DATA_SEP}docx" \
  --noconfirm \
  --log-level WARN \
  sidecar/app.py

# onefile：dist/anonkit-sidecar；onedir：dist/anonkit-sidecar/anonkit-sidecar（旁边是 _internal/）
if [ "$MODE" = onedir ]; then
  EXE="$WORK/dist/anonkit-sidecar/anonkit-sidecar"
else
  EXE="$WORK/dist/anonkit-sidecar"
fi
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*) EXE="$EXE.exe" ;;
esac

echo "== 校验产物可启动（/health 自检）=="
PORT="$("$PY" -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
# sidecar 默认拒绝无 token 启动（安全加固），自检用一次性随机 token，所有请求带 Bearer 头。
TOKEN="$("$PY" -c 'import secrets; print(secrets.token_hex(16))')"
AUTH_HEADER="Authorization: Bearer ${TOKEN}"
"$EXE" --host 127.0.0.1 --port "$PORT" --token "$TOKEN" >"$WORK/sidecar.log" 2>&1 &
PID=$!
ok=0
for _ in $(seq 1 60); do
  if curl -sf -H "$AUTH_HEADER" "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; then ok=1; break; fi
  sleep 0.5
done
if [ "$ok" != 1 ]; then
  echo "sidecar 产物健康检查失败，启动日志：" >&2
  tail -n 30 "$WORK/sidecar.log" >&2
  exit 1
fi
echo "  ✓ /health OK"

# 回归检查：用真实 docx 走一遍 /parse，确保 python-docx 模板数据已打进包
SAMPLE="$(pwd)/testdocs/01-房屋租赁合同.docx"
if [ ! -f "$SAMPLE" ] && [ -f "$(pwd)/sidecar/tests/samples/sample.docx" ]; then
  SAMPLE="$(pwd)/sidecar/tests/samples/sample.docx"
fi
if [ -f "$SAMPLE" ]; then
  # Windows Git Bash 的 $(pwd) 返回 /d/a/...（MSYS 风格），Windows 原生 Python 打不开；
  # 用 cygpath 转成 Windows 原生路径（macOS/Linux 无 cygpath，保持原样）。
  if command -v cygpath >/dev/null 2>&1; then
    SAMPLE="$(cygpath -w "$SAMPLE")"
  fi
  # 用 json.dumps 构造请求体：Windows 路径含反斜杠（C:\Users\...），
  # 手写 JSON 字符串会产生非法转义序列（\U、\r、\u），Python 端 json.loads 会抛错。
  PAYLOAD="$("$PY" -c 'import json,sys; print(json.dumps({"doc_id":"bvt","path":sys.argv[1]}))' "$SAMPLE")"
  if curl -sf -X POST "http://127.0.0.1:$PORT/parse" \
      -H "$AUTH_HEADER" \
      -H 'Content-Type: application/json' \
      -d "$PAYLOAD" \
      | "$PY" -c 'import sys,json; d=json.load(sys.stdin); assert d.get("ok") is True, d; assert d.get("format")=="docx", d' 2>"$WORK/parse-check.log"; then
    echo "  ✓ /parse 真实 docx 通过"
  else
    echo "sidecar 产物 /parse 校验失败（python-docx 模板可能未打进包）：" >&2
    tail -n 30 "$WORK/parse-check.log" >&2
    kill "$PID" 2>/dev/null || true
    wait "$PID" 2>/dev/null || true
    exit 1
  fi
else
  echo "  - 跳过 /parse 回归校验（未找到样本 docx）"
fi
kill "$PID" 2>/dev/null || true
wait "$PID" 2>/dev/null || true

BIN="src-tauri/binaries"
mkdir -p "$BIN"
if [ "$MODE" = onedir ]; then
  # 目录模式：整个目录原样拷贝（exe 必须与 _internal/ 同级）。
  # 注意：Tauri externalBin 只接受单文件，目录需走 bundle.resources，见 docs/release.md。
  DEST="$BIN/anonkit-sidecar-${TRIPLE}-onedir"
  rm -rf "$DEST"
  cp -R "$WORK/dist/anonkit-sidecar" "$DEST"
else
  DEST="$BIN/anonkit-sidecar-${TRIPLE}"
  case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) DEST="$DEST.exe" ;;
  esac
  cp "$EXE" "$DEST"
  chmod +x "$DEST"
fi
echo "== 完成(${MODE}): ${DEST}（$(du -sh "$DEST" | cut -f1)）=="
