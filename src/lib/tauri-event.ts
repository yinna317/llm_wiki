/**
 * Drop-in replacement for `@tauri-apps/api/event`'s `listen`.
 *
 * Desktop mode delegates to the real module. Browser mode multiplexes every
 * listener over a single SSE connection to `/api/v1/events`; the Rust side
 * publishes each Tauri event with its name as the SSE `event:` field, so
 * `addEventListener(name)` per topic (including dynamic `claude-cli:<id>`)
 * just works.
 */
import { listen as tauriListen, type UnlistenFn } from "@tauri-apps/api/event"
import { apiServerBaseUrl, tokenQuery, useTauriTransport } from "./platform"

export type { UnlistenFn }

export interface EventLike<T> {
  payload: T
}

let eventSource: EventSource | null = null

function ensureEventSource(): EventSource {
  if (!eventSource) {
    eventSource = new EventSource(`${apiServerBaseUrl()}/events?${tokenQuery()}`)
    eventSource.onerror = () => {
      // EventSource auto-reconnects; nothing to do but avoid an unhandled
      // error surfacing in the console as an exception.
    }
  }
  return eventSource
}

export function listen<T>(event: string, handler: (event: EventLike<T>) => void): Promise<UnlistenFn> {
  if (useTauriTransport) {
    return tauriListen<T>(event, handler)
  }
  const source = ensureEventSource()
  const wrapped = (message: MessageEvent<string>) => {
    try {
      handler({ payload: JSON.parse(message.data) as T })
    } catch {
      // Ignore malformed frames — the terminal event of each stream carries
      // full state, so a dropped intermediate frame is recoverable.
    }
  }
  source.addEventListener(event, wrapped)
  return Promise.resolve(() => source.removeEventListener(event, wrapped))
}
