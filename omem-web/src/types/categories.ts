export interface CategoryConfig {
  name: string
  display_name: string
  description: string
  decision_rule: string | null
  always_merge: boolean
  append_only: boolean
  temporal_versioned: boolean
  merge_supported: boolean
  admission_weight: number
  importance_base: number
  prompt_format: string | null
  default_visibility: string
  default_scope: string
  default_ttl_days: number | null
  sort_order: number
  is_active: boolean
}

export interface AliasResponse {
  alias: string
  target: string
}
