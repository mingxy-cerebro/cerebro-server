# 🧪 omem Plugin v0.1.5 测试指南

> 测试环境配置 | 功能逐项验证 | 预期结果对照

---

## ⚙️ 环境配置（请先确认）

```jsonc
// ~/.opencode/config.jsonc
{
  "plugins": [
    "@mingxy/omem@0.1.5"
  ]
}
```

**Server状态**：https://www.mengxy.cc （API Key已配置）
**插件版本**：`@mingxy/omem@0.1.5`
**重启方式**：修改配置后务必重启 OpenCode

---

## ✅ 测试项清单

### 1. 🔧 新工具调用测试

依次让AI执行以下指令，验证工具是否注册成功：

**A. memory_profile — 查看用户画像**
```
查看我的记忆画像
```
> **预期**：返回profile记忆汇总（如有），或提示暂无profile

**B. memory_list — 列出记忆**
```
列出我最近的5条记忆
```
> **预期**：返回记忆列表，含id/content/tags/created_at

**C. memory_stats — 记忆统计**
```
我的记忆库统计情况如何？
```
> **预期**：返回总记忆数、各分类数量、时间分布

**D. memory_ingest — 手动摄入记忆**
```
将以下内容记录到记忆库：
我刚刚学会了使用omem的记忆管理功能，体验很棒。
```
> **预期**：Toast提示"✨ Memory Ingested"，smart模式下生成L0/L1/L2

---

### 2. 🤖 AutoCapture 自动捕获测试

**测试方法**：正常对话即可，观察右下角Toast

**建议测试话术**（触发事件/案例/模式识别）：
```
我遇到一个问题：Python的asyncio事件循环老是报RuntimeError，
解决方案是改用asyncio.new_event_loop()配合set_event_loop()。

这个方案帮我解决了问题，记录下来以后参考。
```
> **预期**：对话结束后，Toast弹出"✨ Memories Captured (N)"

**验证摄入结果**：
```
查看刚才自动捕获的记忆
```

---

### 3. 🔒 私密内容自动标记测试

**测试方法**：输入含私密信息的对话

**建议测试话术**：
```
我的服务器密码是 Mengfanbo@0714，IP是47.93.199.242，
root用户，用于部署omem服务。这是私密信息请妥善保管。
```
> **预期**：
> 1. Toast弹出"🔒 Private memories saved to Vault"
> 2. 记忆tags包含"私密"
> 3. 该记忆不会出现在普通list中（或带有vault标识）

**验证方式**：
```
查看我的私密记忆
```

---

### 4. 🌐 英文 Toast 显示测试

**测试方法**：观察所有Toast提示文字

| 场景 | 预期英文Toast |
|------|--------------|
| Smart模式捕获成功 | ✨ Memories Captured (N) |
| Raw模式保存成功 | 📝 Memories Saved (N) |
| 私密内容捕获 | 🔒 Private memories saved to Vault |
| Ingest手动摄入 | ✨ Memory Ingested |
| 摄入失败 | ❌ Failed to capture memories |

---

### 5. 📊 Dynamic Context 生成测试

**前置条件**：需有Events/Cases/Patterns分类的记忆（7天内）

**测试方法**：
```
根据我之前的经验，我现在遇到了一个新问题...
```
> **预期**：如有符合条件的记忆，AI会收到dynamic_context并参考

**快速产生测试数据**：
```
记录一个案例：
问题：服务器磁盘满了
解决：用du -sh /*排查，发现是日志文件，用logrotate配置轮转
分类：cases
```

---

## 📋 测试结果记录表

| # | 测试项 | 状态 | 备注 |
|---|--------|------|------|
| 1 | memory_profile | ⬜ | |
| 2 | memory_list | ⬜ | |
| 3 | memory_stats | ⬜ | |
| 4 | memory_ingest | ⬜ | |
| 5 | AutoCapture Smart | ⬜ | |
| 6 | AutoCapture Raw | ⬜ | |
| 7 | 私密内容标记 | ⬜ | |
| 8 | 英文Toast | ⬜ | |
| 9 | Dynamic Context | ⬜ | |

---

## 🐛 已知问题

1. **Smart模式字段丢失**：source/agent_id/session_id 目前为空（server端bug）
2. **Dynamic Context为空**：需先产生Events/Cases/Patterns分类的记忆（非profile/preferences）
3. **L3记忆**：当前server未实现，只有L0/L1/L2

---

## 🔗 相关链接

- Web管理端：https://www.mengxy.cc
- Server源码：`/mnt/d/dev/github/project/omem-server`
- Plugin源码：`/mnt/d/dev/github/project/omem/opencode-plugin`

---

> 💡 **提示**：测试过程中如遇问题，可随时要求AI查看server日志排查。
> 日志位置：`/mnt/d/dev/github/project/omem-server/logs/`
