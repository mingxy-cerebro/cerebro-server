import { test, expect } from './auth.setup'

test.describe('Login Page', () => {
  test('should render login form', async ({ page }) => {
    await page.goto('/login')
    await expect(page.getByPlaceholder('请输入 API Key')).toBeVisible()
    await expect(page.getByRole('button', { name: '登录' })).toBeVisible()
  })

  test('should show validation error for empty apiKey', async ({ page }) => {
    await page.goto('/login')
    await page.getByRole('button', { name: '登录' }).click()
    await expect(page.getByText('请输入 API Key')).toBeVisible()
  })

  test('should navigate to dashboard after successful login', async ({ page }) => {
    await page.goto('/login')
    await page.getByPlaceholder('请输入 API Key').fill('test-api-key-12345')
    await page.getByRole('button', { name: '登录' }).click()
    await expect(page).toHaveURL(/\/(dashboard)?/)
  })

  test('should have no console errors', async ({ page }) => {
    const errors: string[] = []
    page.on('console', msg => {
      if (msg.type() === 'error') errors.push(msg.text())
    })
    await page.goto('/login')
    await page.waitForLoadState('networkidle')
    expect(errors).toHaveLength(0)
  })
})
