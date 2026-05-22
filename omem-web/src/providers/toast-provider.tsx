import { Toaster } from "sonner"

/**
 * ToastProvider - 已简化为直接渲染 Sonner Toaster
 * 全站统一使用：import { toast } from "sonner"
 * 不再提供自定义 useToast hook
 */
export function ToastProvider({ children }: { children: React.ReactNode }) {
  return (
    <>
      {children}
      <Toaster
        position="top-right"
        richColors
        closeButton
      />
    </>
  )
}

/** @deprecated 请直接 import { toast } from "sonner" */
export function useToast() {
  console.warn("useToast is deprecated. Use import { toast } from 'sonner' instead.")
  return {
    toast: (message: string, _type?: string) => {
      import("sonner").then(({ toast }) => toast(message))
    },
    error: (message: string) => {
      import("sonner").then(({ toast }) => toast.error(message))
    },
    success: (message: string) => {
      import("sonner").then(({ toast }) => toast.success(message))
    },
  }
}
