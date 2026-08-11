/**
 * Shared HTTP helpers routed through Tauri's Rust-backed plugin so
 * third-party endpoints that don't set browser-friendly CORS headers
 * still work. Every part of the app that hits a user-configured URL
 * (LLM chat, embedding, web search, anything new) should import from
 * here rather than call `fetch` directly.
 *
 * Why it matters:
 *  - MiniMax's /anthropic endpoint: CORS allow-headers omits x-api-key
 *  - Volcengine Ark /api/coding/v3: CORS omits Authorization entirely
 *  - Any enterprise / on-prem gateway that doesn't anticipate browser
 *    origins — a common shape across domestic Chinese clouds
 *
 * In unit tests (vitest / node), the plugin's browser-only globals
 * aren't available; `getHttpFetch` lazily imports and falls back to
 * `globalThis.fetch` so helper functions in this file can be imported
 * from any environment without crashing at module load.
 */

import { useWikiStore } from "@/stores/wiki-store"
import { isProxyActive, type ProxyConfig } from "@/lib/proxy-config"
import { apiServerBaseUrl, tokenQuery, useTauriTransport } from "@/lib/platform"

let pluginFetchPromise: Promise<typeof globalThis.fetch> | null = null

/**
 * Browser-mode fetch: POST the request description to `/api/v1/http-proxy`
 * and stream the upstream response back. The proxy endpoint mirrors the
 * upstream status code and content-type, and the body streams, so SSE-style
 * LLM responses behave the same as the plugin path.
 */
function headersToRecord(headers: HeadersInit | undefined): Record<string, string> {
  if (!headers) return {}
  if (headers instanceof Headers) return Object.fromEntries(headers.entries())
  if (Array.isArray(headers)) return Object.fromEntries(headers)
  return { ...headers }
}

const browserProxyFetch: typeof globalThis.fetch = async (input, init) => {
  const url = typeof input === "string" ? input : input instanceof URL ? input.toString() : input.url
  const body = init?.body
  if (body != null && typeof body !== "string") {
    throw new Error("browser-mode http proxy only supports string request bodies")
  }
  const danger = (init as PluginRequestInit | undefined)?.danger
  const response = await globalThis.fetch(`${apiServerBaseUrl()}/http-proxy?${tokenQuery()}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      url,
      method: init?.method ?? "GET",
      headers: headersToRecord(init?.headers),
      body: body ?? undefined,
      dangerAcceptInvalidCerts: danger?.acceptInvalidCerts === true ? true : undefined,
    }),
    signal: init?.signal ?? null,
  })
  return response
}

/**
 * True when running outside a browser / webview (vitest, SSR, any
 * Node-based tooling). The Tauri plugin is importable in Node
 * (resolution succeeds) but its internals reach for `window` at call
 * time, so we must avoid invoking it — guard BEFORE the dynamic
 * import rather than trying to .catch() an error that happens later.
 */
const isNodeEnv = typeof window === "undefined"

type PluginRequestInit = RequestInit & {
  danger?: {
    acceptInvalidCerts?: boolean
    acceptInvalidHostnames?: boolean
  }
}

export function withProxyTlsSettings(
  init: RequestInit | undefined,
  proxy: ProxyConfig,
): PluginRequestInit | undefined {
  if (!isProxyActive(proxy) || proxy.acceptInvalidCerts !== true) return init
  const pluginInit = init as PluginRequestInit | undefined
  return {
    ...pluginInit,
    danger: {
      ...pluginInit?.danger,
      acceptInvalidCerts: true,
    },
  }
}

/**
 * Returns a fetch function that routes through Tauri's HTTP plugin in
 * production, falling back to the platform's native fetch in non-Tauri
 * environments (tests / SSR / storybook). Call this once per request:
 *
 *   const httpFetch = await getHttpFetch()
 *   const response = await httpFetch(url, opts)
 *
 * The promise is cached, so repeated calls don't re-import the plugin.
 */
export function getHttpFetch(): Promise<typeof globalThis.fetch> {
  if (!pluginFetchPromise) {
    if (!useTauriTransport) {
      // Browser mode (llm-wiki serve): no plugin runtime — proxy through
      // the Rust server so CORS-rejecting providers still work.
      pluginFetchPromise = Promise.resolve(browserProxyFetch)
    } else if (isNodeEnv) {
      // Bind so `this === globalThis` — Node's fetch requires it.
      pluginFetchPromise = Promise.resolve(globalThis.fetch.bind(globalThis))
    } else {
      pluginFetchPromise = import("@tauri-apps/plugin-http")
        .then((m) => {
          const pluginFetch = m.fetch
          const configuredFetch: typeof globalThis.fetch = (input, init) => {
            // Read at request time so changing Network settings takes effect
            // immediately. The option is deliberately scoped to the proxy
            // toggle; disabling the proxy restores normal TLS verification.
            const proxy = useWikiStore.getState().proxyConfig
            const requestInit = withProxyTlsSettings(init, proxy)
            return pluginFetch(input, requestInit)
          }
          return configuredFetch
        })
        .catch(() => globalThis.fetch.bind(globalThis))
    }
  }
  return pluginFetchPromise
}

/**
 * Detect fetch-level network failures across Tauri's different webview
 * backends. Each platform phrases the same failure class differently:
 *
 *   macOS / iOS (WebKit):       Error,  message === "Load failed"
 *   Windows    (Edge WebView2): TypeError, message === "Failed to fetch"
 *   Linux      (WebKitGTK):     Error,  message === "Load failed"
 *
 * They all collapse DNS / TLS / connection-refused / CORS-preflight
 * into a single opaque error with no structured detail. The only
 * reliable cross-platform signal is "not an AbortError AND one of
 * these generic network error shapes", which this helper centralizes.
 */
export function isFetchNetworkError(err: unknown): boolean {
  if (!(err instanceof Error)) return false
  if (err.name === "AbortError") return false
  // Chromium / Edge WebView2
  if (err.name === "TypeError") return true
  // WebKit (macOS / Linux GTK)
  if (err.message === "Load failed") return true
  // Chromium mid-stream drop
  if (err.message === "Failed to fetch") return true
  if (err.message.includes("network error")) return true
  return false
}
