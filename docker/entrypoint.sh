#!/bin/sh
# ============================================================================
# Freebuff2API 容器入口（Railway / Render / Fly / 普通 docker run 通用）
#
# 只做三件事，然后把进程交给网关本体：
#   1. 确保数据目录存在（平台持久卷挂载点）
#   2. 补齐启动期必需的环境变量（api_keys / skip_upstream_check）
#   3. exec 网关（有 config.json 就用它，没有就纯环境变量驱动）
#
# 为什么不在这里生成 config.json：
#   网关本身已支持「默认值 + 环境变量」，硬造 JSON 还要处理 token 里的引号/反斜杠转义，
#   转义一旦出错就是启动即崩。少一层字符串拼接，少一类玄学故障。
# ============================================================================
set -eu

DATA_DIR="${DATA_DIR:-/data}"
# 端口只是给日志用；真正生效的是 src/config.rs 里的 PORT 处理
PORT="${PORT:-47821}"
export DATA_DIR PORT

mkdir -p "$DATA_DIR" 2>/dev/null || echo "[entrypoint] 警告：无法创建 $DATA_DIR，数据可能不持久"

# --- api_keys：对外监听必须配置（config.rs::validate 会硬拒绝裸奔）----------------
# PaaS 上不适合让用户先读文档再设变量 —— 未提供就自动生成一个并打到日志里。
# 关键：生成的 Key 持久化到 $DATA_DIR/.auto_api_key（挂了卷就跨部署稳定），
# 否则每次重新部署都换 Key，用户粘到面板/客户端里的旧 Key 直接作废。
if [ -z "${API_KEYS:-}" ]; then
  key_file="$DATA_DIR/.auto_api_key"
  if [ -s "$key_file" ]; then
    API_KEYS="$(cat "$key_file")"
    echo "[entrypoint] 未设置 API_KEYS：沿用持久化的自动生成 Key（$key_file）"
  else
    if command -v openssl >/dev/null 2>&1; then
      generated="sk-fb-$(openssl rand -hex 24)"
    else
      generated="sk-fb-$(od -An -N24 -tx1 /dev/urandom | tr -d ' \n')"
    fi
    API_KEYS="$generated"
    printf '%s\n' "$API_KEYS" > "$key_file" 2>/dev/null \
      && chmod 600 "$key_file" 2>/dev/null \
      || echo "[entrypoint] 警告：无法持久化自动生成的 Key（$key_file 不可写），下次重启会换新 Key"
  fi
  export API_KEYS
  echo "======================================================================"
  echo "[entrypoint] 当前 API key（即面板右上角要粘的那个）"
  echo ""
  echo "    $API_KEYS"
  echo ""
  echo "  - 首次自动生成时已写入 $key_file，挂了持久卷则跨部署不变"
  echo "  - 想自己指定：在平台变量里设 API_KEYS=<你的值>（优先级更高）"
  echo "  - 「生成并启用 Key」按钮需先用上面这把 Key 登录（右上角）才能用"
  echo "======================================================================"
fi

# --- 凭证：首次部署还没有 AUTH_TOKENS 时，先让服务起来，否则容器会 crash-loop ---
if [ -z "${AUTH_TOKENS:-}" ] && [ -z "${SKIP_UPSTREAM_CHECK:-}" ]; then
  SKIP_UPSTREAM_CHECK=true
  export SKIP_UPSTREAM_CHECK
  echo "[entrypoint] 未设置 AUTH_TOKENS：以空账号池启动（SKIP_UPSTREAM_CHECK=true）。"
  echo "            启动后在 /ui 面板导入凭证即可，无需重启。"
  echo "            想固定凭证：设 AUTH_TOKENS=token1,token2（或挂载 $DATA_DIR/config.json）。"
fi

echo "[entrypoint] 启动：DATA_DIR=$DATA_DIR PORT=$PORT 配置源=$(
  if [ -f "$DATA_DIR/config.json" ]; then echo "$DATA_DIR/config.json"; else echo "环境变量"; fi
)"

if [ -f "$DATA_DIR/config.json" ]; then
  exec /usr/local/bin/freebuff2api --config "$DATA_DIR/config.json"
fi

exec /usr/local/bin/freebuff2api
