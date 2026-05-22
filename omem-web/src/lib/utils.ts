import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

/**
 * 脱敏显示 API Key
 * 规则：前4位 + **** + 后4位
 * 示例：c60b****75a
 */
export function maskApiKey(apiKey: string): string {
  if (!apiKey || apiKey.length < 12) return apiKey
  return `${apiKey.slice(0, 4)}****${apiKey.slice(-4)}`
}
