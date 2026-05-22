import { test, expect } from './auth.setup'

test.describe('Memories Page', () => {
  test('should render memories list', async ({ page }) => {
    await page.goto('/memories')
    await expect(page.getByPlaceholder('搜索记忆...')).toBeVisible()
    await expect(page.getByRole('button', { name: '新建记忆' })).toBeVisible()
  })

  test('should have no console errors', async ({ page }) => {
    const errors: string[] = []
    page.on('console', msg => {
      if (msg.type() === 'error') errors.push(msg.text())
    })
    await page.goto('/memories')
    await page.waitForLoadState('networkidle')
    expect(errors).toHaveLength(0)
  })
})
