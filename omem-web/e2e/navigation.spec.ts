import { test, expect } from './auth.setup'

test.describe('Navigation', () => {
  test('should navigate between pages via sidebar', async ({ page }) => {
    await page.goto('/dashboard')

    await page.getByRole('link', { name: '记忆列表' }).click()
    await expect(page).toHaveURL(/\/memories/)

    await page.getByRole('link', { name: '空间管理' }).click()
    await expect(page).toHaveURL(/\/spaces/)

    await page.getByRole('link', { name: '统计分析' }).click()
    await expect(page).toHaveURL(/\/analytics/)

    await page.getByRole('link', { name: '系统设置' }).click()
    await expect(page).toHaveURL(/\/settings/)
  })

  test('should have no console errors on any page', async ({ page }) => {
    const errors: string[] = []
    page.on('console', msg => {
      if (msg.type() === 'error') errors.push(msg.text())
    })

    const pages = ['/dashboard', '/memories', '/spaces', '/analytics', '/settings', '/profile']
    for (const url of pages) {
      await page.goto(url)
      await page.waitForLoadState('networkidle')
    }

    expect(errors).toHaveLength(0)
  })
})
