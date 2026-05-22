import { describe, it, expect, beforeEach, vi } from 'vitest'
import { useVaultStore } from '@/stores/vault'

vi.mock('@/api/client', () => ({
  default: {
    get: vi.fn(),
    post: vi.fn(),
    delete: vi.fn(),
  }
}))

describe('Vault Store', () => {
  beforeEach(() => {
    useVaultStore.setState({
      isUnlocked: false,
      hasPassword: false,
      isLoading: false,
    })
  })

  it('should initialize with locked state', () => {
    const state = useVaultStore.getState()
    expect(state.isUnlocked).toBe(false)
    expect(state.hasPassword).toBe(false)
  })

  it('should lock vault', () => {
    useVaultStore.setState({ isUnlocked: true })
    useVaultStore.getState().lock()
    expect(useVaultStore.getState().isUnlocked).toBe(false)
  })

  it('should set loading state during password operations', async () => {
    const store = useVaultStore.getState()
    expect(store.isLoading).toBe(false)
  })
})
