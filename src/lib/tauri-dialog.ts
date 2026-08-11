/**
 * Drop-in replacement for `@tauri-apps/plugin-dialog`.
 *
 * Browsers can't offer native file pickers with absolute paths, which is
 * what the rest of the app needs. In browser mode `open`/`save` resolve to
 * `null` (same shape as "user cancelled") so callers degrade gracefully
 * instead of crashing; `message` falls back to a native alert.
 */
import {
  open as tauriOpen,
  save as tauriSave,
  message as tauriMessage,
  type OpenDialogOptions,
  type SaveDialogOptions,
} from "@tauri-apps/plugin-dialog"
import { useTauriTransport } from "./platform"

type MessageOptions = Parameters<typeof tauriMessage>[1]

export function open(options?: OpenDialogOptions & { multiple?: false }): Promise<string | null>
export function open(options: OpenDialogOptions & { multiple: true }): Promise<string[] | null>
export function open(options?: OpenDialogOptions): Promise<string | string[] | null> {
  if (useTauriTransport) return tauriOpen(options as OpenDialogOptions & { multiple?: false })
  console.warn("[browser-mode] native file dialog unavailable; treating as cancelled")
  return Promise.resolve(null)
}

export function save(options?: SaveDialogOptions): Promise<string | null> {
  if (useTauriTransport) return tauriSave(options)
  console.warn("[browser-mode] native save dialog unavailable; treating as cancelled")
  return Promise.resolve(null)
}

export async function message(msg: string, options?: MessageOptions): Promise<void> {
  if (useTauriTransport) {
    await tauriMessage(msg, options)
    return
  }
  window.alert(msg)
}
