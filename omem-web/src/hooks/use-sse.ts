import { useEffect, useRef, useCallback } from "react"
import { useAuthStore } from "@/stores/auth"

export interface ServerEvent {
  event_type: string
  tenant_id: string
  data: Record<string, unknown>
  timestamp: string
}

type EventHandler = (event: ServerEvent) => void

let globalEventSource: EventSource | null = null
let globalHandlers: Map<string, Set<EventHandler>> = new Map()
let globalWildcardHandlers: Set<EventHandler> = new Set()
let reconnectTimer: ReturnType<typeof setTimeout> | null = null

function getSSEUrl(): string | null {
  const currentUser = useAuthStore.getState().users.find(
    (u) => u.id === useAuthStore.getState().currentUserId
  )
  if (!currentUser?.apiKey) return null
  return `/v1/events?api_key=${encodeURIComponent(currentUser.apiKey)}&tenant_id=${encodeURIComponent(currentUser.apiKey)}`
}

function connect() {
  if (globalEventSource) return

  const url = getSSEUrl()
  if (!url) return

  globalEventSource = new EventSource(url)

  globalEventSource.onmessage = (e) => {
    try {
      const event: ServerEvent = JSON.parse(e.data)
      const handlers = globalHandlers.get(event.event_type)
      if (handlers) {
        handlers.forEach((h) => h(event))
      }
      globalWildcardHandlers.forEach((h) => h(event))
    } catch {
      // ignore parse errors
    }
  }

  globalEventSource.onerror = () => {
    globalEventSource?.close()
    globalEventSource = null
    if (reconnectTimer) clearTimeout(reconnectTimer)
    reconnectTimer = setTimeout(connect, 5000)
  }
}

function disconnect() {
  if (reconnectTimer) {
    clearTimeout(reconnectTimer)
    reconnectTimer = null
  }
  globalEventSource?.close()
  globalEventSource = null
}

export function useSSE(eventType: string, handler: EventHandler) {
  const handlerRef = useRef(handler)
  handlerRef.current = handler

  useEffect(() => {
    if (!globalHandlers.has(eventType)) {
      globalHandlers.set(eventType, new Set())
    }
    const wrapped = ((e: ServerEvent) => handlerRef.current(e)) as EventHandler
    globalHandlers.get(eventType)!.add(wrapped)

    connect()

    return () => {
      globalHandlers.get(eventType)?.delete(wrapped)
      if (globalHandlers.get(eventType)?.size === 0) {
        globalHandlers.delete(eventType)
      }
      if (globalHandlers.size === 0 && globalWildcardHandlers.size === 0) {
        disconnect()
      }
    }
  }, [eventType])
}

export function useSSEAll(handler: EventHandler) {
  const handlerRef = useRef(handler)
  handlerRef.current = handler

  useEffect(() => {
    const wrapped = ((e: ServerEvent) => handlerRef.current(e)) as EventHandler
    globalWildcardHandlers.add(wrapped)
    connect()

    return () => {
      globalWildcardHandlers.delete(wrapped)
      if (globalHandlers.size === 0 && globalWildcardHandlers.size === 0) {
        disconnect()
      }
    }
  }, [])
}

export function useSSEConnected() {
  const check = useCallback(() => globalEventSource?.readyState === EventSource.OPEN, [])
  return check
}
