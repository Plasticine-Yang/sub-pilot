import { useEffect, useState } from "react";
import { selfCheck, type SelfCheckReport, ComponentState } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { cn } from "@/lib/utils";

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

function App() {
  const status = useSelfCheck();
  const ready = status.phase === CheckPhase.Ready && status.report.ok;

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
        <Card
          className={cn(
            "flex flex-col items-center justify-center gap-3 border-2 border-dashed py-16 text-center",
            ready ? "border-border" : "border-border/60 opacity-60",
          )}
        >
          <p className="text-base font-medium">拖拽视频到此处，或选择文件</p>
          <p className="text-sm text-muted-foreground">
            支持 MP4 / MKV / MOV / AVI
          </p>
          <Button disabled={!ready}>选择视频</Button>
        </Card>
      </section>

      <section aria-label="最近项目" className="space-y-3">
        <h2 className="text-sm font-medium text-muted-foreground">最近项目</h2>
        <Card className="px-4 py-8 text-center text-sm text-muted-foreground">
          还没有项目。导入一个视频即可开始。
        </Card>
      </section>
    </main>
  );
}

export default App;
