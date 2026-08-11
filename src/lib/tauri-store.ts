/**
 * Drop-in replacement for `@tauri-apps/plugin-store`'s `load`.
 *
 * Desktop mode delegates to the real plugin (app-state.json via IPC).
 * Browser mode backs the same file through `GET/PUT /api/v1/app-state`,
 * keeping an in-memory mirror so `get` stays synchronous-looking and `set`
 * triggers a serialized write — mirroring `autoSave: true`.
 */
import { load as tauriLoad } from "@tauri-apps/plugin-store"
import { apiServerBaseUrl, tokenQuery, useTauriTransport } from "./platform"

export interface StoreLike {
  get<T>(key: string): Promise<T | undefined>
  set(key: string, value: unknown): Promise<void>
  delete(key: string): Promise<void>
  save(): Promise<void>
}

class BrowserStore implements StoreLike {
  private cache: Record<string, unknown> | null = null
  private writeQueue: Promise<void> = Promise.resolve()

  private async ensureLoaded(): Promise<Record<string, unknown>> {
    if (this.cache) return this.cache
    const response = await fetch(`${apiServerBaseUrl()}/app-state?${tokenQuery()}`)
    if (!response.ok) throw new Error(`Failed to load app state: HTTP ${response.status}`)
    const data = (await response.json()) as Record<string, unknown>
    this.cache = data && typeof data === "object" ? data : {}
    return this.cache
  }

  async get<T>(key: string): Promise<T | undefined> {
    const cache = await this.ensureLoaded()
    return cache[key] as T | undefined
  }

  async set(key: string, value: unknown): Promise<void> {
    const cache = await this.ensureLoaded()
    cache[key] = value
    await this.save()
  }

  async delete(key: string): Promise<void> {
    const cache = await this.ensureLoaded()
    delete cache[key]
    await this.save()
  }

  save(): Promise<void> {
    // Serialize writes so a rapid set() sequence can't interleave two PUTs
    // with divergent snapshots of the cache.
    this.writeQueue = this.writeQueue.then(() => this.flush())
    return this.writeQueue
  }

  private async flush(): Promise<void> {
    if (!this.cache) return
    const response = await fetch(`${apiServerBaseUrl()}/app-state?${tokenQuery()}`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(this.cache),
    })
    if (!response.ok) throw new Error(`Failed to save app state: HTTP ${response.status}`)
  }
}

export function load(name: string, options?: Parameters<typeof tauriLoad>[1]): Promise<StoreLike> {
  if (useTauriTransport) {
    return tauriLoad(name, options) as unknown as Promise<StoreLike>
  }
  return Promise.resolve(new BrowserStore())
}
