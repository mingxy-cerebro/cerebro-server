import apiClient from "./client"
import type {
  PreferenceResponse,
  StatsResponse,
  InjectionResponse,
  InductionRunResponse,
  VersionResponse,
  ChangelogEntry,
  CreatePreferenceBody,
  UpdatePreferenceBody,
} from "@/types/profile-v2"

function v2Url(path: string) {
  return `/v2/profile${path}`
}

export const profileV2Api = {
  getPreferences(projectPath?: string) {
    return apiClient.get<PreferenceResponse[]>(v2Url("/preferences"), {
      params: projectPath ? { project_path: projectPath } : undefined,
    })
  },

  getPreference(id: string) {
    return apiClient.get<PreferenceResponse>(v2Url(`/preferences/${id}`))
  },

  createPreference(data: CreatePreferenceBody) {
    return apiClient.post<PreferenceResponse>(v2Url("/preferences"), data)
  },

  updatePreference(id: string, data: UpdatePreferenceBody) {
    return apiClient.put<PreferenceResponse>(v2Url(`/preferences/${id}`), data)
  },

  deletePreference(id: string) {
    return apiClient.delete(v2Url(`/preferences/${id}`))
  },

  getStats(projectPath?: string) {
    return apiClient.get<StatsResponse>(v2Url("/stats"), {
      params: projectPath ? { project_path: projectPath } : undefined,
    })
  },

  getInjection(projectPath?: string) {
    return apiClient.get<InjectionResponse>(v2Url("/inject"), {
      params: projectPath ? { project_path: projectPath } : undefined,
    })
  },

  triggerInduction(candidateTexts?: string[], projectPath?: string) {
    return apiClient.post(v2Url("/induction/trigger"), {
      candidate_texts: candidateTexts ?? [],
      project_path: projectPath,
    })
  },

  getInductionRuns(projectPath?: string) {
    return apiClient.get<InductionRunResponse[]>(v2Url("/induction/runs"), {
      params: projectPath ? { project_path: projectPath } : undefined,
    })
  },

  getVersions(projectPath?: string) {
    return apiClient.get<VersionResponse[]>(v2Url("/versions"), {
      params: projectPath ? { project_path: projectPath } : undefined,
    })
  },

  getChangelog(projectPath?: string) {
    return apiClient.get<ChangelogEntry[]>(v2Url("/changelog"), {
      params: projectPath ? { project_path: projectPath } : undefined,
    })
  },
}
