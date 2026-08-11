/**
 * Drop-in replacement for `@tauri-apps/plugin-opener`.
 *
 * Browser mode: `openUrl` opens a new tab; `openPath` (revealing a local
 * file) has no browser equivalent and rejects with a clear message.
 */
import { openUrl as tauriOpenUrl, openPath as tauriOpenPath } from "@tauri-apps/plugin-opener"
import { useTauriTransport } from "./platform"

export async function openUrl(url: string): Promise<void> {
  if (useTauriTransport) return tauriOpenUrl(url)
  window.open(url, "_blank", "noopener,noreferrer")
}

export async function openPath(path: string): Promise<void> {
  if (useTauriTransport) return tauriOpenPath(path)
  throw new Error(`Revealing local files is not available in browser mode: ${path}`)
}
