# sub-pilot

一款完全本地运行、免费的 AI 视频字幕工具（Tauri 桌面版）。术语见 `CONTEXT.md`，架构决策见 `docs/adr/`。

## Agent skills

### Issue tracker

Issues and PRDs are tracked as local markdown files under `.scratch/<feature>/`. No external PR surface. See `docs/agents/issue-tracker.md`.

### Triage labels

Five canonical roles, each label string equals its name (needs-triage / needs-info / ready-for-agent / ready-for-human / wontfix). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.
