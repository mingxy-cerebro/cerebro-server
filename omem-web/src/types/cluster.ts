export interface Cluster {
  id: string
  tenant_id: string
  space_id: string
  title: string
  summary: string
  category: string
  member_count: number
  importance: number
  keywords: string[]
  anchor_memory_id: string
  created_at: string
  updated_at: string
  last_accessed_at?: string
}

export interface ClusteringJob {
  id: string
  tenant_id: string
  space_id: string
  status: "pending" | "running" | "completed" | "failed"
  total_memories: number
  processed_memories: number
  assigned_to_existing: number
  created_new_clusters: number
  errors: number
  started_at?: string
  completed_at?: string
  error_message?: string
  created_at: string
}

export interface ClusterMember {
  id: string
  content: string
  category: string
  importance: number
  created_at: string
}

export interface ClusterDetail {
  cluster: Cluster
  members: ClusterMember[]
}

export interface ClusteringStats {
  total_clusters: number
  total_memories_in_clusters: number
  orphaned_memories: number
  recent_jobs: ClusteringJob[]
}