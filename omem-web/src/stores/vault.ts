import { create } from "zustand"
import apiClient from "@/api/client"

interface VaultState {
  isUnlocked: boolean
  hasPassword: boolean
  isLoading: boolean
  setPassword: (password: string) => Promise<void>
  verifyPassword: (password: string) => Promise<boolean>
  unlock: (password: string) => Promise<boolean>
  lock: () => void
  checkStatus: () => Promise<void>
}

export const useVaultStore = create<VaultState>((set, get) => ({
  isUnlocked: false,
  hasPassword: false,
  isLoading: false,
  
  checkStatus: async () => {
    try {
      const res = await apiClient.get<{ has_password: boolean }>("/v1/vault/status")
      set({ hasPassword: res.has_password })
    } catch {
      set({ hasPassword: false })
    }
  },
  
  setPassword: async (password) => {
    set({ isLoading: true })
    try {
      await apiClient.post("/v1/vault/password", { password })
      set({ isUnlocked: true, hasPassword: true })
    } finally {
      set({ isLoading: false })
    }
  },
  
  verifyPassword: async (password) => {
    try {
      const res = await apiClient.post<{ valid: boolean }>("/v1/vault/verify", { password })
      return res.valid
    } catch {
      return false
    }
  },
  
  unlock: async (password) => {
    const valid = await get().verifyPassword(password)
    if (valid) {
      set({ isUnlocked: true })
    }
    return valid
  },
  
  lock: () => set({ isUnlocked: false }),
}))
