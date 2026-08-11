/**
 * Drop-in replacement for `@tauri-apps/plugin-autostart`.
 *
 * Autostart is a desktop-shell concept; in browser mode every call is a
 * harmless no-op (`isEnabled` reports false) so the settings UI renders.
 */
import {
  enable as tauriEnable,
  disable as tauriDisable,
  isEnabled as tauriIsEnabled,
} from "@tauri-apps/plugin-autostart"
import { useTauriTransport } from "./platform"

export async function enable(): Promise<void> {
  if (useTauriTransport) return tauriEnable()
}

export async function disable(): Promise<void> {
  if (useTauriTransport) return tauriDisable()
}

export async function isEnabled(): Promise<boolean> {
  if (useTauriTransport) return tauriIsEnabled()
  return false
}
