import { chromium } from 'playwright-core'

const BASE_URL = 'https://www.mengxy.cc'

async function testProfile() {
  console.log('Connecting to Chrome via CDP...')
  const browser = await chromium.connectOverCDP('http://localhost:9222')
  console.log(`Connected: ${browser.version()}`)

  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
  })

  // Set auth
  await context.addInitScript(() => {
    const authData = JSON.stringify({
      state: {
        apiKey: 'c60beb98-7aab-4985-8c1d-29ffd6aff75a',
        baseUrl: 'https://www.mengxy.cc',
        spaceName: 'MengFanbo',
        isAuthenticated: true,
      },
      version: 0,
    })
    sessionStorage.setItem('omem-auth', authData)
    localStorage.setItem('e2e_bypass_auth', 'true')
  })

  const page = await context.newPage()
  const errors = []

  page.on('console', (msg) => {
    if (msg.type() === 'error') {
      errors.push(msg.text())
    }
  })

  page.on('pageerror', (err) => {
    errors.push(err.message)
  })

  console.log('Testing profile page...')
  await page.goto(`${BASE_URL}/profile`, { waitUntil: 'networkidle', timeout: 15000 })
  await page.waitForTimeout(2000)

  const url = page.url()
  console.log(`Current URL: ${url}`)

  if (errors.length === 0) {
    console.log('PASS - 0 errors')
  } else {
    console.log(`FAIL - ${errors.length} errors:`)
    errors.forEach((e) => { console.log(`  - ${e}`) })
  }

  await context.close()
  await browser.close()
}

testProfile().catch(console.error)
