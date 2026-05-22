#!/bin/bash
set -e

echo "🚀 omem-web 部署脚本"
echo "===================="

# 配置
SERVER="47.93.199.242"
REMOTE_DIR="/var/www/omem-web"
NGINX_CONF="/etc/nginx/sites-enabled/www.mengxy.cc.conf"

echo "📦 Step 1: 构建项目..."
npm run build

echo ""
echo "📤 Step 2: 上传到服务器..."
echo "   需要配置SSH密钥后才能自动上传"
echo "   手动上传命令："
echo "   scp -r dist/* root@$SERVER:$REMOTE_DIR/"

echo ""
echo "🔄 Step 3: 重载Nginx..."
echo "   ssh root@$SERVER 'nginx -s reload'"

echo ""
echo "✅ 部署完成！"
echo "   访问: https://www.mengxy.cc"
