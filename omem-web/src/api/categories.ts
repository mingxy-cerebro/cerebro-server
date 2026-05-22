import apiClient from "./client"
import type { CategoryConfig, AliasResponse } from "@/types/categories"

export const categoriesApi = {
  list() {
    return apiClient.get<CategoryConfig[]>("/v1/categories")
  },
  get(name: string) {
    return apiClient.get<CategoryConfig>(`/v1/categories/${encodeURIComponent(name)}`)
  },
  create(data: Partial<CategoryConfig> & { name: string; display_name: string }) {
    return apiClient.post<CategoryConfig>("/v1/categories", data)
  },
  update(name: string, data: Partial<CategoryConfig>) {
    return apiClient.put<CategoryConfig>(`/v1/categories/${encodeURIComponent(name)}`, data)
  },
  delete(name: string) {
    return apiClient.delete(`/v1/categories/${encodeURIComponent(name)}`)
  },
  listAliases() {
    return apiClient.get<AliasResponse[]>("/v1/categories/aliases")
  },
  createAlias(alias: string, target: string) {
    return apiClient.post<AliasResponse>("/v1/categories/aliases", { alias, target })
  },
  deleteAlias(alias: string) {
    return apiClient.delete(`/v1/categories/aliases/${encodeURIComponent(alias)}`)
  },
}
