# PRD / Spec: SubtitleFlow MVP (macOS)

Status: ready-for-agent

> 本 spec 由 grill-with-docs 对话综合而来。产品愿景见仓库根 `SubtitleFlow_PRD.md`；术语见 `CONTEXT.md`；架构决策见 `docs/adr/0001`–`0004`。本 spec 只覆盖 macOS 首发的 MVP。

## Problem Statement

想给外语视频（日漫、美剧、YouTube、B站等）配字幕的用户，目前要在多个割裂的工具间来回倒腾：用一个工具转写、手动整理成可翻译的文本、贴到某个 AI 翻译、再想办法把译文对回时间轴、最后用命令行 ffmpeg 烧录。过程繁琐、易错（译文段落错位后很难定位），且多数在线工具要上传视频、涉及隐私与费用。用户需要一个完全本地、免费、不绑定任何翻译平台的一体化字幕工具。

## Solution

一款 macOS 桌面应用（Tauri），把整条字幕流水线收拢到一个界面里：导入视频 → 本地 Whisper 转写出原始字幕（Original Subtitle）→ 一键生成翻译提示词（Prompt）供用户自行粘贴到任意外部 AI → 用户把译文字幕（Translated Subtitle）导回 → 应用自动校验（Validation）段数/编号/编码/格式并定位异常段落 → 导出外挂字幕（Sidecar）或烧录（Burn-in）进视频。全程离线、不登录、不上传、不集成翻译 API。字幕产物以项目（Project）目录 + `project.json` 形式持久化，可中断、可恢复，支持「最近项目」。

## User Stories

1. As a 视频字幕用户, I want 把本地视频文件拖拽或选择导入应用, so that 我能开始为它制作字幕。
2. As a 用户, I want 应用识别我导入的视频的文件名与时长, so that 我确认导入的是正确的文件。
3. As a 用户, I want 应用支持 MP4 / MKV / MOV / AVI 这几种常见容器, so that 我常见的视频都能处理。
4. As a 用户, I want 应用用内置的 base 模型立即开始转写, so that 我装完就能离线用、不必先下载模型。
5. As a 用户, I want 在转写前选择更大的模型（tiny/base/small/medium/large-v3）, so that 我能在速度与精度间权衡。
6. As a 用户, I want 当我选择尚未下载的模型时应用按需下载并显示进度, so that 我能用上更高精度的模型。
7. As a 用户, I want 在转写过程中看到进度与预计剩余时间, so that 我知道还要等多久。
8. As a 用户, I want 转写完成后得到一个源语言的原始字幕（original.srt）, so that 我有可翻译、可导出的基础字幕。
9. As a 用户, I want 打开生成的字幕文件或其所在目录, so that 我能直接查看或用别的编辑器打开。
10. As a 用户, I want 一键生成适用于 SRT 的翻译提示词, so that 我不用自己琢磨怎么让 AI 正确翻译并保持格式。
11. As a 用户, I want 分别复制「Prompt」「字幕内容」或「一键复制全部」, so that 我能按外部 AI 的输入习惯灵活粘贴。
12. As a 用户, I want 把从外部 AI 得到的译文字幕导回应用, so that 我能继续在应用内完成校验与导出。
13. As a 用户, I want 应用在导入译文时校验其段数与原始字幕一致, so that 漏译/多译的段落能被立刻发现。
14. As a 用户, I want 应用校验译文字幕的编号连续、UTF-8 编码、SRT 语法合法, so that 我不会导出一个损坏的字幕。
15. As a 用户, I want 校验失败时应用把我定位到具体出问题的段落, so that 我能快速回到外部 AI 修正那一段。
16. As a 用户, I want 校验时时间轴以原始字幕为准, so that 外部 AI 重排毫秒或格式不会造成无意义的报错。
17. As a 用户, I want 校验通过后导出外挂字幕（把字幕文件放到视频旁或封装为可开关的软字幕）, so that 我能得到一个可随时开关字幕的视频/字幕组合。
18. As a 用户, I want 把字幕烧录进视频画面导出, so that 我能得到一个在任何播放器上都显示字幕的成品视频。
19. As a 用户, I want 在烧录/导出过程中看到进度, so that 长视频编码时我知道进展。
20. As a 用户, I want 我的每次字幕工作被保存为一个项目, so that 我关闭应用后还能回来继续。
21. As a 用户, I want 首页看到「最近项目」列表, so that 我能快速回到之前的工作。
22. As a 用户, I want 重新打开一个中途的项目时它恢复到上次的状态, so that 我不用从头再来。
23. As a 用户, I want 首次启动时应用自检 ffmpeg 与默认模型就绪, so that 我能立即开始而不遇到缺依赖的错误。
24. As a 用户, I want 全程无需登录、无需联网（下载额外模型除外）、数据不上传, so that 我的视频与隐私留在本地。
25. As a 用户, I want 转写或导出出错时看到清晰的错误信息, so that 我知道是文件问题、依赖问题还是其它原因。

## Implementation Decisions

**平台与技术栈**
- 首发仅 macOS（Apple Silicon 优先，GPU 加速走 Metal/CoreML）；跨平台是后续目标，本 MVP 不实现（ADR-0001 语境）。
- 前端 Tauri v2 + React + TypeScript + TailwindCSS + shadcn/ui；后端 Rust。

**转写引擎（ADR-0001）**
- 用 whisper.cpp，经 `whisper-rs` 直接链接进 Rust 后端，不使用 faster-whisper，不打包 Python 运行时。
- 模型采用 GGML/GGUF 格式。

**依赖交付（ADR-0002）**
- ffmpeg 静态二进制随应用打包进 `resources/ffmpeg/`。
- 内置 `base` 模型作默认；`small/medium/large-v3` 等在识别界面按需下载（带进度与校验）。

**项目持久化（ADR-0003）**
- 每个 Project 是一个目录，含源视频引用、`original.srt`、译文字幕、`project.json`（状态机、所用模型、进度）。不使用数据库。
- 「最近项目」由应用数据目录下一个轻量索引文件维护。
- `project.json` 记录一个显式的项目状态机：`imported → transcribing → transcribed → prompt_ready → translation_imported → validated → exported`（失败态另计）。

**模块与接缝（与用户确认的测试接缝一致）**
- **接缝 1 — 字幕领域模块（纯逻辑，无 IO）**：SRT 解析、Validation（译文 vs 原始）、Prompt 生成。输入输出为字符串/结构体，无需 mock。校验规则见 ADR-0004。
- **接缝 2 — 外部进程适配器（trait）**：定义 `Transcriber`（whisper-rs 背后）与 `MediaProcessor`（ffmpeg 抽音频/探测时长/烧录/mux 外挂）两个 trait。编排逻辑（项目状态流转、进度回传、错误映射）依赖 trait，可用假实现替身测试。
- **Tauri command 层为薄壳**：仅把前端请求转发到上述逻辑，不承载业务逻辑、不单独铺测试接缝，避免接缝扩散。

**校验（ADR-0004）**
- 硬错误（阻断导出并定位段落）：段数与原始字幕不一致、编号不连续、非 UTF-8、SRT 语法非法。
- 时间轴：直接采用原始字幕的时间轴，译文只贡献文本；不对译文时间戳做严格逐段比对。

**导出**
- 外挂/Sidecar：把 `.srt` 输出到视频旁，或用 ffmpeg mux 为可开关的软字幕轨。
- Burn-in：用 ffmpeg 字幕滤镜将字幕渲染进画面；需处理 CJK 字体嵌入。

**曳光弹顺序（tracer-bullet，供 to-tickets 拆分）**
1. 导入视频 → ffmpeg 抽音频 → base 模型转写 → 生成并展示 `original.srt`（打通整条技术栈：前端↔Rust↔whisper-rs↔ffmpeg↔项目持久化↔进度回传）。
2. Prompt 生成（复制 Prompt / 复制字幕 / 一键全部）。
3. 译文字幕导入 + 校验（硬错误定位到段落）。
4. 外挂字幕导出。
5. 烧录导出（最重，放最后）。

## Testing Decisions

- **好的测试只验证外部行为，不绑定实现细节。** 接缝 1 的字幕逻辑用具体输入/输出样例驱动（真实的 SRT 文本片段、错位/漏段/编号乱序/非法语法等边界样本），断言解析结果、校验判定与定位段号、Prompt 文本。
- **接缝 1（字幕领域模块）是测试重点**：纯函数、无外部依赖，覆盖正常路径与全部硬错误分支；这是校验卖点的落点，必须扎实。
- **接缝 2（外部进程 trait）用假替身测试编排**：以假的 `Transcriber`/`MediaProcessor` 验证项目状态流转、进度回传、错误映射，不在单测里真正调用 whisper/ffmpeg。
- **真实 whisper/ffmpeg 集成**用一个极短的音视频 fixture 做端到端手动验证（转写产出合理字幕、烧录产出可播放视频），不追求确定性断言。
- 先按 `/tdd` 一次红-绿一个切片地构建每颗曳光弹；本仓库暂无既有测试，接缝 1 的样例测试即为后续 prior art。

## Out of Scope

- 非 macOS 平台（Windows/Linux）——后续跨平台目标。
- 登录、账号、云端、任何服务器依赖。
- 集成任何翻译 API 或绑定特定 AI 平台（翻译由用户在外部自行完成）。
- OCR 硬字幕识别。
- 字幕编辑器与时间轴编辑（PRD V3）。
- 双语字幕、ASS/SSA 格式（PRD V2）。
- 批量处理（PRD V4）。
- 自动修复 SRT、插件系统、企业版、云端 GPU（PRD 后续方向）。

## Further Notes

- 首次启动自检：确认随包的 ffmpeg 可执行、内置 base 模型存在；缺失时给出明确提示（正常情况下都随包存在）。
- 内置默认模型选 base 而非 PRD 原写的 small，是为压低安装包体积；用户可随时下载更大模型（ADR-0002）。
- 术语在整份 spec 与后续 issue/测试命名中统一使用 `CONTEXT.md` 的词汇（Project/Task/Transcription/Original Subtitle/Translated Subtitle/Prompt/Validation/Burn-in/Sidecar）。
