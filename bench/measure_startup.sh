#!/usr/bin/env bash
# 冷启动与空闲内存实测（spec：应用启动 <1.5s、空闲内存 <120MB）。
#
# 测量对象：release 产物 `target/release/InkVeil`（含 sidecar 随包启动）。
# 输出：
#   - 窗口出现时间（用户感知的启动完成，门槛 <1.5s；osascript 失败则记录 N/A）
#   - sidecar /health 就绪时间（后台依赖就绪）
#   - 空闲 RSS：主进程 + sidecar 工作进程（取同名下 RSS 最大者，规避
#     PyInstaller onefile bootstrap 小进程）
#
# 用法：bash bench/measure_startup.sh
set -uo pipefail
cd "$(dirname "$0")/.."

APP="target/release/InkVeil"
[ -x "$APP" ] || { echo "缺少 $APP，先 npm run tauri build"; exit 2; }

T0=$(python3 -c 'import time; print(time.time())')
"$APP" >/tmp/anonkit-startup.log 2>&1 &
APP_PID=$!

# ---- 窗口出现时间（用户感知启动完成）----
WINDOW_T=""
if command -v osascript >/dev/null 2>&1; then
  for _ in $(seq 1 40); do
    WIN=$(osascript -e 'tell application "System Events" to count windows of process "InkVeil"' 2>/dev/null)
    if [ "${WIN:-0}" -ge 1 ] 2>/dev/null; then
      WINDOW_T=$(python3 -c 'import time; print(f"{(time.time()-'"$T0"'):.2f}")')
      break
    fi
    sleep 0.25
  done
fi

# ---- sidecar /health 就绪 ----
SIDECAR_PID=""
PORT=""
for _ in $(seq 1 60); do
  SIDECAR_PID="$(pgrep -f 'anonkit-sidecar.*--port' | head -n1 || true)"
  if [ -n "$SIDECAR_PID" ]; then
    PORT="$(ps -o command= -p "$SIDECAR_PID" 2>/dev/null | sed -n 's/.*--port[ =]*\([0-9]*\).*/\1/p')"
    [ -n "$PORT" ] && break
  fi
  sleep 0.25
done

READY=""
if [ -n "$PORT" ]; then
  for _ in $(seq 1 60); do
    if curl -sf "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; then READY=1; break; fi
    sleep 0.25
  done
fi
T1=$(python3 -c 'import time; print(time.time())')
READY_T=$(python3 -c "print(f'{$T1-$T0:.2f}')")

echo "窗口出现: ${WINDOW_T:-N/A}s（门槛 1.5s，N/A=osascript 受限）"
echo "sidecar 就绪: ${READY_T}s${READY:-（未就绪，见 /tmp/anonkit-startup.log）}"

# ---- 空闲 RSS（3 次采样取最小）----
main_min=""
side_min=""
for i in 1 2 3; do
  sleep 2
  if ps -p "$APP_PID" >/dev/null 2>&1; then
    m=$(ps -o rss= -p "$APP_PID" 2>/dev/null | awk '{print $1/1024}')
    [ -z "$main_min" ] && main_min=$m
    main_min=$(python3 -c "print(min($m, ${main_min:-$m}))")
  fi
  # sidecar 工作进程：同名下 RSS 最大者（PyInstaller bootstrap 很小，忽略）
  max_rss=$(pgrep -f 'anonkit-sidecar' 2>/dev/null | while read -r p; do
    ps -o rss= -p "$p" 2>/dev/null
  done | sort -rn | head -n1)
  if [ -n "$max_rss" ]; then
    s=$(awk -v x="$max_rss" 'BEGIN{print x/1024}')
    [ -z "$side_min" ] && side_min=$s
    side_min=$(python3 -c "print(min($s, ${side_min:-$s}))")
  fi
done

kill "$APP_PID" 2>/dev/null || true
pkill -f "target/release/InkVeil" 2>/dev/null || true
pkill -f "anonkit-sidecar" 2>/dev/null || true

main_f="${main_min:-N/A}"
side_f="${side_min:-N/A}"
echo "空闲内存: 主进程=${main_f}MB + sidecar=${side_f}MB"
python3 -c "
m = float('${main_min:-nan}'); s = float('${side_min:-nan}')
t = m + s
print(f'合计: {t:.0f}MB（门槛 120MB）→', 'PASS' if t <= 120 else 'FAIL')
" 2>/dev/null || true
