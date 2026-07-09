# Tickets: SubtitleFlow MVP (macOS)

把 `.scratch/subtitleflow-mvp/PRD.md` 的 MVP 拆成曳光弹垂直切片。术语见 `CONTEXT.md`，架构决策见 `docs/adr/0001`–`0004`。

Work the **frontier**: 任何「Blocked by」已全部完成的票都可开工。依赖图：0 → 1 → {2, 3, 4, 7 并行}；4 → 5 → 6。每张票用 `/implement` 单独构建，票与票之间清空上下文。

## 0. 项目骨架搭建（prefactor）

**What to build:** 初始化整个 Tauri v2 应用工程，让它能启动并显示一个空首页，且把外部依赖就位——为后续所有曳光弹铺好可运行的骨架。「先让改动变容易，再做容易的改动」。

**Blocked by:** None — can start immediately.

- [ ] Tauri v2 + React + TypeScript + TailwindCSS + shadcn/ui 前端初始化，`npm run tauri dev` 能启动窗口
- [ ] Rust 后端工程就绪，能编译、能被前端通过 Tauri command 调用（一个 ping 样例即可）
- [ ] ffmpeg 静态二进制打进 `resources/ffmpeg/`（macOS/Apple Silicon）
- [ ] base 模型（GGML/GGUF）打进 `resources/models/`
- [ ] 首次启动自检：确认随包 ffmpeg 可执行、base 模型文件存在，缺失时给出明确提示
- [ ] 空首页可显示（后续票在此填充导入区与最近项目）

## 1. 曳光弹：导入 → 转写 → 展示 original.srt

**What to build:** 用户拖拽或选择一个视频（MP4/MKV/MOV/AVI），应用显示文件名与时长，用内置 base 模型转写（显示进度与预计剩余时间），产出源语言的 `original.srt` 并在界面展示；整个过程持久化为一个 Project 目录 + `project.json`。这一票打通整条技术栈：前端 ↔ Rust ↔ whisper-rs ↔ ffmpeg（抽音频）↔ 项目持久化 ↔ 进度回传。

**Blocked by:** 0

- [ ] 拖拽/选择导入视频，支持 MP4/MKV/MOV/AVI，展示文件名与时长
- [ ] 定义 `MediaProcessor` trait（探测时长、抽音频），用 ffmpeg 实现；编排逻辑依赖 trait
- [ ] 定义 `Transcriber` trait，用 whisper-rs + base 模型实现；编排逻辑依赖 trait
- [ ] 转写过程回传进度与预计剩余时间到前端
- [ ] 产出 `original.srt`（SRT 序列化经接缝 1 的字幕逻辑）
- [ ] 界面展示生成的原始字幕；提供「打开字幕」「打开目录」
- [ ] 创建 Project 目录 + `project.json`（状态：imported → transcribing → transcribed）
- [ ] 接缝 1 单测：SRT 序列化/解析正常与边界样本
- [ ] 接缝 2 单测：用假 `Transcriber`/`MediaProcessor` 验证状态流转与进度回传

## 2. 最近项目与项目恢复

**What to build:** 首页显示「最近项目」列表；用户重新打开一个中途的项目时，应用恢复到上次保存的状态（已转写的显示原始字幕，未完成的回到对应步骤）。

**Blocked by:** 1

- [ ] 应用数据目录维护一个轻量最近项目索引
- [ ] 首页展示最近项目列表，可点击打开
- [ ] 打开项目时从 `project.json` 恢复状态机到正确步骤
- [ ] 索引与项目目录不一致时（目录被删/移动）优雅处理

## 3. Prompt 生成

**What to build:** 转写完成后，应用为原始字幕自动生成适用于 SRT 的翻译 Prompt；用户可分别「复制 Prompt」「复制字幕」或「一键复制全部」，以便粘贴到任意外部 AI。

**Blocked by:** 1

- [ ] 接缝 1：由原始字幕生成翻译 Prompt 文本（纯函数）
- [ ] 界面提供复制 Prompt / 复制字幕 / 一键复制全部
- [ ] `project.json` 状态推进到 prompt_ready
- [ ] 接缝 1 单测：给定原始字幕，Prompt 文本符合预期

## 4. 译文字幕导入 + 校验

**What to build:** 用户把从外部 AI 得到的译文字幕导回应用，应用校验其与原始字幕段数一致、编号连续、UTF-8 编码、SRT 语法合法；校验失败时把用户定位到具体出问题的段落。时间轴以原始字幕为准（ADR-0004）。

**Blocked by:** 1

- [ ] 导入译文字幕文件
- [ ] 接缝 1：Validation 纯逻辑——段数/编号/UTF-8/SRT 语法四类硬错误判定
- [ ] 校验失败定位到具体段落编号并在界面展示
- [ ] 时间轴采用原始字幕，不对译文时间戳严格比对
- [ ] `project.json` 状态推进到 translation_imported → validated
- [ ] 接缝 1 单测：正常样本 + 每类硬错误样本（漏段/多段/编号乱序/非 UTF-8/非法语法）都被正确判定与定位

## 5. 外挂字幕导出

**What to build:** 校验通过后，用户把译文字幕导出为外挂字幕——把 `.srt` 输出到视频旁，或用 ffmpeg 把字幕 mux 成视频里可开关的软字幕轨。

**Blocked by:** 4

- [ ] `MediaProcessor` trait 扩展：mux 软字幕轨
- [ ] 导出 sidecar `.srt` 到指定目录
- [ ] 导出封装软字幕轨的视频（可开关）
- [ ] `project.json` 状态推进到 exported
- [ ] 接缝 2 单测：用假 `MediaProcessor` 验证导出编排

## 6. 烧录导出

**What to build:** 用户把字幕烧录进视频画面导出，得到在任何播放器上都显示字幕的成品视频；处理 CJK 字体嵌入，导出过程显示进度。最重的一环，放最后。

**Blocked by:** 5

- [ ] `MediaProcessor` trait 扩展：用 ffmpeg 字幕滤镜烧录
- [ ] 处理 CJK 字体嵌入，确保中文/日文正确渲染
- [ ] 烧录过程回传进度
- [ ] `project.json` 记录烧录导出结果
- [ ] 用极短音视频 fixture 做端到端手动验证：产出可播放、字幕正确显示的视频

## 7. 模型选择 + 按需下载

**What to build:** 用户在转写前可选择模型（tiny/base/small/medium/large-v3）；当选择尚未下载的模型时，应用按需下载并显示进度、校验完整性，然后用它转写。

**Blocked by:** 1

- [ ] 转写前的模型选择 UI
- [ ] 检测模型是否已下载
- [ ] 按需下载缺失模型，显示进度并校验完整性
- [ ] 下载失败时给出明确错误与重试
- [ ] `project.json` 记录本项目所用模型
