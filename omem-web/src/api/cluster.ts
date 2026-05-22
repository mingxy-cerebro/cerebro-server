import apiClient from "./client"
import type { ClusteringStats, ClusteringJob, Cluster, ClusterDetail } from "@/types/cluster"

export async function getClusteringStats(): Promise<ClusteringStats> {
  return apiClient.get("/v1/clusters/stats")
}

export async function triggerClustering(spaceId?: string, batchSize?: number, mode?: string): Promise<{ job_id: string; status: string; message: string }> {
  return apiClient.post("/v1/clusters/trigger", {
    space_id: spaceId,
    batch_size: batchSize,
    mode,
  })
}

export async function listClusteringJobs(): Promise<{ jobs: ClusteringJob[] }> {
  return apiClient.get("/v1/clusters/jobs")
}

export async function getClusteringJob(jobId: string): Promise<{ job: ClusteringJob }> {
  return apiClient.get(`/v1/clusters/jobs/${jobId}`)
}

export async function deleteClusteringJob(jobId: string): Promise<void> {
  return apiClient.delete(`/v1/clusters/jobs/${jobId}`)
}

export async function listClusters(limit?: number, offset?: number): Promise<{ clusters: Cluster[]; total: number }> {
  const params: Record<string, string> = {}
  if (limit !== undefined) params.limit = String(limit)
  if (offset !== undefined) params.offset = String(offset)
  return apiClient.get("/v1/clusters", { params })
}

export async function deleteCluster(id: string): Promise<{ deleted: string; unlinked_memories: number }> {
  return apiClient.delete(`/v1/clusters/${id}`)
}

export async function getCluster(id: string): Promise<ClusterDetail> {
  return apiClient.get(`/v1/clusters/${id}`)
}

export async function batchDeleteClusters(clusterIds: string[]): Promise<{ deleted: number; unlinked_memories: number }> {
  return apiClient.post("/v1/clusters/batch-delete", { cluster_ids: clusterIds })
}

export async function deleteAllClusters(): Promise<{ deleted: number; unlinked_memories: number }> {
  return apiClient.delete("/v1/clusters/all", { timeout: 300000 })
}