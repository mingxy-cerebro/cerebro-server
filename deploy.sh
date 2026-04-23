#!/bin/bash
# omem-server 一键部署脚本
# 用法: ./deploy.sh [binary_path]
# 默认从 target/release/omem-server 读取

set -e

SERVER="root@47.93.199.242"
REMOTE_BIN="/opt/omem/omem-server"
BINARY="${1:-target/release/omem-server}"
SSHPASS="Mengfanbo@0714"

echo "🔨 检查编译产物..."
if [ ! -f "$BINARY" ]; then
  echo "❌ 找不到 binary: $BINARY"
  echo "   先编译: cd omem-server && cargo build --release"
  exit 1
fi

echo "📦 Binary 大小: $(du -h "$BINARY" | cut -f1)"

echo "🛑 停止服务 (omem.service)..."
sshpass -p "$SSHPASS" ssh "$SERVER" "
  systemctl stop omem 2>/dev/null || true
  sleep 1
  # 强制杀残留进程
  pkill -9 -f omem-server 2>/dev/null || true
  sleep 2
  # 确认端口释放
  if ss -tlnp | grep -q ':8080'; then
    echo '❌ 端口 8080 仍被占用，强制清理...'
    fuser -k 8080/tcp 2>/dev/null || true
    sleep 2
  fi
  ss -tlnp | grep -q ':8080' && echo '❌ 端口仍未释放！' && exit 1 || echo '✅ 端口已释放'
"

echo "☁️  上传 binary..."
sshpass -p "$SSHPASS" scp "$BINARY" "$SERVER:$REMOTE_BIN"

echo "🚀 启动服务..."
sshpass -p "$SSHPASS" ssh "$SERVER" "
  chmod +x $REMOTE_BIN
  systemctl enable omem
  systemctl start omem
  sleep 3
"

echo "🔍 验证..."
sshpass -p "$SSHPASS" ssh "$SERVER" "
  # 等待启动
  for i in 1 2 3 4 5; do
    if curl -s http://localhost:8080/health | grep -q ok; then
      echo '✅ omem-server 运行正常'
      systemctl status omem --no-pager | head -5
      exit 0
    fi
    sleep 1
  done
  echo '❌ 健康检查失败！'
  journalctl -u omem --since '10 sec ago' --no-pager | tail -10
  exit 1
"

echo "🎉 部署完成！"
