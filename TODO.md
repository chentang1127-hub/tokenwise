# TokenWise — 当前进度 & 待办

> 最后更新：2026-06-22

---

## 现在在哪？

**v0.6.0** — 核心功能全部完成，P1-P5 全部交付，生产可用。

✅ 编译通过（0 warnings）
✅ 51 个测试全过（36 单元 + 15 集成）
✅ 51 项功能全部完成

---

## 下一步做什么？

### 🟢 如果你要上线发布

- [ ] 更新 README 里的"48 tests"→"51 tests"
- [ ] 清理 `tokenwise.db` 里的真实数据（如果要公开仓库）
- [ ] 决定是否重命名（TokenWise Core vs 其他名字）
- [ ] 发布到 Docker Hub

### 🟡 如果你要继续加功能

- [ ] 看 [PLAN.md](PLAN.md) 第四章 — Phase 4 功能补齐（OpenRouter provider、预算上限等）
- [ ] Alert 通知功能（竞品有，我们没有）

### 🔵 如果你想学习代码

- [ ] 先看 [STRUCTURE.md](STRUCTURE.md) — 了解每个文件干什么
- [ ] 从 `src/main.rs` 开始读 — 程序的入口
- [ ] 然后看 `src/proxy/server.rs` — 核心：请求怎么被处理的

---

## 最近一次改动

| 日期 | 做了什么 |
|------|---------|
| 6/22 | P5 全部完成：Try It 流式响应、用量告警、数据导出、多 Key 管理、Settings 页面 |
| 6/21 | P3+P4 完成：Docker 部署、systemd、env override、Backup/Status CLI |
| 6/20 | P1+P2 完成：Webhook 接入 proxy、多租户过滤、gRPC、流式安全兜底 |

---

## 常用命令

```bash
# 编译 & 运行
cargo build --release                    # 编译
cargo run --release -- start              # 启动（proxy :9401 + dashboard :9400）
cargo run --release -- --market cn start  # 中国版配置启动

# 测试
cargo test              # 跑全部 51 个测试
cargo clippy            # 代码检查

# 工具
cargo run --release -- validate              # 检查配置文件对不对
cargo run --release -- backup --output ./bk  # 备份数据库
cargo run --release -- status                # 查看运行状态
```

---

## 改代码工作流（小白版）

每次改代码时，照这个流程走：

```
1. 打开 TODO.md（就是这个文件）→ 确定这次要改什么
2. 打开 STRUCTURE.md → 找到要改的文件在哪里
3. 告诉 AI：要改的功能 + 涉及的文件 + 让它先读代码再动手
4. 改完 → cargo test 跑测试
5. 测试通过 → git commit 存档
6. 更新这个 TODO.md → 记录改了什么
```
