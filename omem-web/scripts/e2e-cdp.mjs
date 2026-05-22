import { chromium } from 'playwright-core'
import { fileURLToPath } from 'url'
import { dirname, join } from 'path'
import fs from 'fs'

const __filename = fileURLToPath(import.meta.url)
const __dirname = dirname(__filename)
const SCREENSHOT_DIR = join(__dirname, '..', 'e2e-screenshots')

// Ensure screenshot directory exists
if (!fs.existsSync(SCREENSHOT_DIR)) {
  fs.mkdirSync(SCREENSHOT_DIR, { recursive: true })
}

const BASE_URL = 'https://www.mengxy.cc'
const PAGES = [
  { path: '/', name: 'login' },
  { path: '/dashboard', name: 'dashboard' },
  { path: '/memories', name: 'memories' },
  { path: '/spaces', name: 'spaces' },
  { path: '/analytics', name: 'analytics' },
  { path: '/import', name: 'import' },
  { path: '/settings', name: 'settings' },
  { path: '/profile', name: 'profile' },
]

async function runTests() {
  console.log('Connecting to Chrome via CDP...')
  const browser = await chromium.connectOverCDP('http://localhost:9222')
  console.log(`Connected: ${browser.version()}`)

  // Create a new incognito context for clean state
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
  })

  // Set auth data for API calls + bypass
  await context.addInitScript(() => {
    localStorage.setItem('e2e_bypass_auth', 'true')
    localStorage.setItem('omem-auth', JSON.stringify({
      state: {
        apiKey: 'c60beb98-7aab-4985-8c1d-29ffd6aff75a',
        baseUrl: 'https://www.mengxy.cc',
        spaceName: 'MengFanbo',
        isAuthenticated: true,
      },
      version: 0,
    }))
  })

  const results = []

  for (const pageConfig of PAGES) {
    const page = await context.newPage()
    const url = `${BASE_URL}${pageConfig.path}`
    const errors = []
    const consoleLogs = []

    // Capture console messages and errors
    page.on('console', (msg) => {
      const text = msg.text()
      consoleLogs.push({ type: msg.type(), text })
      if (msg.type() === 'error') {
        errors.push(text)
      }
    })

    page.on('pageerror', (err) => {
      errors.push(err.message)
    })

    console.log(`\n[${pageConfig.name}] Testing ${url}...`)

    try {
      await page.goto(url, { waitUntil: 'networkidle', timeout: 15000 })
      await page.waitForTimeout(1000) // Wait for any async rendering

      // Take screenshot
      const screenshotPath = join(SCREENSHOT_DIR, `${pageConfig.name}.png`)
      await page.screenshot({ path: screenshotPath, fullPage: false })

      const result = {
        name: pageConfig.name,
        url,
        status: errors.length === 0 ? 'PASS' : 'FAIL',
        errors,
        consoleCount: consoleLogs.length,
        consoleErrors: consoleLogs.filter((c) => c.type === 'error').length,
        screenshot: screenshotPath,
      }
      results.push(result)

      if (errors.length === 0) {
        console.log(`  PASS - 0 errors, ${consoleLogs.length} console messages`)
      } else {
        console.log(`  FAIL - ${errors.length} errors:`)
        errors.forEach((e) => { console.log(`    - ${e}`) })
      }
    } catch (err) {
      results.push({
        name: pageConfig.name,
        url,
        status: 'ERROR',
        errors: [err.message],
        consoleCount: 0,
        consoleErrors: 0,
        screenshot: null,
      })
      console.log(`  ERROR - ${err.message}`)
    } finally {
      await page.close()
    }
  }

  await context.close()
  await browser.close()

  // Print summary
  console.log('\n' + '='.repeat(60))
  console.log('E2E TEST SUMMARY')
  console.log('='.repeat(60))

  const passed = results.filter((r) => r.status === 'PASS').length
  const failed = results.filter((r) => r.status === 'FAIL').length
  const errors = results.filter((r) => r.status === 'ERROR').length

  results.forEach((r) => {
    let icon
    if (r.status === 'PASS') icon = 'PASS'
    else if (r.status === 'FAIL') icon = 'FAIL'
    else icon = 'ERR '
    console.log(`[${icon}] ${r.name.padEnd(12)} - ${r.errors.length} errors`)
  })

  console.log('-'.repeat(60))
  console.log(`Total: ${results.length} | Passed: ${passed} | Failed: ${failed} | Errors: ${errors}`)
  console.log(`Screenshots: ${SCREENSHOT_DIR}`)
  console.log('='.repeat(60))

  // Exit with error code if any failures
  process.exit(failed > 0 || errors > 0 ? 1 : 0)
}

runTests().catch((err) => {
  console.error('Test runner failed:', err)
  process.exit(1)
})
