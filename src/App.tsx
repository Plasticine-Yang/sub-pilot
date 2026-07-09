import { useCallback, useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  selfCheck,
  importVideo,
  startTranscription,
  readOriginalSrt,
  projectLocation,
  onTranscriptionEvent,
  ProjectStatus,
  type SelfCheckReport,
  type Project,
  ComponentState,
} from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { cn, formatDuration, formatEta } from "@/lib/utils";

enum CheckPhase {
  Loading = "loading",
  Ready = "ready",
  Error = "error",
}

type CheckStatus =
  | { phase: CheckPhase.Loading }
  | { phase: CheckPhase.Ready; report: SelfCheckReport }
  | { phase: CheckPhase.Error; message: string };

const COMPONENT_LABELS: Record<keyof Omit<SelfCheckReport, "ok">, string> = {
  ffmpeg: "ffmpeg",
  model: "base 模型",
};

const SUPPORTED_EXT = ["mp4", "mkv", "mov", "avi"];

function problemDetail(label: string, state: ComponentState): string {
  switch (state) {
    case ComponentState.Missing:
      return `缺少 ${label}，请运行 scripts/setup-resources.sh 获取`;
    case ComponentState.NotExecutable:
      return `${label} 存在但不可执行`;
    case ComponentState.Ok:
      return `${label} 就绪`;
  }
}

function useSelfCheck() {
  const [status, setStatus] = useState<CheckStatus>({
    phase: CheckPhase.Loading,
  });

  useEffect(() => {
    let active = true;
    selfCheck()
      .then((report) => active && setStatus({ phase: CheckPhase.Ready, report }))
      .catch(
        (err) =>
          active &&
          setStatus({
            phase: CheckPhase.Error,
            message: String(err ?? "自检失败"),
          }),
      );
    return () => {
      active = false;
    };
  }, []);

  return status;
}

function DependencyBanner({ status }: { status: CheckStatus }) {
  if (status.phase === CheckPhase.Loading) {
    return (
      <Card className="border-border bg-muted/40 px-4 py-3 text-sm text-muted-foreground">
        正在自检运行环境…
      </Card>
    );
  }

  if (status.phase === CheckPhase.Error) {
    return (
      <Card className="border-destructive/40 bg-destructive/5 px-4 py-3 text-sm text-destructive">
        环境自检未能完成：{status.message}
      </Card>
    );
  }

  const { report } = status;
  if (report.ok) return null;

  const problems = (["ffmpeg", "model"] as const)
    .filter((key) => report[key] !== ComponentState.Ok)
    .map((key) => problemDetail(COMPONENT_LABELS[key], report[key]));

  return (
    <Card className="border-destructive/40 bg-destructive/5 px-4 py-3">
      <p className="text-sm font-medium text-destructive">缺少运行所需的依赖</p>
      <ul className="mt-2 space-y-1 text-sm text-destructive/90">
        {problems.map((detail) => (
          <li key={detail}>• {detail}</li>
        ))}
      </ul>
    </Card>
  );
}

/** Progress observed during transcription. */
interface Progress {
  fraction: number;
  etaMs: number | null;
}

function ImportZone({
  ready,
  onImport,
  error,
}: {
  ready: boolean;
  onImport: (path: string) => void;
  error: string | null;
}) {
  const [dragging, setDragging] = useState(false);

  // Tauri delivers OS file drops through a webview event, not the DOM.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let active = true;
    try {
      getCurrentWebview()
        .onDragDropEvent((event) => {
          if (event.payload.type === "over" || event.payload.type === "enter") {
            setDragging(true);
          } else if (event.payload.type === "drop") {
            setDragging(false);
            const path = event.payload.paths[0];
            if (path && ready) onImport(path);
          } else {
            setDragging(false);
          }
        })
        .then((fn) => {
          if (active) unlisten = fn;
          else fn();
        })
        .catch(() => {});
    } catch {
      // Drag-drop is a progressive enhancement; the picker still works.
    }
    return () => {
      active = false;
      unlisten?.();
    };
  }, [ready, onImport]);

  const pick = useCallback(async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: "视频", extensions: SUPPORTED_EXT }],
    });
    if (typeof selected === "string") onImport(selected);
  }, [onImport]);

  return (
    <Card
      className={cn(
        "flex flex-col items-center justify-center gap-3 border-2 border-dashed py-16 text-center transition-colors",
        ready ? "border-border" : "border-border/60 opacity-60",
        dragging && ready && "border-primary bg-primary/5",
      )}
    >
      <p className="text-base font-medium">拖拽视频到此处，或选择文件</p>
      <p className="text-sm text-muted-foreground">支持 MP4 / MKV / MOV / AVI</p>
      <Button disabled={!ready} onClick={pick}>
        选择视频
      </Button>
      {error && <p className="text-sm text-destructive">{error}</p>}
    </Card>
  );
}

function TranscribingView({
  project,
  progress,
}: {
  project: Project;
  progress: Progress | null;
}) {
  const pct = progress ? Math.round(progress.fraction * 100) : 0;
  return (
    <Card className="gap-4 px-6 py-6">
      <div>
        <p className="font-medium">{project.videoFileName}</p>
        <p className="text-sm text-muted-foreground">
          时长 {formatDuration(project.durationMs)} · 使用 {project.model} 模型转写中
        </p>
      </div>
      <div className="space-y-2">
        <div className="h-2 w-full overflow-hidden rounded-full bg-muted">
          <div
            className="h-full rounded-full bg-primary transition-all"
            style={{ width: `${pct}%` }}
          />
        </div>
        <div className="flex justify-between text-sm text-muted-foreground">
          <span>{pct}%</span>
          <span>
            {progress?.etaMs != null
              ? `预计剩余 ${formatEta(progress.etaMs)}`
              : "预计剩余时间计算中…"}
          </span>
        </div>
      </div>
    </Card>
  );
}

function TranscribedView({
  project,
  srt,
  onReset,
}: {
  project: Project;
  srt: string;
  onReset: () => void;
}) {
  const openSubtitle = useCallback(async () => {
    const loc = await projectLocation(project.id);
    await openPath(loc.originalSrt);
  }, [project.id]);

  const openDirectory = useCallback(async () => {
    const loc = await projectLocation(project.id);
    await revealItemInDir(loc.originalSrt);
  }, [project.id]);

  return (
    <Card className="gap-4 px-6 py-6">
      <div className="flex items-start justify-between gap-4">
        <div>
          <p className="font-medium">{project.videoFileName}</p>
          <p className="text-sm text-muted-foreground">
            时长 {formatDuration(project.durationMs)} · 原始字幕已生成
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
      <pre className="max-h-96 overflow-auto rounded-md bg-muted/50 p-4 text-xs leading-relaxed whitespace-pre-wrap">
        {srt}
      </pre>
    </Card>
  );
}

function FailedView({
  project,
  onReset,
}: {
  project: Project;
  onReset: () => void;
}) {
  return (
    <Card className="gap-3 border-destructive/40 bg-destructive/5 px-6 py-6">
      <p className="text-sm font-medium text-destructive">
        转写失败：{project.error ?? "未知错误"}
      </p>
      <div>
        <Button variant="outline" size="sm" onClick={onReset}>
          重试
        </Button>
      </div>
    </Card>
  );
}

/** Selects exactly one workspace view from the current project status. */
function Workspace({
  project,
  progress,
  srt,
  ready,
  importError,
  onImport,
  onReset,
}: {
  project: Project | null;
  progress: Progress | null;
  srt: string | null;
  ready: boolean;
  importError: string | null;
  onImport: (path: string) => void;
  onReset: () => void;
}) {
  if (!project) {
    return <ImportZone ready={ready} onImport={onImport} error={importError} />;
  }
  switch (project.status) {
    case ProjectStatus.Imported:
    case ProjectStatus.Transcribing:
      return <TranscribingView project={project} progress={progress} />;
    case ProjectStatus.Transcribed:
      return srt !== null ? (
        <TranscribedView project={project} srt={srt} onReset={onReset} />
      ) : (
        <TranscribingView project={project} progress={progress} />
      );
    case ProjectStatus.Failed:
      return <FailedView project={project} onReset={onReset} />;
  }
}

function App() {
  const status = useSelfCheck();
  const ready = status.phase === CheckPhase.Ready && status.report.ok;

  const [project, setProject] = useState<Project | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [srt, setSrt] = useState<string | null>(null);
  const [importError, setImportError] = useState<string | null>(null);
  const projectRef = useRef<Project | null>(null);

  useEffect(() => {
    projectRef.current = project;
  }, [project]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let active = true;
    onTranscriptionEvent((event) => {
      const current = projectRef.current;
      if (!current || event.projectId !== current.id) return;
      if (event.kind === "progress") {
        setProgress({ fraction: event.fraction, etaMs: event.etaMs });
      } else if (event.kind === "status") {
        setProject({ ...current, status: event.status });
        if (event.status === ProjectStatus.Transcribed) {
          readOriginalSrt(current.id).then(setSrt).catch(() => setSrt(""));
        }
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
  }, []);

  const handleImport = useCallback(async (path: string) => {
    setImportError(null);
    try {
      const proj = await importVideo(path);
      setProject(proj);
      setProgress(null);
      setSrt(null);
      await startTranscription(proj.id);
      setProject({ ...proj, status: ProjectStatus.Transcribing });
    } catch (err) {
      setImportError(String(err));
    }
  }, []);

  const reset = useCallback(() => {
    setProject(null);
    setProgress(null);
    setSrt(null);
    setImportError(null);
  }, []);

  return (
    <main className="mx-auto flex min-h-screen max-w-3xl flex-col gap-8 px-6 py-10">
      <header className="space-y-1">
        <h1 className="text-2xl font-semibold tracking-tight">SubtitleFlow</h1>
        <p className="text-sm text-muted-foreground">
          完全本地运行的 AI 视频字幕工具
        </p>
      </header>

      <DependencyBanner status={status} />

      <section aria-label="导入视频">
        <Workspace
          project={project}
          progress={progress}
          srt={srt}
          ready={ready}
          importError={importError}
          onImport={handleImport}
          onReset={reset}
        />
      </section>

      {!project && (
        <section aria-label="最近项目" className="space-y-3">
          <h2 className="text-sm font-medium text-muted-foreground">最近项目</h2>
          <Card className="px-4 py-8 text-center text-sm text-muted-foreground">
            还没有项目。导入一个视频即可开始。
          </Card>
        </section>
      )}
    </main>
  );
}

export default App;
