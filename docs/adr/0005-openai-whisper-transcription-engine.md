# 转写引擎改用 OpenAI 官方原版 whisper（Python + PyTorch）

**Status: accepted** — 本决策 supersede [ADR-0001](./0001-whisper-cpp-transcription-engine.md)。

我们把转写引擎从 whisper.cpp（经 `whisper-rs` 静态链接进 Rust）反转为 OpenAI 官方原版 `openai/whisper`（Python + PyTorch），理由是遵循官方原版的参考实现与实践。代价是重新引入 ADR-0001 当初特意规避的问题：需把 Python 运行时随应用打包，体积显著增大（PyTorch 依赖比 faster-whisper 用的 CTranslate2 更重），macOS 代码签名与分发更复杂——这与产品「完全本地、免安装、易分发」原则存在张力，此处明确接受该取舍。

## Consequences

- **模型格式**：从 GGML/GGUF 改为官方 PyTorch `.pt` 权重（如 `base.pt`），下载源与校验和随之更换（见 ADR-0002）。GGML 模型不再使用。
- **集成架构反转**：`Transcriber` 不再是进程内 FFI（whisper-rs 直接链接），改为 Rust 后端子进程调用打包的 Python whisper。这一点上它与被 ADR-0001 弃用的 faster-whisper 属于同类集成方式（子进程 + Python 运行时），差别仅在官方版走 PyTorch 而非 CTranslate2。
- **加速路径**：不再依赖 whisper.cpp 的 Metal/CoreML 原生加速；转由 PyTorch 在对应平台的后端提供。

## Considered Options

- **维持 whisper.cpp（ADR-0001 现状）**：最贴合「免安装、易分发」，但偏离官方参考实现。被本决策取代。
- **回到 faster-whisper**：同样需 Python 运行时，但更轻；非官方原版。未采纳。
- **OpenAI 官方原版 whisper（本决策）**：最贴合官方实践，代价是最重的分发体积与签名复杂度。
