import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/** Formats a millisecond duration as `H:MM:SS` (or `M:SS` under an hour). */
export function formatDuration(ms: number): string {
  const totalSecs = Math.floor(ms / 1000);
  const secs = totalSecs % 60;
  const mins = Math.floor(totalSecs / 60) % 60;
  const hours = Math.floor(totalSecs / 3600);
  const mm = String(mins).padStart(2, "0");
  const ss = String(secs).padStart(2, "0");
  return hours > 0 ? `${hours}:${mm}:${ss}` : `${mins}:${ss}`;
}

/** Human-friendly "预计剩余" label from an ETA in milliseconds. */
export function formatEta(ms: number): string {
  const totalSecs = Math.max(0, Math.round(ms / 1000));
  if (totalSecs < 60) return `约 ${totalSecs} 秒`;
  const mins = Math.floor(totalSecs / 60);
  const secs = totalSecs % 60;
  return `约 ${mins} 分 ${secs} 秒`;
}

