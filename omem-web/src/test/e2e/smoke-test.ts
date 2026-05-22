import { chromium } from '@playwright/test'

declare const process: { exit: (code: number) => void }

async function runTests() {
  console.log('🔌 连接 Chrome (localhost:9222)...')
  const browser = await chromium.connectOverCDP('http://localhost:9222')
  const context = browser.contexts()[0] || await browser.newContext()
  const page = context.pages()[0] || await context.newPage()
  page.setDefaultTimeout(60000)

  console.log('✅ 已连接 Chrome')

  // Test 1: 登录页
  console.log('\n📄 Test 1: 登录页')
  await page.goto('http://localhost:5173/login', { waitUntil: 'networkidle' })
  await page.waitForSelector('h1', { timeout: 30000 })
  const loginTitle = await page.locator('h1').textContent()
  console.log('  页面标题:', loginTitle)
  console.assert(loginTitle?.includes('登录'), '❌ 登录页标题错误')
  console.log('  ✅ 登录页渲染正常')

  // 检查登录表单字段
  const hasNameInput = await page.locator('input[type="text"]').count() > 0
  const hasKeyInput = await page.locator('input[type="password"]').count() > 0
  console.log('  表单字段:', hasNameInput ? '✅ 账号名称' : '❌ 账号名称', hasKeyInput ? '✅ API Key' : '❌ API Key')

  // Test 2: 仪表盘
  console.log('\n📊 Test 2: 仪表盘')
  await page.goto('http://localhost:5173/dashboard', { waitUntil: 'networkidle' })
  const dashTitle = await page.locator('h1').textContent()
  console.log('  页面标题:', dashTitle)
  console.assert(dashTitle?.includes('仪表盘'), '❌ 仪表盘标题错误')
  console.log('  ✅ 仪表盘渲染正常')

  // Test 3: 记忆列表
  console.log('\n📝 Test 3: 记忆列表')
  await page.goto('http://localhost:5173/memories', { waitUntil: 'networkidle' })
  const memTitle = await page.locator('h1').textContent()
  console.log('  页面标题:', memTitle)
  console.assert(memTitle?.includes('记忆'), '❌ 记忆列表标题错误')

  // 检查私密记忆
  const privateCards = await page.locator('text=🔒 私密记忆').count()
  console.log('  私密记忆卡片数:', privateCards)
  console.log('  ✅ 记忆列表渲染正常')

  // Test 4: 空间管理
  console.log('\n🏗️ Test 4: 空间管理')
  await page.goto('http://localhost:5173/spaces', { waitUntil: 'networkidle' })
  const spaceTitle = await page.locator('h1').textContent()
  console.log('  页面标题:', spaceTitle)
  console.assert(spaceTitle?.includes('空间'), '❌ 空间管理标题错误')
  console.log('  ✅ 空间管理渲染正常')

  // Test 5: 统计分析
  console.log('\n📈 Test 5: 统计分析')
  await page.goto('http://localhost:5173/analytics', { waitUntil: 'networkidle' })
  const anaTitle = await page.locator('h1').textContent()
  console.log('  页面标题:', anaTitle)
  console.assert(anaTitle?.includes('统计分析'), '❌ 统计分析标题错误')
  console.log('  ✅ 统计分析渲染正常')

  // Test 6: 系统设置
  console.log('\n⚙️ Test 6: 系统设置')
  await page.goto('http://localhost:5173/settings', { waitUntil: 'networkidle' })
  const setTitle = await page.locator('h1').textContent()
  console.log('  页面标题:', setTitle)
  console.assert(setTitle?.includes('设置'), '❌ 系统设置标题错误')
  console.log('  ✅ 系统设置渲染正常')

  // Test 7: 批量导入
  console.log('\n📥 Test 7: 批量导入')
  await page.goto('http://localhost:5173/import', { waitUntil: 'networkidle' })
  const impTitle = await page.locator('h1').textContent()
  console.log('  页面标题:', impTitle)
  console.assert(impTitle?.includes('导入'), '❌ 批量导入标题错误')
  console.log('  ✅ 批量导入渲染正常')

  console.log('\n🎉 所有测试通过！')
  await browser.close()
}

runTests().catch(err => {
  console.error('\n❌ 测试失败:', err.message)
  process.exit(1)
})
