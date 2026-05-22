// Profile v2 API types — mirrors omem-server/src/api/handlers/profile_v2.rs DTOs

export interface PreferenceResponse {
  id: string
  slot: string
  value: string
  confidence: number
  scope: string
  project_path: string | null
  source: string
  status: string
  created_at: string
  updated_at: string
}

export interface StatsResponse {
  total: number
  by_scope: Record<string, number>
  by_status: Record<string, number>
  last_induction_at: string | null
}

export interface InjectionResponse {
  content: string
  preference_count: number
  estimated_tokens: number
}

export interface InductionRunResponse {
  id: string
  status: string
  candidate_count: number
  extracted_count: number
  error: string | null
  started_at: string
  completed_at: string | null
}

export interface VersionResponse {
  id: string
  preference_count: number
  created_at: string
}

export interface ChangelogEntry {
  id: string
  preference_id: string
  action: string
  old_value: string | null
  new_value: string | null
  source: string
  created_at: string
}

export interface CreatePreferenceBody {
  slot: string
  value: string
  confidence?: number
  scope?: string
  project_path?: string
}

export interface UpdatePreferenceBody {
  value?: string
  confidence?: number
  scope?: string
  project_path?: string
}
