/**
 * Drop-in replacement for `@tauri-apps/api/core`.
 *
 * Desktop mode delegates to the real module; browser mode (`llm-wiki serve`)
 * routes `invoke` through the `/api/v1/cmd/{name}` bridge and
 * `convertFileSrc` through `/api/v1/files/raw`. Call sites keep their
 * exact Tauri call shape — only this import changes.
 */
import { invoke as tauriInvoke, convertFileSrc as tauriConvertFileSrc } from "@tauri-apps/api/core"
import { apiServerBaseUrl, serveSessionToken, tokenQuery, useTauriTransport } from "./platform"

interface CommandErrorBody {
  ok?: boolean
  error?: string
}

export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (useTauriTransport) {
    return tauriInvoke<T>(cmd, args)
  }
  const response = await fetch(`${apiServerBaseUrl()}/cmd/${cmd}?${tokenQuery()}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(args ?? {}),
  })
  const text = await response.text()
  let data: unknown = null
  try {
    data = text ? JSON.parse(text) : null
  } catch {
    // Non-JSON error page (proxy, 413, …) — fall through to the status error.
  }
  if (!response.ok) {
    const message = (data as CommandErrorBody | null)?.error ?? `HTTP ${response.status}`
    throw new Error(message)
  }
  const body = data as CommandErrorBody | null
  if (body && typeof body === "object" && body.ok === false) {
    throw new Error(body.error ?? `Command ${cmd} failed`)
  }
  return data as T
}

export function convertFileSrc(path: string): string {
  if (useTauriTransport) {
    return tauriConvertFileSrc(path)
  }
  return `${apiServerBaseUrl()}/files/raw?path=${encodeURIComponent(path)}&${tokenQuery()}`
}

export { serveSessionToken }
