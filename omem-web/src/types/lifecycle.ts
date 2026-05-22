/** Tier 衰减参数 */
export interface TierDecayParams {
  beta: number
  floor: number
}

/** 衰减配置 — 匹配后端 GET /v1/stats/config 返回结构 */
export interface DecayConfig {
  half_life_days: number
  importance_modulation: number
  recency_weight: number
  frequency_weight: number
  intrinsic_weight: number
  tiers: Record<string, TierDecayParams>
}

/** 升降级阈值 */
export interface TierThreshold {
  min_access_count: number
  min_composite: number
  min_importance?: number
}

/** 升级/降级配置 */
export interface PromotionConfig {
  peripheral_to_working: TierThreshold
  working_to_core: TierThreshold
}

/** 生命周期完整配置 */
export interface LifecycleConfig {
  decay: DecayConfig
  promotion: PromotionConfig
}

/** 衰减曲线数据点 */
export interface DecayDataPoint {
  day: number
  score: number
}

/** 衰减曲线响应 */
export interface DecayCurveResponse {
  memory_id: string
  points: DecayDataPoint[]
}

/** 升降级变更记录 */
export interface TierChange {
  memoryId: string
  memoryTitle: string
  from: string
  to: string
  reason: string
  at: string
  accessCount: number
}

export interface TierChangesResponse {
  changes: TierChange[]
  totalCount: number
}
