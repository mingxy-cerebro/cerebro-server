import { create } from "zustand"
import { persist } from "zustand/middleware"

export interface User {
  id: string
  name: string
  apiKey: string
  apiUrl: string
  lastUsed: string
  spaceName?: string
  isProtected?: boolean
}

interface AuthState {
  users: User[]
  currentUserId: string | null
  isAuthenticated: boolean
  addUser: (user: User) => void
  setCurrentUser: (id: string) => void
  removeUser: (id: string) => void
  removeUsersByApiKeys: (apiKeys: string[]) => void
  logout: () => void
}

export const useAuthStore = create<AuthState>()(
  persist(
    (set) => ({
      users: [],
      currentUserId: null,
      isAuthenticated: false,
      addUser: (user) =>
        set((state) => ({
          users: [...state.users, user],
          currentUserId: user.id,
          isAuthenticated: true,
        })),
      setCurrentUser: (id) =>
        set({
          currentUserId: id,
          isAuthenticated: true,
        }),
      removeUser: (id) =>
        set((state) => {
          const newUsers = state.users.filter((u) => u.id !== id)
          const newCurrentId =
            state.currentUserId === id
              ? newUsers.length > 0
                ? newUsers[0].id
                : null
              : state.currentUserId
          return {
            users: newUsers,
            currentUserId: newCurrentId,
            isAuthenticated: newUsers.length > 0,
          }
        }),
      removeUsersByApiKeys: (apiKeys: string[]) =>
        set((state) => {
          const newUsers = state.users.filter((u) => !apiKeys.includes(u.apiKey))
          const newCurrentId =
            state.currentUserId && !newUsers.some((u) => u.id === state.currentUserId)
              ? newUsers.length > 0
                ? newUsers[0].id
                : null
              : state.currentUserId
          return {
            users: newUsers,
            currentUserId: newCurrentId,
            isAuthenticated: newUsers.length > 0,
          }
        }),
      logout: () => {
        sessionStorage.removeItem("omem-auth")
        set({
          currentUserId: null,
          isAuthenticated: false,
        })
      },
    }),
    {
      name: "omem-auth",
      storage: {
        getItem: (name) => {
          const item = sessionStorage.getItem(name)
          return item ? JSON.parse(item) : null
        },
        setItem: (name, value) => {
          sessionStorage.setItem(name, JSON.stringify(value))
        },
        removeItem: (name) => sessionStorage.removeItem(name),
      },
    }
  )
)
