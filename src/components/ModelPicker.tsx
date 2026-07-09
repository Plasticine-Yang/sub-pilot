import { useCallback, useEffect, useState } from "react";
import {
  downloadModel,
  listModels,
  onModelDownloadEvent,
  ModelId,
  type ModelStatus,
} from "@/lib/api";
import { Button } from "@/components/ui/button";

/** Human-readable labels for the accuracy/speed tradeoff. */
const MODEL_HINTS: Record<ModelId, string> = {
  [ModelId.Tiny]: "最快，精度最低",
  [ModelId.Base]: "随包内置，速度与精度均衡",
  [ModelId.Small]: "更高精度，体积约 0.5GB",
  [ModelId.Medium]: "高精度，体积约 1.5GB",
  [ModelId.LargeV3]: "最高精度，体积约 3GB",
};

/** Ticket 7: pick a transcription model, downloading it on demand if missing. */
export function ModelPicker({
  selected,
  onSelect,
  disabled,
}: {
  selected: ModelId;
  onSelect: (model: ModelId) => void;
  disabled: boolean;
}) {
  const [statuses, setStatuses] = useState<ModelStatus[] | null>(null);
  const [downloading, setDownloading] = useState<ModelId | null>(null);
  const [pct, setPct] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    listModels()
      .then(setStatuses)
      .catch((err) => setError(String(err)));
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let active = true;
    onModelDownloadEvent((event) => {
      if (event.kind === "progress") {
        setPct(
          event.total && event.total > 0
            ? Math.round((event.downloaded / event.total) * 100)
            : null,
        );
      } else if (event.kind === "done") {
        setDownloading(null);
        setPct(null);
        refresh();
      } else {
        setDownloading(null);
        setPct(null);
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
  }, [refresh]);

  const startDownload = useCallback(async (model: ModelId) => {
    setError(null);
    setDownloading(model);
    setPct(0);
    try {
      await downloadModel(model);
    } catch (err) {
      setDownloading(null);
      setPct(null);
      setError(String(err));
    }
  }, []);

  if (statuses === null) {
    return <p className="text-sm text-muted-foreground">正在读取模型列表…</p>;
  }

  return (
    <div className="space-y-2">
      <p className="text-sm font-medium">选择转写模型</p>
      <div className="grid gap-2">
        {statuses.map((status) => {
          const isSelected = status.id === selected;
          const available = status.downloaded || status.bundled;
          const isDownloading = downloading === status.id;
          return (
            <div
              key={status.id}
              className={[
                "flex items-center justify-between gap-3 rounded-md border px-3 py-2",
                isSelected ? "border-primary bg-primary/5" : "border-border",
              ].join(" ")}
            >
              <button
                type="button"
                className="flex-1 text-left disabled:opacity-60"
                disabled={disabled || !available}
                onClick={() => onSelect(status.id)}
              >
                <span className="text-sm font-medium">{status.name}</span>
                <span className="ml-2 text-xs text-muted-foreground">
                  {MODEL_HINTS[status.id]}
                </span>
              </button>
              {available ? (
                <span className="text-xs text-muted-foreground">
                  {isSelected ? "已选择" : status.bundled ? "已内置" : "已下载"}
                </span>
              ) : isDownloading ? (
                <span className="text-xs text-muted-foreground">
                  {pct != null ? `下载中 ${pct}%` : "下载中…"}
                </span>
              ) : (
                <Button
                  variant="outline"
                  size="xs"
                  disabled={disabled || downloading !== null}
                  onClick={() => startDownload(status.id)}
                >
                  下载
                </Button>
              )}
            </div>
          );
        })}
      </div>
      {error && (
        <p className="text-sm text-destructive">模型下载失败：{error}</p>
      )}
    </div>
  );
}
