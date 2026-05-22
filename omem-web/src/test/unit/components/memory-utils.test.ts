import { describe, it, expect } from 'vitest'
import { formatContent, formatDate } from '@/views/memories/memory-list'
import { isPrivateMemory } from '@/lib/tag-utils'

const createMemory = (tags: string[]) => ({
  id: 'test-id',
  content: '测试内容',
  l0_abstract: '',
  l1_overview: '',
  l2_content: '测试内容',
  category: 'test',
  memory_type: 'pinned',
  state: 'active',
  tier: 'core',
  importance: 0.5,
  confidence: 0.8,
  access_count: 1,
  tags,
  scope: 'global',
  created_at: '2026-04-18T10:00:00Z',
  updated_at: '2026-04-18T10:00:00Z',
})

describe('Memory List Utils', () => {
  it('should detect private memory by tag', () => {
    expect(isPrivateMemory(createMemory(['私密']).tags)).toBe(true)
    expect(isPrivateMemory(createMemory(['工作', '私密']).tags)).toBe(true)
  })

  it('should not detect private memory without tag', () => {
    expect(isPrivateMemory(createMemory(['工作']).tags)).toBe(false)
    expect(isPrivateMemory(createMemory([]).tags)).toBe(false)
  })

  it('should format content with truncation', () => {
    const longContent = 'a'.repeat(200)
    expect(formatContent(longContent)).toHaveLength(123)
    expect(formatContent(longContent)).toMatch(/\.\.\.$/)

    const shortContent = '短内容'
    expect(formatContent(shortContent)).toBe('短内容')
  })

  it('should format date to Chinese locale', () => {
    const result = formatDate('2026-04-18T10:30:00Z')
    expect(result).toContain('2026')
    expect(result).toContain('04')
    expect(result).toContain('18')
  })

  it('should handle empty content', () => {
    expect(formatContent('')).toBe('—')
    expect(formatContent(undefined)).toBe('—')
  })
})
