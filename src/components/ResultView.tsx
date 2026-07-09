import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  exportBurnIn,
  exportSidecar,
  exportSoftSubtitle,
  generatePrompt,
  importTranslation,
  onExportEvent,
  projectLocation,
  ProjectStatus,
  type Project,
  type ValidationResult,
} from "@/lib/api";
import { copyText } from "@/lib/clipboard";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { formatDuration } from "@/lib/utils";

/** A copy button that briefly confirms after writing to the clipboard. */
function CopyButton({
  text,
  label,
  variant = "outline",
}: {
  text: () => string;
  label: string;
  variant?: "outline" | "default";
}) {
  const [copied, setCopied] = useState(false);
  const onClick = useCallback(async () => {
    await copyText(text());
    setCopied(true);
    const timer = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(timer);
  }, [text]);
  return (
    <Button variant={variant} size="sm" onClick={onClick}>
      {copied ? "已复制" : label}
    </Button>
  );
}

/** Ticket 3: auto-generated translation Prompt with copy actions. */
function PromptSection({ project, srt }: { project: Project; srt: string }) {
  const [prompt, setPrompt] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    generatePrompt(project.id)
      .then((text) => active && setPrompt(text))
      .catch((err) => active && setError(String(err)));
    return () => {
      active = false;
    };
  }, [project.id]);

  if (error) {
    return (
      <div className="space-y-1">
        <p className="text-sm font-medium">翻译 Prompt</p>
        <p className="text-sm text-destructive">生成 Prompt 失败：{error}</p>
      </div>
    );
  }

  if (prompt === null) {
    return (
      <p className="text-sm text-muted-foreground">正在生成翻译 Prompt…</p>
    );
  }

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between gap-4">
        <p className="text-sm font-medium">翻译 Prompt</p>
        <div className="flex gap-2">
          <CopyButton text={() => prompt} label="复制 Prompt" />
          <CopyButton text={() => srt} label="复制字幕" />
          <CopyButton
            text={() => `${prompt}\n\n${srt}`}
            label="一键复制全部"
            variant="default"
          />
        </div>
      </div>
      <pre className="max-h-48 overflow-auto rounded-md bg-muted/50 p-4 text-xs leading-relaxed whitespace-pre-wrap">
        {prompt}
      </pre>
      <p className="text-xs text-muted-foreground">
        把 Prompt 与字幕粘贴到任意外部 AI，翻译后再把译文字幕导回。
      </p>
    </div>
  );
}

/** Post-transcription workspace. Grows through tickets 3–6. */
export function ResultView({
  project,
  srt,
  onReset,
  onStatusChange,
  initialTranslatedSrt = null,
}: {
  project: Project;
  srt: string;
  onReset: () => void;
  onStatusChange: (status: ProjectStatus) => void;
  initialTranslatedSrt?: string | null;
}) {
  // Validated SRT is new data returned by the import, so it lives here; the
  // status itself is lifted to App so props stay the single source of truth.
  // Seeded from a reopened project so a restored validated project shows it.
  const [translatedSrt, setTranslatedSrt] = useState<string | null>(
    initialTranslatedSrt,
  );

  const openSubtitle = useCallback(async () => {
    const loc = await projectLocation(project.id);
    await openPath(loc.originalSrt);
  }, [project.id]);

  const openDirectory = useCallback(async () => {
    const loc = await projectLocation(project.id);
    await revealItemInDir(loc.originalSrt);
  }, [project.id]);

  const onValidated = useCallback(
    (result: ValidationResult) => {
      if (result.ok && result.srt !== undefined) {
        setTranslatedSrt(result.srt);
        onStatusChange(ProjectStatus.Validated);
      }
    },
    [onStatusChange],
  );

  const validated = project.status === ProjectStatus.Validated;

  return (
    <Card className="gap-4 px-6 py-6">
      <div className="flex items-start justify-between gap-4">
        <div>
          <p className="font-medium">{project.videoFileName}</p>
          <p className="text-sm text-muted-foreground">
            时长 {formatDuration(project.durationMs)} ·{" "}
            {validated ? "译文字幕已校验" : "原始字幕已生成"}
          </p>
        </div>
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={openSubtitle}>
            打开字幕
          </Button>
          <Button variant="outline" size="sm" onClick={openDirectory}>
            打开目录
          </Button>
          <Button variant="ghost" size="sm" onClick={onReset}>
            新建
          </Button>
        </div>
      </div>

      <pre className="max-h-72 overflow-auto rounded-md bg-muted/50 p-4 text-xs leading-relaxed whitespace-pre-wrap">
        {validated && translatedSrt !== null ? translatedSrt : srt}
      </pre>

      {!validated && <PromptSection project={project} srt={srt} />}
      <TranslationSection
        project={project}
        validated={validated}
        onValidated={onValidated}
      />
      {validated && <ExportSection project={project} />}
    </Card>
  );
}

/** Ticket 4: import a translated subtitle and validate it against the original. */
function TranslationSection({
  project,
  validated,
  onValidated,
}: {
  project: Project;
  validated: boolean;
  onValidated: (result: ValidationResult) => void;
}) {
  const [error, setError] = useState<ValidationResult | null>(null);
  const [busy, setBusy] = useState(false);

  const pickAndValidate = useCallback(async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: "字幕", extensions: ["srt"] }],
    });
    if (typeof selected !== "string") return;
    setBusy(true);
    setError(null);
    try {
      const result = await importTranslation(project.id, selected);
      if (result.ok) {
        onValidated(result);
      } else {
        setError(result);
      }
    } catch (err) {
      setError({ ok: false, message: String(err) });
    } finally {
      setBusy(false);
    }
  }, [project.id, onValidated]);

  if (validated) {
    return (
      <div className="rounded-md border border-green-600/30 bg-green-600/5 px-4 py-3">
        <p className="text-sm font-medium text-green-700">
          译文字幕校验通过，时间轴已采用原始字幕。
        </p>
        <p className="mt-1 text-xs text-muted-foreground">
          可重新导入以替换译文字幕。
        </p>
        <div className="mt-2">
          <Button variant="outline" size="sm" onClick={pickAndValidate} disabled={busy}>
            {busy ? "校验中…" : "重新导入译文字幕"}
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between gap-4">
        <p className="text-sm font-medium">导入译文字幕</p>
        <Button variant="default" size="sm" onClick={pickAndValidate} disabled={busy}>
          {busy ? "校验中…" : "选择译文 .srt"}
        </Button>
      </div>
      <p className="text-xs text-muted-foreground">
        从外部 AI 得到译文字幕后导回，应用会校验段数、编号、UTF-8 与 SRT 语法。
      </p>
      {error && (
        <div className="rounded-md border border-destructive/40 bg-destructive/5 px-4 py-3">
          <p className="text-sm font-medium text-destructive">
            校验未通过：{error.message}
          </p>
          {error.segment != null && (
            <p className="mt-1 text-xs text-destructive/90">
              请回到外部 AI 修正第 {error.segment} 段后重新导入。
            </p>
          )}
        </div>
      )}
    </div>
  );
}

/** Ticket 5 + 6: export the validated subtitle as sidecar, soft track, or burn-in. */
function ExportSection({ project }: { project: Project }) {
  const [busy, setBusy] = useState<null | "sidecar" | "soft" | "burn">(null);
  const [done, setDone] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [burnPct, setBurnPct] = useState<number | null>(null);

  // Burn-in is long-running: progress and the terminal result arrive as events.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let active = true;
    onExportEvent((event) => {
      if (event.projectId !== project.id) return;
      if (event.kind === "progress") {
        setBurnPct(Math.round(event.fraction * 100));
      } else if (event.kind === "done") {
        setBurnPct(null);
        setBusy(null);
        setDone(event.path);
      } else {
        setBurnPct(null);
        setBusy(null);
        setError(event.message);
      }
    })
      .then((fn) => {
        if (active) unlisten = fn;
        else fn();
      })
      .catch(() => {});
    return () => {
      active = false;
      unlisten?.();
    };
  }, [project.id]);

  const runFileExport = useCallback(
    async (
      kind: "sidecar" | "soft",
      fn: (id: string, dir: string) => Promise<string>,
    ) => {
      const outDir = await open({ directory: true, multiple: false });
      if (typeof outDir !== "string") return;
      setBusy(kind);
      setError(null);
      setDone(null);
      try {
        setDone(await fn(project.id, outDir));
      } catch (err) {
        setError(String(err));
      } finally {
        setBusy(null);
      }
    },
    [project.id],
  );

  const runBurnIn = useCallback(async () => {
    const outDir = await open({ directory: true, multiple: false });
    if (typeof outDir !== "string") return;
    setBusy("burn");
    setError(null);
    setDone(null);
    setBurnPct(0);
    try {
      await exportBurnIn(project.id, outDir);
    } catch (err) {
      setBurnPct(null);
      setBusy(null);
      setError(String(err));
    }
  }, [project.id]);

  const revealDone = useCallback(async () => {
    if (done) await revealItemInDir(done);
  }, [done]);

  return (
    <div className="space-y-3">
      <div className="space-y-2">
        <p className="text-sm font-medium">导出外挂字幕</p>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="default"
            size="sm"
            disabled={busy !== null}
            onClick={() => runFileExport("sidecar", exportSidecar)}
          >
            {busy === "sidecar" ? "导出中…" : "导出 .srt 到目录"}
          </Button>
          <Button
            variant="outline"
            size="sm"
            disabled={busy !== null}
            onClick={() => runFileExport("soft", exportSoftSubtitle)}
          >
            {busy === "soft" ? "封装中…" : "导出可开关软字幕视频"}
          </Button>
        </div>
        <p className="text-xs text-muted-foreground">
          软字幕可在播放器里随时开关；.srt 会放到你选择的目录旁。
        </p>
      </div>

      <div className="space-y-2">
        <p className="text-sm font-medium">烧录导出</p>
        <Button
          variant="outline"
          size="sm"
          disabled={busy !== null}
          onClick={runBurnIn}
        >
          {busy === "burn" ? "烧录中…" : "把字幕烧录进画面"}
        </Button>
        <p className="text-xs text-muted-foreground">
          字幕永久渲染进画面，任何播放器都能显示（含中文/日文）；耗时较长。
        </p>
        {burnPct !== null && (
          <div className="space-y-1">
            <div className="h-2 w-full overflow-hidden rounded-full bg-muted">
              <div
                className="h-full rounded-full bg-primary transition-all"
                style={{ width: `${burnPct}%` }}
              />
            </div>
            <p className="text-xs text-muted-foreground">{burnPct}%</p>
          </div>
        )}
      </div>

      {done && (
        <p className="text-xs text-muted-foreground">
          已导出到 {done} ·{" "}
          <button className="underline" onClick={revealDone}>
            在访达中显示
          </button>
        </p>
      )}
      {error && <p className="text-sm text-destructive">导出失败：{error}</p>}
    </div>
  );
}
