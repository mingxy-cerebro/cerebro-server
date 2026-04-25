# Sub-project A: Backend Quick Fixes

## 概述
5个后端快速修复，半天内完成。

## A1: DELETE /v1/clusters/jobs/{id}
- **问题**: clustering失败任务无法删除，堆积在列表中
- **方案**: 新增DELETE端点，删除job记录
- **文件**: clusters.rs + router.rs + handlers/mod.rs
- **已有参考**: `delete_cluster` handler (删除cluster本身)

## A2: 私密记忆修复
- **问题**: 之前844条记忆全是visibility=global，含身份证等敏感信息
- **方案**: 用确定性隐私检测脚本重新扫描并标记
- **执行方式**: SSH到服务器，用curl调用batch-visibility API

## A3: POST /v1/lifecycle/trigger
- **问题**: lifecycle只能等定时器触发，无法手动执行
- **方案**: 新增POST端点，立即触发一次lifecycle cycle
- **文件**: lifecycle handler + router + mod导出
- **参考**: trigger_clustering handler的模式

## A4: 定时器改每天0点
- **问题**: 当前6小时间隔，不直观
- **方案**: 改为每天0点执行（midnight scheduling）
- **文件**: lifecycle/scheduler.rs
- **方案**: 计算到下一个midnight的间隔，用tokio::time::sleep_until

## A5: 记忆批量删除前端对接
- **问题**: 后端batch_delete API已存在，前端未对接
- **方案**: memory-list.tsx加勾选+批量删除按钮
- **已有API**: POST /v1/memories/batch-delete {ids: [...]}
- **前端参考**: 现有单条删除的AlertDialog模式
