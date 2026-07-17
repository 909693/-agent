# 祈愿 (RETL) — AI 长篇小说创作助手

祈愿是一款面向中文网文创作的 AI 辅助写作桌面应用。它把长篇小说的创作拆成可控的三个阶段，从世界观、角色、大纲一路推进到章节正文，并用 RAG 上下文、伏笔追踪与一致性检查解决长篇写作最头疼的"前后矛盾、越写越崩"问题。

## 主要功能

### 三阶段框架生成
基于一句设定（题材 / 基调 / 主题），自动依次生成：
- **世界观** (`world.json`) — 地理、规则、势力、历史
- **角色** (`characters.json`) — 性格、成长弧线、人物关系
- **剧情大纲** (`plot.json`) — 幕 → 章节的层级结构
- **时间线** (`timeline.json`) — 事件时序

### 章节写作
多种写作模式，覆盖从零到成稿的全流程：
- **填充** — 按大纲生成章节
- **扩写** — 基于用户草稿展开
- **续写** — 从光标处接着写
- **局部重写** — 只重写选中片段

### 长篇一致性保障
- **RAG 上下文** — 智能窗口（前 3 章 + 最近 10 章），避免上下文膨胀
- **角色状态追踪** — 提取每章的位置、伤势、情绪变化，防止"死人复活"
- **伏笔系统** — 追踪已埋 / 已回收的伏笔
- **一致性检查** — 扫描全书章节摘要，检测角色状态矛盾、设定冲突、未回收伏笔
- **批量生成** — 可取消、带实时进度的多章连续生成

### 创作增强
- **读者视角模拟** — 预判读者阅读体验
- **风格学习** — 学习并延续既定文风
- **多线叙事管理** — 管理并行剧情线
- **敏感内容检测** — 本地正则 + AI 双层检测（适配番茄 / 起点 / 晋江平台规则）
- **版本快照** — 手动保存章节历史版本
- **暗色主题 · 全文搜索**

### 可扩展插件系统
- **Skills** — Git 仓库形式的提示词模板 / 创作规则
- **MCP 服务器** — 接入 Model Context Protocol 外部工具

## 技术栈

| 层 | 技术 |
| --- | --- |
| 桌面框架 | [Tauri 2](https://tauri.app/) |
| 后端 | Rust |
| 前端 | React 19 + TypeScript 5.8 + Vite 7 |
| 图标 | lucide-react |
| 文档解析 | mammoth (docx) |
| LLM | 统一客户端，支持 OpenAI / Anthropic / Gemini 三种 API 格式 |
| 密钥存储 | 系统 keyring（不明文落盘） |

## 快速开始

```bash
# 安装依赖
npm install

# 开发模式（同时启动前端 + Rust 后端）
npm run tauri dev

# 构建生产版本
npm run tauri build
```

前端单独调试：

```bash
npm run dev      # Vite 开发服务器 (端口 1420)
npm run build    # 构建前端
```

## 项目结构

```
src/                          # React 前端
├── api.ts                    # Tauri 命令的 TypeScript 封装
├── App.tsx                   # 主路由与状态管理
└── components/
    ├── ChapterEditor.tsx     # 主写作界面
    ├── ChapterManager.tsx    # 章节 / 批量生成管理
    └── ChatCreator.tsx       # 框架生成向导

src-tauri/src/                # Rust 后端
├── lib.rs                    # 所有 Tauri 命令（API 面）
├── engine/                   # 核心 AI 生成逻辑
│   ├── mod.rs
│   └── prompts.rs            # 提示词模板
├── llm/client.rs             # 多 provider LLM 客户端
├── storage/                  # 文件存储 + keyring + 审计日志
└── plugins/                  # Skills / MCP 插件系统
```

## 数据存储

项目数据存放于 `~/Library/Application Support/retl/projects/{project_id}/`，包含世界观、角色、大纲、时间线、逐章摘要与章节正文，可在设置中自定义数据目录。

## 安全

- API Key 通过系统 keyring 安全存储，不写入配置文件
- 关键操作带审计日志
- 无硬编码密钥
