import '@testing-library/jest-dom'
import { vi } from 'vitest'

const storageMock = {
  getItem: vi.fn(),
  setItem: vi.fn(),
  removeItem: vi.fn(),
  clear: vi.fn(),
}
Object.defineProperty(window, 'localStorage', { value: storageMock })
Object.defineProperty(window, 'sessionStorage', { value: storageMock })
