# Phase 1 增强版：灵魂低语三层架构

## 概述

将当前的静态 SOUL_WHISPER（system prompt 注入）升级为完整三层架构。**所有注入走消息流（synthetic text part），不走 system prompt**。

## 注入机制：synthetic text part（已验证有效）

`fetchPolicyNudgeHook` 已在线上使用此方式——月儿每次回复都能看到 FETCH_POLICY_NUDGE 的内容，证明 synthetic text part 注入**有效且不会被忽略**。

```typescript
// 已有的 fetchPolicyNudgeHook 实现方式
const syntheticPart = {
  id: nudgeId,
  type: "text" as const,
  text: "<cerebro-system-reminder>...</cerebro-system-reminder>",
  synthetic: true,
};
userMsg.parts.splice(textPartIdx, 0, syntheticPart);
```

## 三层架构

### Layer 1：精准工具拦截（tool.execute.before + messages.transform）

**两个hook配合**：
1. `tool.execute.before`：记录触发的工具名到 session Map
2. `experimental.chat.messages.transform`：消费工具记录，生成针对性 synthetic part

**白名单 + 针对性提醒**：

| 工具 | 提醒内容 |
|------|---------|
| glob | "即将搜索文件路径 — 先检查 <cerebro-context> 中是否有已知路径" |
| grep | "即将搜索代码 — 先检查记忆中的代码位置记录" |
| bash | "即将执行命令 — 检查记忆中是否有环境特定的命令（如WSL用PowerShell）" |
| playwright_* | "即将操作浏览器 — 检查记忆中是否有相关URL或页面信息" |

### Layer 2：通用兜底（messages.transform）

始终注入通用记忆提醒（synthetic text part），替换当前 system prompt 中的 SOUL_WHISPER。

### Layer 3：回复前轻提醒（messages.transform）

在 FETCH_POLICY_NUDGE 之后追加简短提醒。

## 最终注入顺序（experimental.chat.messages.transform）

```
用户消息 parts（splice 到 textPartIdx 位置，原始文本前）：
  1. [synthetic] <cerebro-system-reminder> FETCH_POLICY ...  （已有）
  2. [synthetic] <cerebro-soul-whisper> [精准] ...  （Layer 1，条件触发）
  3. [synthetic] <cerebro-soul-whisper> 通用提醒 ...  （Layer 2，始终注入）
  4. [原始text] 用户实际输入 ...
```

## 修改文件

| 文件 | 改动 |
|------|------|
| `plugins/opencode/src/hooks.ts` | 1) 新增 `soulWhisperToolTracker`（tool.execute.before）<br>2) SOUL_WHISPER 从 system.push 改为 synthetic part<br>3) fetchPolicyNudgeHook 增加三层注入 |
| `plugins/opencode/src/index.ts` | 注册 `tool.execute.before` hook |
| `plugins/opencode/src/config.ts` | 新增 soulWhisper 配置项 |

## 配置（opencode.json plugin_config）

```jsonc
{
  "@mingxy/cerebro": {
    "soulWhisper": {
      "enabled": true,
      "layer1": { "enabled": true, "tools": ["glob", "grep", "bash", "playwright_browser_navigate"] },
      "layer2": { "enabled": true },
      "layer3": { "enabled": true }
    }
  }
}
```

## 风险

极低。只改 plugin 代码，不碰服务端。synthetic part 注入方式已在线上验证。
