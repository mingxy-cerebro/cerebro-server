import apiClient from "./client"

export async function getSchedulerStatus() {
  return apiClient.get("/v1/scheduler/status") as Promise<{
    lifecycle: { paused: boolean; running: boolean }
    clustering: { paused: boolean; running: boolean }
  }>
}

export async function pauseLifecycle() {
  return apiClient.post("/v1/scheduler/lifecycle/pause") as Promise<{ ok: boolean; action: string }>
}

export async function resumeLifecycle() {
  return apiClient.post("/v1/scheduler/lifecycle/resume") as Promise<{ ok: boolean; action: string }>
}

export async function pauseClustering() {
  return apiClient.post("/v1/scheduler/clustering/pause") as Promise<{ ok: boolean; action: string }>
}

export async function resumeClustering() {
  return apiClient.post("/v1/scheduler/clustering/resume") as Promise<{ ok: boolean; action: string }>
}
