
const { chromium } = require('playwright');

(async () => {
    const browser = await chromium.launch();
    const page = await browser.newPage();
    
    await page.setContent(`
<!DOCTYPE html>
<html>
<head><meta charset="UTF-8"></head>
<body style="margin:0">
    <canvas id="canvas" width="1200" height="800"></canvas>
    <script>
        const canvas = document.getElementById('canvas');
        const ctx = canvas.getContext('2d');
        
        ctx.fillStyle = '#F8F9FA';
        ctx.fillRect(0, 0, 1200, 800);
        
        const colors = {
            browser: '#E8EAF6',
            nginx: '#FFF3E0',
            server: '#E0F2F1',
            text: '#263238',
            arrow: '#546E7A',
            border: '#B0BEC5'
        };
        
        function drawLayer(x, y, w, h, fillColor) {
            ctx.fillStyle = fillColor;
            ctx.fillRect(x, y, w, h);
            ctx.strokeStyle = colors.border;
            ctx.lineWidth = 2;
            ctx.strokeRect(x, y, w, h);
        }
        
        function drawArrow(x, y1, y2) {
            ctx.strokeStyle = colors.arrow;
            ctx.lineWidth = 3;
            ctx.beginPath();
            ctx.moveTo(x, y1);
            ctx.lineTo(x, y2);
            ctx.stroke();
            
            ctx.fillStyle = colors.arrow;
            ctx.beginPath();
            ctx.moveTo(x, y2);
            ctx.lineTo(x - 10, y2 - 15);
            ctx.lineTo(x + 10, y2 - 15);
            ctx.closePath();
            ctx.fill();
        }
        
        const layerW = 900, layerH = 180, startX = 150;
        const y1 = 80, y2 = 380, y3 = 680;
        
        drawLayer(startX, y1, layerW, layerH, colors.browser);
        drawLayer(startX, y2, layerW, layerH, colors.nginx);
        drawLayer(startX, y3, layerW, layerH, colors.server);
        
        const arrowX = startX + layerW / 2;
        drawArrow(arrowX, y1 + layerH, y2);
        drawArrow(arrowX, y2 + layerH, y3);
        
        ctx.fillStyle = colors.text;
        ctx.font = 'bold 32px sans-serif';
        ctx.fillText('用户浏览器', startX + 30, y1 + 50);
        ctx.font = '20px sans-serif';
        ctx.fillText('Vue 3 + Vite', startX + 30, y1 + 90);
        ctx.fillText('Ant Design Vue + Pinia', startX + 30, y1 + 120);
        ctx.fillText('Axios (X-API-Key Header)', startX + 30, y1 + 150);
        
        ctx.font = 'bold 32px sans-serif';
        ctx.fillText('Nginx 服务器', startX + 30, y2 + 50);
        ctx.font = '20px sans-serif';
        ctx.fillText('www.mengxy.cc', startX + 30, y2 + 90);
        ctx.fillText('静态文件服务 + API反向代理', startX + 30, y2 + 120);
        ctx.fillText('/v1/* → localhost:8080/v1/*', startX + 30, y2 + 150);
        
        ctx.font = 'bold 32px sans-serif';
        ctx.fillText('omem-server', startX + 30, y3 + 50);
        ctx.font = '20px sans-serif';
        ctx.fillText('Rust + Axum', startX + 30, y3 + 90);
        ctx.fillText('localhost:8080', startX + 30, y3 + 120);
        ctx.fillText('REST API (48+ endpoints)', startX + 30, y3 + 150);
        
        ctx.fillStyle = colors.arrow;
        ctx.font = '18px sans-serif';
        ctx.fillText('HTTPS', arrowX + 20, (y1 + layerH + y2) / 2);
        ctx.font = '16px sans-serif';
        ctx.fillText('/v1/*', arrowX + 20, (y1 + layerH + y2) / 2 + 25);
        ctx.font = '18px sans-serif';
        ctx.fillText('HTTP', arrowX + 20, (y2 + layerH + y3) / 2);
        ctx.font = '16px sans-serif';
        ctx.fillText('localhost:8080', arrowX + 20, (y2 + layerH + y3) / 2 + 25);
    </script>
</body>
</html>
    `);
    
    await page.waitForTimeout(1000);
    await page.screenshot({ path: 'architecture.png', fullPage: true });
    await browser.close();
    console.log('架构图已生成：architecture.png');
})();
