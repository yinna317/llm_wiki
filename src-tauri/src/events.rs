/// App-wide event bus bridging Tauri window events to the browser-mode SSE
/// stream (`GET /api/v1/events`).
///
/// Desktop mode delivers events through Tauri's webview channel; browser mode
/// has no webview, so every emission point also publishes onto a Tokio
/// broadcast channel that the API server streams to SSE clients. Emission
/// sites call `emit` instead of `AppHandle::emit` — behaviour in desktop mode
/// is unchanged.
use serde::Serialize;
use serde_json::Value;
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter, Runtime};
use tokio::sync::broadcast;

/// Bounded per-event bus. Slow SSE consumers that fall behind simply miss
/// intermediate events (broadcast::Receiver returns Lagged); the UI already
/// tolerates dropped intermediate updates because every stream has a
/// terminal event (`done` / queue snapshot) carrying full state.
const BUS_CAPACITY: usize = 256;

static BUS: OnceLock<broadcast::Sender<(String, Value)>> = OnceLock::new();

fn bus() -> &'static broadcast::Sender<(String, Value)> {
    BUS.get_or_init(|| broadcast::channel(BUS_CAPACITY).0)
}

/// Publish an event onto the SSE bus without touching Tauri. Used where a
/// Tauri webview emission would be meaningless (e.g. API-originated chat
/// streams already go over their own SSE response).
pub fn publish(event: &str, payload: Value) {
    // No subscribers is normal in desktop mode — ignore SendError.
    let _ = bus().send((event.to_string(), payload));
}

/// Emit to Tauri webviews AND the SSE bus. Drop-in replacement for
/// `AppHandle::emit` at app-wide emission sites.
pub fn emit<R: Runtime, T: Serialize + Clone>(app: &AppHandle<R>, event: &str, payload: T) {
    let _ = app.emit(event, payload.clone());
    if let Ok(value) = serde_json::to_value(payload) {
        publish(event, value);
    }
}

/// Subscribe to the SSE bus. Each subscriber gets an independent cursor.
pub fn subscribe() -> broadcast::Receiver<(String, Value)> {
    bus().subscribe()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn published_events_reach_subscribers_in_order() {
        let mut rx = subscribe();
        publish("test-event", serde_json::json!({ "n": 1 }));
        publish("test-event", serde_json::json!({ "n": 2 }));
        let (event, payload) = rx.recv().await.unwrap();
        assert_eq!(event, "test-event");
        assert_eq!(payload["n"], 1);
        let (_, payload) = rx.recv().await.unwrap();
        assert_eq!(payload["n"], 2);
    }
}
