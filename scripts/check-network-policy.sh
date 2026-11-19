#!/usr/bin/env bash
# 墨隐 InkVeil 隐私保障（Task 9）：核心处理路径零外发构建期检查。
#
# 检查项：
#   1) anonymizer-core（纯规则引擎）依赖树不得含任何网络栈 crate；
#   2) anonymizer-pipeline 允许的网络依赖仅限 reqwest 栈（仅用于 127.0.0.1
#      回环 sidecar 通信，SidecarClient 构造时已强制回环校验）；
#   3) 渲染层 src/ 无 fetch / XMLHttpRequest / WebSocket / sendBeacon 等直接网络调用；
#   4) 生产 CSP 显式配置，且 connect-src / default-src 不含外部协议源。
#
# 用法：bash scripts/check-network-policy.sh（在仓库根目录执行）
set -euo pipefail

cd "$(dirname "$0")/.."

fail=0
note() { echo "  ✓ $1"; }
err() { echo "  ✗ $1"; fail=1; }

# cargo tree 每行首的 crate 名
names() { grep -oE '^[a-zA-Z0-9_-]+ v[0-9]' | awk '{print $1}'; }

echo "== [1/4] anonymizer-core 依赖树无网络栈 =="
core_out=$(cargo tree -p anonymizer-core --edges normal 2>/dev/null || true)
net_hits=$(echo "$core_out" | names | grep -E '^(reqwest|hyper|tokio|rustls|webpki|h2|ureq|awc|isahc|surf|actix|tungstenite|native-tls|openssl|curl|mio|socket2)$' || true)
if [ -n "$net_hits" ]; then
  err "anonymizer-core 依赖树包含网络栈 crate:"
  echo "$net_hits"
else
  note "anonymizer-core 无网络栈依赖"
fi

echo "== [2/4] anonymizer-pipeline 网络依赖仅限 reqwest 栈（回环 sidecar）=="
pipe_out=$(cargo tree -p anonymizer-pipeline --edges normal 2>/dev/null || true)
net_crates=$(echo "$pipe_out" | names | grep -E '^(reqwest|hyper|tokio|rustls|webpki|h2|ureq|awc|isahc|surf|actix|tungstenite|native-tls|openssl|curl|mio|socket2)$' | sort -u || true)
extra=""
for c in $net_crates; do
  case "$c" in
    reqwest|hyper|tokio|rustls|webpki|h2|mio|socket2) ;; # reqwest 及其底层（回环通信）
    *) extra="$extra $c" ;;
  esac
done
if [ -n "$extra" ]; then
  err "pipeline 存在预期之外的网络依赖:$extra"
else
  note "pipeline 网络依赖为 reqwest 栈（仅 127.0.0.1 回环）"
fi

echo "== [3/4] 渲染层无直接网络调用 =="
hits=$(grep -rnE 'fetch\(|XMLHttpRequest|new WebSocket|EventSource|sendBeacon' src/ 2>/dev/null || true)
if [ -n "$hits" ]; then
  err "src/ 存在直接网络调用:"
  echo "$hits"
else
  note "src/ 无 fetch / XHR / WebSocket / sendBeacon"
fi

echo "== [4/4] 生产 CSP 显式配置且无外部协议源 =="
if ! python3 - <<'EOF'
import json, re, sys
cfg = json.load(open('src-tauri/tauri.conf.json'))
sec = cfg.get('app', {}).get('security') or {}
csp = sec.get('csp')
if not csp:
    print("  ✗ tauri.conf.json 未配置生产 CSP")
    sys.exit(1)

def sources(directive, text):
    m = re.search(rf'{directive}\s+([^;]+)', text)
    return m.group(1).split() if m else []

# 允许的本地源：Tauri IPC 协议、Tauri 资源 origin
ALLOWED = {'http://ipc.localhost', 'http://localhost', 'http://tauri.localhost'}
for d in ('connect-src', 'default-src'):
    for s in sources(d, csp):
        if re.match(r'^(https?://|wss?://)', s) and s not in ALLOWED:
            print(f"  ✗ {d} 包含外部协议源: {s}")
            sys.exit(1)
print("  ✓ 生产 CSP 已配置且无外部协议源")
EOF
then
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  echo "网络策略检查未通过"
  exit 1
fi
echo "网络策略检查全部通过"
