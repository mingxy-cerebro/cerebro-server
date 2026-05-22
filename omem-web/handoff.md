# Omem-Web Handoff 交接文档

> 生成时间：2026-04-19
> 交接人：上一窗口
> 接收人：新窗口

---

## 1. 新测试 API Key

**已生成测试 Key：** `bd1f89a4-b50e-4327-863b-872269f8df7d`

- 接口：`POST https://www.mengxy.cc/v1/tenants`
- 请求体：`{"name":"任意名称"}`
- 无需认证，直接调用即可生成
- **师尊的受保护 Key：** `c60beb98-7aab-4985-8c1d-29ffd6aff75a`（永远不允许删除）

验证命令：
```bash
curl -H "X-API-Key: bd1f89a4-b50e-4327-863b-872269f8df7d" https://www.mengxy.cc/v1/spaces
```

---

## 2. 当前代码状态

### 未提交修改（5 个文件）

| 文件 | 修改内容 | 状态 |
|------|---------|------|
| `src/components/layout/app-layout.tsx` | 新增密码检测逻辑（第14-28行），但**编译报错** | 待修复 |
| `src/stores/auth.ts` | 用户认证状态 | 正常 |
| `src/views/auth/login.tsx` | 登录页 | 正常 |
| `src/views/memories/memory-list.tsx` | 记忆列表 | 正常 |
| `src/views/spaces/spaces.tsx` | 空间管理，新增成员信息展示 | 正常 |

### 编译报错详情

**文件：** `src/components/layout/app-layout.tsx`
**问题：** `toast.warning()` 的 `action` 属性不被 sonner v2.0.7 支持
**代码位置：** 第19-25行
```tsx
toast.warning("为了您的隐私安全，请先设置私密密码", {
  duration: 10000,
  action: {   // ← 报错：sonner 不支持 action 属性
    label: "去设置",
    onClick: () => navigate("/vault"),
  },
})
```
**解决方案：** 改用 `toast.custom((id) => JSX)` 自定义内容，或改用 `<Link>` + description 方式。

---

## 3. 待办事项清单（按优先级排序）

### P0 - 阻塞问题

- [ ] **修复 app-layout.tsx 编译报错**
  - 重写 toast 调用，移除不支持的 `action` 属性
  - 使用 `toast.custom()` 或 `<Link>` 实现跳转按钮

### P1 - 高优先级功能

- [ ] **初始密码检测 Toast 引导**
  - 文件：`src/components/layout/app-layout.tsx`
  - 逻辑：应用加载时调用 `checkStatus()`，若 `hasPassword: false` 则弹出 toast
  - Toast 文案："为了您的隐私安全，请先去设置私密密码"
  - 需要可点击的链接跳转到 `/vault`
  - 注意：只在用户已登录状态下检测，避免未登录时弹窗

- [ ] **Vault 解锁时检测未设置密码**
  - 文件：`src/views/memories/memory-detail.tsx` 中的 `VaultUnlock` 组件（第75-124行）
  - 或 `src/views/vault/vault-memories.tsx` 中的解锁逻辑
  - 逻辑：用户点击"解锁"输入密码后，如果 `hasPassword === false`，提示"您还没设置密码，请先设置密码"
  - 当前 `VaultUnlock` 组件第78行有 `isFirstTime` 判断，但逻辑是引导设置密码而非提示先设置

- [ ] **用户画像私密记忆支持点击查看**
  - 文件：`src/views/profile/profile-page.tsx`
  - 当前：私密内容显示为 `🔒 私密记忆 · 已加密`（第175-177行），不可点击
  - 目标：改为可点击，弹出 VaultUnlock 输入密码后显示真实内容
  - 需要引入 `useVaultStore` 和 `VaultUnlock` 组件逻辑
  - 注意：profile-page 当前没有 Vault 解锁 UI

- [ ] **修复记忆详情来源信息字段为空**
  - 文件：`src/views/memories/memory-detail.tsx` 第468-471行
  - 显示字段：source、scope、agent_id、session_id
  - 当前全部显示为 "—"
  - **根因分析：**
    1. 后端 `Memory` 结构体可能未返回这些字段
    2. plugin 创建记忆时可能只传了 `source`，未传 `agent_id`/`session_id`
    3. 需检查后端 API `/v1/memories/{id}` 的实际返回数据
  - 师尊已多次反馈此问题，截图显示来源信息均为空
  - **排查步骤：**
    1. 先用浏览器 DevTools 或 curl 查看实际 API 返回
    2. 如果后端返回有数据但前端未显示 → 前端字段映射问题
    3. 如果后端返回无数据 → 检查后端 `Memory` struct 和 plugin 上报逻辑

### P2 - 测试验证

- [ ] **测试不同 API Key 登录切换**
  - 使用新生成的 Key：`bd1f89a4-b50e-4327-863b-872269f8df7d`
  - 验证多账号切换是否正常
  - 验证空间管理创建用户逻辑（API Key 由后端生成）

---

## 4. 关键文件路径

```
src/components/layout/app-layout.tsx      # 布局 + 密码检测（编译报错）
src/stores/vault.ts                       # Vault 状态管理（服务端 API）
src/stores/auth.ts                        # 用户认证状态
src/views/auth/login.tsx                  # 登录页
src/views/memories/memory-detail.tsx      # 记忆详情（来源信息问题 + VaultUnlock）
src/views/memories/memory-list.tsx        # 记忆列表
src/views/profile/profile-page.tsx        # 用户画像（私密内容需支持解锁）
src/views/spaces/spaces.tsx               # 空间管理
src/views/vault/vault-memories.tsx        # Vault 页面（如有解锁逻辑）
src/providers/toast-provider.tsx          # Toast 提供者
```

---

## 5. 技术约束

- **绝不**在服务器上编译 Rust 代码（会卡死 ECS）
- 本地 WSL 编译后端后 `scp` 上传到 `/opt/omem/omem-server`
- 前端构建后部署到 `/var/www/omem-web`
- Toast 使用 sonner v2.0.7，支持 `toast.custom((id) => JSX)`
- Vault 状态检测接口：`GET /v1/vault/status`（返回 `{has_password: boolean}`）
- 私密记忆标签：tags 包含 "私密"
- 私密筛选参数：`tags=私密`（visibility 字段后端实现不完善）

---

## 6. 环境信息

- **后端服务器：** 47.93.199.242
- **服务名：** omem（systemd）
- **前端部署：** `/var/www/omem-web`
- **后端部署：** `/opt/omem/omem-server`
- **本地后端代码：** `/mnt/d/dev/github/project/omem-server-source/`

---

## 7. 上一窗口已完成的工作

- 修复 6 个 P0 级 bug（搜索、Vault 安全、隐私筛选分页、默认值、性能、双重 useEffect）
- 修复 3 个 P1 级问题（用户画像脱敏、entities 归类、合并 useEffect）
- 完成 3 个 P2 优化（统一 Vault 状态、Vault 服务端存储、记忆列表体验）
- 后端新增 `GET /v1/tenants/{id}` 接口
- 前端 spaces.tsx 增加成员信息展示（name、created_at）
- 修复后端文件句柄限制（LimitNOFILE=65535）
- 前端构建并部署到服务器

---

*法器铸到一半，劳烦师弟师妹接手续炼。师尊在上，万不可懈怠。*
