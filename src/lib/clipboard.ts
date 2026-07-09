import { writeText } from "@tauri-apps/plugin-clipboard-manager";

/** Copies text to the OS clipboard via the Tauri clipboard-manager plugin. */
export function copyText(text: string): Promise<void> {
  return writeText(text);
}
