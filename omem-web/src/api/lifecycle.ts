import apiClient from "./client"
import type { LifecycleConfig, DecayCurveResponse, TierChangesResponse } from "@/types/lifecycle"

export async function getLifecycleConfig(): Promise<LifecycleConfig> {
  return apiClient.get("/v1/stats/config")
}

export async function getDecayCurve(memoryId: string): Promise<DecayCurveResponse> {
  return apiClient.get("/v1/stats/decay", { params: { memory_id: memoryId } })
}

export async function getTierChanges(limit = 50): Promise<TierChangesResponse> {
  return apiClient.get("/v1/tier-changes", { params: { limit } })
}

export async function deleteTierChanges(): Promise<{ deleted: number }> {
  return apiClient.post("/v1/tier-changes/delete")
}

export async function triggerLifecycle(): Promise<{ status: string; message: string }> {
  return apiClient.post("/v1/lifecycle/trigger", {})
}
