import { useState, type FormEvent } from "react"
import { useNavigate } from "react-router-dom"
import axios from "axios"
import {
  Key,
  Trash2,
  Check,
  Eye,
  EyeOff,
  Brain,
  Sparkles,
  Zap,
  Shield,
  ArrowRight,
} from "lucide-react"
import { useAuthStore } from "@/stores/auth"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { AppFooter } from "@/components/layout/app-footer"
import { maskApiKey } from "@/lib/utils"

function MemoryNetwork() {
  const nodes = [
    { cx: 250, cy: 250, r: 8 },
    { cx: 160, cy: 130, r: 4 },
    { cx: 360, cy: 140, r: 3 },
    { cx: 390, cy: 290, r: 5 },
    { cx: 330, cy: 390, r: 3 },
    { cx: 170, cy: 400, r: 4 },
    { cx: 110, cy: 290, r: 3 },
    { cx: 200, cy: 80, r: 5 },
    { cx: 310, cy: 210, r: 3 },
    { cx: 140, cy: 220, r: 3 },
    { cx: 280, cy: 340, r: 3 },
    { cx: 420, cy: 240, r: 3 },
  ]

  const connections = [
    [0, 1],
    [0, 2],
    [0, 3],
    [0, 4],
    [0, 5],
    [0, 6],
    [0, 7],
    [0, 8],
    [0, 9],
    [0, 10],
    [0, 11],
    [1, 7],
    [1, 9],
    [2, 7],
    [2, 8],
    [3, 8],
    [3, 4],
    [4, 10],
    [5, 10],
    [5, 6],
    [6, 9],
    [8, 11],
    [3, 11],
  ]

  return (
    <svg viewBox="0 0 500 500" className="w-full h-full">
      <title>Memory Network</title>
      <defs>
        <radialGradient id="netGlow" cx="50%" cy="50%" r="50%">
          <stop offset="0%" stopColor="#c9a227" stopOpacity="0.18" />
          <stop offset="50%" stopColor="#c9a227" stopOpacity="0.05" />
          <stop offset="100%" stopColor="#c9a227" stopOpacity="0" />
        </radialGradient>
        <filter id="glow">
          <feGaussianBlur stdDeviation="2" result="blur" />
          <feMerge>
            <feMergeNode in="blur" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
      </defs>

      <circle cx="250" cy="250" r="210" fill="url(#netGlow)">
        <animate
          attributeName="r"
          values="210;230;210"
          dur="8s"
          repeatCount="indefinite"
        />
      </circle>

      <circle
        cx="250"
        cy="250"
        r="130"
        fill="none"
        stroke="#c9a227"
        strokeWidth="0.4"
        opacity="0.12"
      >
        <animateTransform
          attributeName="transform"
          type="rotate"
          from="0 250 250"
          to="360 250 250"
          dur="60s"
          repeatCount="indefinite"
        />
      </circle>
      <circle
        cx="250"
        cy="250"
        r="175"
        fill="none"
        stroke="#c9a227"
        strokeWidth="0.25"
        opacity="0.08"
      >
        <animateTransform
          attributeName="transform"
          type="rotate"
          from="360 250 250"
          to="0 250 250"
          dur="90s"
          repeatCount="indefinite"
        />
      </circle>

      {connections.map(([a, b], idx) => (
        <line
          key={`conn-${a}-${b}`}
          x1={nodes[a].cx}
          y1={nodes[a].cy}
          x2={nodes[b].cx}
          y2={nodes[b].cy}
          stroke="#c9a227"
          strokeWidth="0.5"
          opacity="0.25"
        >
          <animate
            attributeName="opacity"
            values="0.25;0.5;0.25"
            dur={`${3 + (idx % 4)}s`}
            repeatCount="indefinite"
          />
        </line>
      ))}

      {nodes.map((n, idx) => (
        <circle
          key={`node-${n.cx}-${n.cy}`}
          cx={n.cx}
          cy={n.cy}
          r={n.r}
          fill="#c9a227"
          opacity={idx === 0 ? 1 : 0.65}
          filter={idx === 0 ? "url(#glow)" : undefined}
        >
          <animate
            attributeName="r"
            values={`${n.r};${n.r * 1.5};${n.r}`}
            dur={`${2 + (idx % 3)}s`}
            repeatCount="indefinite"
          />
          {idx === 0 && (
            <animate
              attributeName="opacity"
              values="1;0.6;1"
              dur="3s"
              repeatCount="indefinite"
            />
          )}
        </circle>
      ))}
    </svg>
  )
}

const features = [
  { icon: Brain, label: "永久记忆" },
  { icon: Zap, label: "瞬间调取" },
  { icon: Shield, label: "隐私安全" },
]

export function LoginPage() {
  const navigate = useNavigate()
  const { users, currentUserId, addUser, setCurrentUser, removeUser } =
    useAuthStore()

  const [apiKey, setApiKey] = useState("")
  const [isLoading, setIsLoading] = useState(false)
  const [error, setError] = useState("")
  const [showPassword, setShowPassword] = useState(false)

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault()
    setError("")
    setIsLoading(true)

    try {
      const baseUrl =
        window.__OMEM_API_URL__ &&
        window.__OMEM_API_URL__ !== "__OMEM_API_URL__"
          ? window.__OMEM_API_URL__
          : window.location.origin
      const client = axios.create({
        baseURL: baseUrl,
        headers: { "X-API-Key": apiKey },
        timeout: 10000,
      })

      const healthRes = await client.get("/health")
      if (healthRes.status !== 200) {
        throw new Error("health check failed")
      }

      let spaceName = "默认空间"
      const spacesRes = await client.get("/v1/spaces")
      const spacesData = spacesRes.data
      const spaces = Array.isArray(spacesData)
        ? spacesData
        : spacesData?.spaces || []
      if (spaces.length > 0 && spaces[0].name) {
        spaceName = spaces[0].name
      }

      const existingUser = users.find((u) => u.apiKey === apiKey)
      if (existingUser) {
        setCurrentUser(existingUser.id)
        navigate("/dashboard")
        return
      }

      const newUser = {
        id: crypto.randomUUID(),
        name: spaceName,
        apiKey,
        apiUrl: baseUrl,
        lastUsed: new Date().toISOString(),
        spaceName,
        isProtected: apiKey === "c60beb98-7aab-4985-8c1d-29ffd6aff75a",
      }
      addUser(newUser)
      navigate("/dashboard")
    } catch (err) {
      setError("验证失败，请检查 API Key 是否正确")
    } finally {
      setIsLoading(false)
    }
  }

  const handleSelectUser = (user: (typeof users)[0]) => {
    setCurrentUser(user.id)
    navigate("/dashboard")
  }

  const handleRemoveUser = (e: React.MouseEvent, id: string) => {
    e.stopPropagation()
    removeUser(id)
  }

  return (
    <div className="dark min-h-screen bg-[#070708] grid lg:grid-cols-[55%_45%]">
      <div className="hidden lg:flex flex-col justify-between relative overflow-hidden p-12">
        <div
          className="absolute inset-0 opacity-[0.03]"
          style={{
            backgroundImage: `radial-gradient(circle at 1px 1px, rgba(255,255,255,0.4) 1px, transparent 0)`,
            backgroundSize: "40px 40px",
          }}
        />

        <div className="absolute inset-0 flex items-center justify-center scale-110">
          <MemoryNetwork />
        </div>

        <div className="relative z-10 flex items-center gap-3">
          <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-[#c9a227]/10 border border-[#c9a227]/20">
            <Brain className="h-5 w-5 text-[#c9a227]" />
          </div>
          <span className="text-xl font-bold tracking-tight text-white">
            omem
          </span>
        </div>

        <div className="relative z-10 space-y-8">
          <div className="space-y-3">
            <div className="inline-flex items-center gap-2 rounded-full border border-[#c9a227]/20 bg-[#c9a227]/5 px-3 py-1 text-xs font-medium text-[#c9a227]">
              <Sparkles className="h-3 w-3" />
              AI 记忆引擎
            </div>
            <h1 className="text-5xl font-bold tracking-tight text-white leading-[1.1]">
              让 AI 记住
              <br />
              <span className="text-[#c9a227]">你的一切</span>
            </h1>
            <p className="text-lg text-white/40 max-w-sm leading-relaxed">
              你的第二大脑，永远在线。
              <br />
              每一次对话，都值得被铭记。
            </p>
          </div>

          <div className="flex gap-6">
            {features.map((f) => (
              <div
                key={f.label}
                className="flex items-center gap-2 text-sm text-white/40"
              >
                <f.icon className="h-4 w-4 text-[#c9a227]/70" />
                {f.label}
              </div>
            ))}
          </div>
        </div>
      </div>

      <div className="flex flex-col min-h-screen">
        <div className="flex-1 flex items-center justify-center p-6">
          <div className="w-full max-w-md space-y-6">
            <div className="lg:hidden text-center space-y-4 mb-2">
              <div className="inline-flex h-14 w-14 items-center justify-center rounded-2xl bg-[#c9a227]/10 border border-[#c9a227]/20">
                <Brain className="h-7 w-7 text-[#c9a227]" />
              </div>
              <div>
                <h1 className="text-2xl font-bold text-white">omem</h1>
                <p className="text-sm text-white/40 mt-1">
                  让 AI 记住你的一切
                </p>
              </div>
            </div>

            <div className="rounded-2xl border border-white/[0.08] bg-white/[0.03] backdrop-blur-xl p-6 space-y-6">
              <div className="space-y-1">
                <h2 className="text-xl font-semibold text-white">
                  欢迎回来
                </h2>
                <p className="text-sm text-white/40">
                  输入 API Key 以访问您的记忆库
                </p>
              </div>

              <form onSubmit={handleSubmit} className="space-y-4">
                <div className="space-y-2">
                  <Label htmlFor="apiKey" className="text-white/70">
                    API Key
                  </Label>
                  <div className="relative">
                    <Key className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-white/30" />
                    <Input
                      id="apiKey"
                      type={showPassword ? "text" : "password"}
                      placeholder="输入您的 omem API Key"
                      value={apiKey}
                      onChange={(e) => setApiKey(e.target.value)}
                      className="pl-9 pr-10 bg-white/[0.05] border-white/10 text-white placeholder:text-white/25 focus:border-[#c9a227]/50 focus:ring-[#c9a227]/20"
                      disabled={isLoading}
                      required
                    />
                    <button
                      type="button"
                      onClick={() => setShowPassword(!showPassword)}
                      className="absolute right-3 top-1/2 -translate-y-1/2 text-white/30 hover:text-white/70 transition-colors"
                      tabIndex={-1}
                    >
                      {showPassword ? (
                        <EyeOff className="h-4 w-4" />
                      ) : (
                        <Eye className="h-4 w-4" />
                      )}
                    </button>
                  </div>
                </div>

                {error && (
                  <div className="rounded-lg bg-red-500/10 border border-red-500/20 p-3 text-sm text-red-400">
                    {error}
                  </div>
                )}

                <Button
                  type="submit"
                  disabled={isLoading || !apiKey}
                  className="w-full bg-[#c9a227] hover:bg-[#d4b43a] text-[#070708] font-semibold transition-all disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  {isLoading ? (
                    <span className="flex items-center gap-2">
                      <span className="h-4 w-4 border-2 border-[#070708]/30 border-t-[#070708] rounded-full animate-spin" />
                      验证中...
                    </span>
                  ) : (
                    <span className="flex items-center gap-2">
                      验证并登录
                      <ArrowRight className="h-4 w-4" />
                    </span>
                  )}
                </Button>
              </form>
            </div>

            {users.length > 0 && (
              <div className="rounded-2xl border border-white/[0.08] bg-white/[0.03] backdrop-blur-xl p-6 space-y-4">
                <h3 className="text-sm font-medium text-white/70">
                  已保存的账号
                </h3>
                <div className="space-y-2">
                  {users.map((user) => (
                    <div
                      key={user.id}
                      className={`group flex w-full items-center rounded-xl border transition-all overflow-hidden ${
                        currentUserId === user.id
                          ? "border-[#c9a227]/30 bg-[#c9a227]/5"
                          : "border-white/10 bg-white/[0.03] hover:border-white/20 hover:bg-white/[0.06]"
                      }`}
                    >
                      <button
                        type="button"
                        onClick={() => handleSelectUser(user)}
                        className="flex flex-1 items-center gap-3 p-3 text-left min-w-0"
                      >
                        <div
                          className={`flex h-9 w-9 shrink-0 items-center justify-center rounded-lg transition-colors ${
                            currentUserId === user.id
                              ? "bg-[#c9a227] text-[#070708]"
                              : "bg-white/10 text-white/40"
                          }`}
                        >
                          {currentUserId === user.id ? (
                            <Check className="h-4 w-4" />
                          ) : (
                            <Key className="h-4 w-4" />
                          )}
                        </div>
                        <div className="space-y-0.5 min-w-0">
                          <p className="text-sm font-medium text-white/90 truncate">
                            {user.spaceName || user.name}
                          </p>
                          <p className="text-xs text-white/35">
                            {maskApiKey(user.apiKey)}
                          </p>
                        </div>
                      </button>
                      <div className="p-2 opacity-0 transition-opacity group-hover:opacity-100">
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon"
                          onClick={(e) => {
                            e.stopPropagation()
                            handleRemoveUser(e, user.id)
                          }}
                          disabled={user.isProtected}
                          className="h-8 w-8 disabled:opacity-0 disabled:cursor-not-allowed text-white/40 hover:text-red-400 hover:bg-red-500/10"
                          title={
                            user.isProtected
                              ? "受保护的账号无法删除"
                              : "删除账号"
                          }
                        >
                          <Trash2 className="h-3.5 w-3.5" />
                        </Button>
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </div>
        </div>

        <AppFooter />
      </div>
    </div>
  )
}
