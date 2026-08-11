/**
 * Runtime detection shared by every transport shim.
 *
 * The same frontend build runs in two modes:
 *  - desktop: inside the Tauri webview (`__TAURI_INTERNALS__` present),
 *    talking to Rust over IPC;
 *  - browser: served by `llm-wiki serve`, talking to the same Rust
 *    functions over HTTP (`/api/v1/cmd`, SSE `/events`, …).
 *
 * Serve mode injects a per-run session token into the URL (`?token=…`);
 * we capture it once into sessionStorage and scrub it from the address bar
 * so reloads and shared links don't leak it.
 */
export const isBrowserRuntime =
  typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window || "__TAURI__" in window)
export const isTauriRuntime = !isBrowserRuntime

/**
 * Vitest runs in jsdom — `window` exists but Tauri internals don't, which
 * would flip every shim into HTTP mode and break the existing `vi.mock`
 * harness. Tests always exercise the Tauri (mocked) path.
 */
export const isTestRuntime =
  typeof process !== "undefined" && !!(process as { env?: Record<string, string> }).env?.VITEST

/** Shims delegate to the real Tauri modules under desktop OR tests. */
export const useTauriTransport = isTauriRuntime || isTestRuntime

const TOKEN_STORAGE_KEY = "llm-wiki.serve-token"

function captureSessionToken(): string | null {
  if (!isBrowserRuntime) return null
  try {
    const stored = window.sessionStorage.getItem(TOKEN_STORAGE_KEY)
    if (stored) return stored
    const url = new URL(window.location.href)
    const token = url.searchParams.get("token")
    if (token) {
      window.sessionStorage.setItem(TOKEN_STORAGE_KEY, token)
      url.searchParams.delete("token")
      window.history.replaceState(null, "", url.toString())
      return token
    }
  } catch {
    // sessionStorage / history blocked — token stays in the URL
    const token = new URL(window.location.href).searchParams.get("token")
    if (token) return token
  }
  return null
}

export const serveSessionToken: string | null = captureSessionToken()

const DEFAULT_API_SERVER_BASE_URL = "http://127.0.0.1:19828/api/v1"

/**
 * Base URL of the Rust HTTP API. When the page was served by `llm-wiki
 * serve` (token present), the API is same-origin; otherwise fall back to an
 * explicit `__LLM_WIKI_API_BASE_URL__` override (browser dev against a
 * running backend) or the well-known default port.
 */
export function apiServerBaseUrl(): string {
  if (isBrowserRuntime) {
    if (serveSessionToken) return `${window.location.origin}/api/v1`
    const env = (window as unknown as { __LLM_WIKI_API_BASE_URL__?: string }).__LLM_WIKI_API_BASE_URL__
    if (env && env.trim().length > 0) return env.trim().replace(/\/$/, "")
  }
  return DEFAULT_API_SERVER_BASE_URL
}

/** `?token=…` suffix for endpoints EventSource/<img> can't set headers on. */
export function tokenQuery(): string {
  return `token=${encodeURIComponent(serveSessionToken ?? "")}`
}

export function ensureBrowserRuntime(): void {
  if (!isBrowserRuntime) {
    throw new Error("This API is only available in browser mode")
  }
}
