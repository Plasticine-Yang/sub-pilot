import { useCallback, useEffect, useState } from "react";
import { listRecent, ProjectStatus, type RecentEntry } from "@/lib/api";
import { Card } from "@/components/ui/card";

/** Human-readable label for the step a project is resumed to. */
const STATUS_LABELS: Record<ProjectStatus, string> = {
  [ProjectStatus.Imported]: "待转写",
  [ProjectStatus.Transcribing]: "转写中",
  [ProjectStatus.Transcribed]: "已转写",
  [ProjectStatus.PromptReady]: "已生成 Prompt",
  [ProjectStatus.TranslationImported]: "译文已导入",
  [ProjectStatus.Validated]: "已校验",
  [ProjectStatus.Exported]: "已导出",
  [ProjectStatus.Failed]: "失败",
};

/** Ticket 2: the home-screen recent-projects list. */
export function RecentProjects({ onOpen }: { onOpen: (id: string) => void }) {
  const [entries, setEntries] = useState<RecentEntry[] | null>(null);

  useEffect(() => {
    let active = true;
    listRecent()
      .then((list) => active && setEntries(list))
      .catch(() => active && setEntries([]));
    return () => {
      active = false;
    };
  }, []);

  const open = useCallback((id: string) => onOpen(id), [onOpen]);

  if (entries === null) return null;

  return (
    <section aria-label="最近项目" className="space-y-3">
      <h2 className="text-sm font-medium text-muted-foreground">最近项目</h2>
      {entries.length === 0 ? (
        <Card className="px-4 py-8 text-center text-sm text-muted-foreground">
          还没有项目。导入一个视频即可开始。
        </Card>
      ) : (
        <div className="grid gap-2">
          {entries.map((entry) => (
            <button
              key={entry.id}
              type="button"
              onClick={() => open(entry.id)}
              className="flex items-center justify-between gap-3 rounded-md border border-border px-4 py-3 text-left transition-colors hover:bg-accent hover:text-accent-foreground"
            >
              <span className="truncate text-sm font-medium">
                {entry.videoFileName}
              </span>
              <span className="shrink-0 text-xs text-muted-foreground">
                {STATUS_LABELS[entry.status]}
              </span>
            </button>
          ))}
        </div>
      )}
    </section>
  );
}
