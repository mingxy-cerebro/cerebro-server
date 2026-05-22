declare module 'sonner' {
  import * as React from 'react'

  interface ToastOptions {
    description?: string
    duration?: number
  }

  export const Toaster: React.FC<{
    position?: 'top-left' | 'top-right' | 'bottom-left' | 'bottom-right' | 'top-center' | 'bottom-center'
    richColors?: boolean
    closeButton?: boolean
  }>

  export function toast(message: string, options?: ToastOptions): string
  export namespace toast {
    function success(message: string, options?: ToastOptions): string
    function error(message: string, options?: ToastOptions): string
    function info(message: string, options?: ToastOptions): string
    function warning(message: string, options?: ToastOptions): string
  }
}
