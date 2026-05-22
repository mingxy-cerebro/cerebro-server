import { cn } from "@/lib/utils"

export const PRIVATE_TAG = "私密"

const TAG_COLOR_MAP: Record<string, { bg: string; text: string; border: string }> = {
  工作:    { bg: "bg-blue-500/10",  text: "text-blue-600",  border: "border-blue-500/30" },
  学习:    { bg: "bg-green-500/10", text: "text-green-600", border: "border-green-500/30" },
  生活:    { bg: "bg-rose-500/10",  text: "text-rose-600",  border: "border-rose-500/30" },
  技术:    { bg: "bg-violet-500/10",text: "text-violet-600",border: "border-violet-500/30" },
  重要:    { bg: "bg-red-500/10",   text: "text-red-600",   border: "border-red-500/30" },
  待办:    { bg: "bg-orange-500/10",text: "text-orange-600",border: "border-orange-500/30" },
  项目:    { bg: "bg-indigo-500/10",text: "text-indigo-600",border: "border-indigo-500/30" },
  灵感:    { bg: "bg-pink-500/10",  text: "text-pink-600",  border: "border-pink-500/30" },
  笔记:    { bg: "bg-slate-500/10", text: "text-slate-600", border: "border-slate-500/30" },
  私密:    { bg: "bg-amber-500/10", text: "text-amber-600", border: "border-amber-500/30" },
}

const FALLBACK_PALETTE = [
  { bg: "bg-cyan-500/10",    text: "text-cyan-600",    border: "border-cyan-500/30" },
  { bg: "bg-emerald-500/10", text: "text-emerald-600", border: "border-emerald-500/30" },
  { bg: "bg-teal-500/10",    text: "text-teal-600",    border: "border-teal-500/30" },
  { bg: "bg-sky-500/10",    text: "text-sky-600",    border: "border-sky-500/30" },
  { bg: "bg-fuchsia-500/10",text: "text-fuchsia-600", border: "border-fuchsia-500/30" },
  { bg: "bg-lime-500/10",   text: "text-lime-600",   border: "border-lime-500/30" },
  { bg: "bg-yellow-500/10", text: "text-yellow-600", border: "border-yellow-500/30" },
]

function stableHash(str: string): number {
  let h = 0
  for (let i = 0; i < str.length; i++) {
    h = ((h << 5) - h + str.charCodeAt(i)) | 0
  }
  return Math.abs(h)
}

export function getTagColors(tag: string) {
  const preset = TAG_COLOR_MAP[tag]
  if (preset) return preset
  const idx = stableHash(tag) % FALLBACK_PALETTE.length
  return FALLBACK_PALETTE[idx]
}

export function getTagClassName(tag: string, extra?: string) {
  const c = getTagColors(tag)
  return cn(
    c.bg,
    c.text,
    c.border,
    "border",
    "hover:opacity-80",
    extra
  )
}

export function isPrivateMemory(tags?: string[], visibility?: string): boolean {
  if (visibility === "private") return true
  return tags?.includes(PRIVATE_TAG) || false
}

export function getCategoryBadgeClass(category?: string): string {
  switch (category?.toLowerCase()) {
    case "profile":     return "bg-violet-500/10 text-violet-600 dark:text-violet-400 border-violet-500/30"
    case "preferences": return "bg-rose-500/10 text-rose-600 dark:text-rose-400 border-rose-500/30"
    case "entities":    return "bg-blue-500/10 text-blue-600 dark:text-blue-400 border-blue-500/30"
    case "patterns":    return "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border-emerald-500/30"
    case "cases":       return "bg-amber-500/10 text-amber-600 dark:text-amber-400 border-amber-500/30"
    case "events":      return "bg-sky-500/10 text-sky-600 dark:text-sky-400 border-sky-500/30"
    case "decisions":   return "bg-indigo-500/10 text-indigo-600 dark:text-indigo-400 border-indigo-500/30"
    default:            return "bg-muted text-muted-foreground border-border"
  }
}

export function getCategoryLabel(category?: string): string {
  switch (category?.toLowerCase()) {
    case "profile":     return "画像"
    case "preferences": return "偏好"
    case "entities":    return "实体"
    case "patterns":    return "模式"
    case "cases":       return "案例"
    case "events":      return "事件"
    case "decisions":   return "决策"
    default:            return category || "—"
  }
}

export function getTierLabel(tier?: string): string {
  switch (tier?.toLowerCase()) {
    case "core":       return "核心"
    case "working":    return "工作区"
    case "peripheral": return "边缘"
    default:           return tier || "—"
  }
}

export function getTierVariant(tier?: string): "default" | "secondary" | "outline" {
  switch (tier?.toLowerCase()) {
    case "core":       return "default"
    case "working":    return "secondary"
    case "peripheral": return "outline"
    default:           return "outline"
  }
}

export function getTierBadgeClass(tier?: string): string {
  switch (tier?.toLowerCase()) {
    case "core":
      return "bg-amber-500/10 text-amber-600 dark:text-amber-400 border-amber-500/30"
    case "working":
      return "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border-emerald-500/30"
    case "peripheral":
      return "bg-slate-500/10 text-slate-500 dark:text-slate-400 border-slate-500/30"
    default:
      return "bg-muted text-muted-foreground border-border"
  }
}
