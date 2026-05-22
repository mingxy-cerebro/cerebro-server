import axios from "axios"
import type { AxiosInstance } from "axios"
import { useAuthStore } from "@/stores/auth"

declare global {
  interface Window {
    __OMEM_API_URL__?: string
  }
}

const apiBaseUrl =
  typeof window !== "undefined" &&
  window.__OMEM_API_URL__ &&
  window.__OMEM_API_URL__ !== "__OMEM_API_URL__"
    ? window.__OMEM_API_URL__
    : "/"

const apiClient = axios.create({
  baseURL: apiBaseUrl,
  timeout: 30000,
  headers: {
    "Content-Type": "application/json",
  },
}) as AxiosInstance

apiClient.interceptors.request.use((config) => {
  const currentUser = useAuthStore.getState().users.find(
    (u) => u.id === useAuthStore.getState().currentUserId
  )
  if (currentUser?.apiKey) {
    config.headers["X-API-Key"] = currentUser.apiKey
    config.headers["X-Agent-ID"] = "omem-web"
  }
  return config
})

apiClient.interceptors.response.use(
  (response) => response.data,
  (error) => {
    if (error.response?.status === 401) {
      useAuthStore.getState().logout()
      window.location.href = "/login"
    }
    return Promise.reject(error)
  }
)

export default apiClient
