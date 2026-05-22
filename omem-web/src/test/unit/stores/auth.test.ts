import { describe, it, expect, beforeEach } from 'vitest'
import { useAuthStore } from '@/stores/auth'

describe('Auth Store', () => {
  beforeEach(() => {
    localStorage.clear()
    useAuthStore.setState({
      users: [],
      currentUserId: null,
      isAuthenticated: false,
    })
  })

  it('should add user and authenticate', () => {
    const store = useAuthStore.getState()
    store.addUser({
      id: 'user-1',
      name: '测试用户',
      apiKey: 'test-key',
      apiUrl: 'https://test.com',
      lastUsed: new Date().toISOString(),
    })

    const state = useAuthStore.getState()
    expect(state.users).toHaveLength(1)
    expect(state.isAuthenticated).toBe(true)
    expect(state.currentUserId).toBe('user-1')
  })

  it('should switch between users', () => {
    const store = useAuthStore.getState()
    store.addUser({
      id: 'user-1',
      name: '用户A',
      apiKey: 'key-a',
      apiUrl: '/',
      lastUsed: new Date().toISOString(),
    })
    store.addUser({
      id: 'user-2',
      name: '用户B',
      apiKey: 'key-b',
      apiUrl: '/',
      lastUsed: new Date().toISOString(),
    })

    store.setCurrentUser('user-2')
    const state = useAuthStore.getState()
    expect(state.currentUserId).toBe('user-2')
    expect(state.users).toHaveLength(2)
  })

  it('should remove user and update auth state', () => {
    const store = useAuthStore.getState()
    store.addUser({
      id: 'user-1',
      name: '用户A',
      apiKey: 'key-a',
      apiUrl: '/',
      lastUsed: new Date().toISOString(),
    })
    store.removeUser('user-1')

    const state = useAuthStore.getState()
    expect(state.users).toHaveLength(0)
    expect(state.isAuthenticated).toBe(false)
    expect(state.currentUserId).toBeNull()
  })

  it('should logout', () => {
    const store = useAuthStore.getState()
    store.addUser({
      id: 'user-1',
      name: '用户A',
      apiKey: 'key-a',
      apiUrl: '/',
      lastUsed: new Date().toISOString(),
    })
    store.logout()

    const state = useAuthStore.getState()
    expect(state.isAuthenticated).toBe(false)
    expect(state.currentUserId).toBeNull()
    expect(state.users).toHaveLength(1)
  })

  it('should persist to localStorage', () => {
    const store = useAuthStore.getState()
    store.addUser({
      id: 'user-1',
      name: '持久化测试',
      apiKey: 'key',
      apiUrl: '/',
      lastUsed: '2026-04-18T10:00:00Z',
    })

    expect(localStorage.setItem).toHaveBeenCalledWith(
      'omem-auth',
      expect.stringContaining('持久化测试')
    )
  })
})
