import { test, expect } from './auth.setup'

test.describe('Dashboard', () => {
  test('should render dashboard with stats cards', async ({ page }) => {
    await page.goto('/dashboard')
    await expect(page.getByText('总记忆数')).toBeVisible()
    await expect(page.getByText('空间数')).toBeVisible()
    await expect(page.getByText('最近7天新增')).toBeVisible()
  })

  test('should have no console errors', async ({ page }) => {
    const errors: string[] = []
    page.on('console', msg => {
      if (msg.type() === 'error') errors.push(msg.text())
    })
    await page.goto('/dashboard')
    await page.waitForLoadState('networkidle')
    expect(errors).toHaveLength(0)
  })
})
