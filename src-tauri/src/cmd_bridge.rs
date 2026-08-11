/// Browser-mode command bridge: maps `POST /api/v1/cmd/{name}` onto the same
/// Rust functions Tauri invokes for the desktop UI, so the frontend keeps one
/// code path — only the transport differs.
///
/// Argument objects arrive exactly as the frontend passes them to
/// `invoke(name, args)` — camelCase keys (`projectPath`, `extractImages`,
/// `streamId`, …). Helpers below read those keys and deserialize complex
/// values with the same serde shapes the Tauri IPC layer uses.
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{Map, Value};
use tauri::{AppHandle, Manager};

use crate::{agent, commands, proxy};

fn missing(key: &str) -> String {
    format!("missing required argument: {key}")
}

fn req_str(args: &Map<String, Value>, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| missing(key))
}

fn opt_str(args: &Map<String, Value>, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn opt_bool(args: &Map<String, Value>, key: &str) -> Option<bool> {
    args.get(key).and_then(Value::as_bool)
}

fn req_bool(args: &Map<String, Value>, key: &str) -> Result<bool, String> {
    args.get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| missing(key))
}

fn opt_usize(args: &Map<String, Value>, key: &str) -> Option<usize> {
    args.get(key)
        .and_then(Value::as_u64)
        .map(|value| value as usize)
}

fn req_usize(args: &Map<String, Value>, key: &str) -> Result<usize, String> {
    opt_usize(args, key).ok_or_else(|| missing(key))
}

fn opt_u64(args: &Map<String, Value>, key: &str) -> Option<u64> {
    args.get(key).and_then(Value::as_u64)
}

fn req_val(args: &Map<String, Value>, key: &str) -> Result<Value, String> {
    args.get(key).cloned().ok_or_else(|| missing(key))
}

fn opt_val(args: &Map<String, Value>, key: &str) -> Option<Value> {
    args.get(key).cloned().filter(|value| !value.is_null())
}

fn de<T: DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|err| format!("invalid argument: {err}"))
}

fn opt_de<T: DeserializeOwned>(args: &Map<String, Value>, key: &str) -> Result<Option<T>, String> {
    opt_val(args, key).map(de).transpose()
}

fn ser<T: Serialize>(result: Result<T, String>) -> Result<Value, String> {
    result.and_then(|value| {
        serde_json::to_value(value).map_err(|err| format!("failed to serialize result: {err}"))
    })
}

fn ser_plain<T: Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|err| format!("failed to serialize result: {err}"))
}

pub async fn dispatch(
    app: &AppHandle,
    name: &str,
    args: Map<String, Value>,
) -> Result<Value, String> {
    let a = &args;
    match name {
        // fs
        "read_file" => ser(commands::fs::read_file(req_str(a, "path")?, opt_bool(a, "extractImages")).await),
        "preprocess_file" => ser(commands::fs::preprocess_file(req_str(a, "path")?).await),
        "write_file" => ser(commands::fs::write_file(req_str(a, "path")?, req_str(a, "contents")?).await),
        "write_file_base64" => ser(commands::fs::write_file_base64(req_str(a, "path")?, req_str(a, "base64")?).await),
        "write_file_atomic" => ser(commands::fs::write_file_atomic(req_str(a, "path")?, req_str(a, "contents")?).await),
        "apply_text_selection_edit" => ser(commands::fs::apply_text_selection_edit(
            req_str(a, "projectPath")?,
            req_str(a, "filePath")?,
            req_str(a, "prefix")?,
            req_str(a, "selectedText")?,
            req_str(a, "suffix")?,
            req_str(a, "replacement")?,
        ).await),
        "create_missing_wiki_page" => ser(commands::fs::create_missing_wiki_page(
            req_str(a, "projectPath")?,
            req_str(a, "title")?,
            opt_str(a, "content"),
        ).await),
        "list_directory" => ser(commands::fs::list_directory(
            req_str(a, "path")?,
            opt_bool(a, "includeHidden"),
            opt_usize(a, "maxDepth"),
        ).await),
        "copy_file" => ser(commands::fs::copy_file(req_str(a, "source")?, req_str(a, "destination")?).await),
        "copy_directory" => ser(commands::fs::copy_directory(req_str(a, "source")?, req_str(a, "destination")?).await),
        "delete_file" => ser(commands::fs::delete_file(req_str(a, "path")?).await),
        "find_related_wiki_pages" => ser(commands::fs::find_related_wiki_pages(
            req_str(a, "projectPath")?,
            req_str(a, "sourceName")?,
        ).await),
        "create_directory" => ser(commands::fs::create_directory(req_str(a, "path")?).await),
        "read_file_as_base64" => ser(commands::fs::read_file_as_base64(req_str(a, "path")?).await),
        "file_exists" => ser(commands::fs::file_exists(req_str(a, "path")?).await),
        "get_file_modified_time" => ser(commands::fs::get_file_modified_time(req_str(a, "path")?).await),
        "get_file_size" => ser(commands::fs::get_file_size(req_str(a, "path")?).await),
        "get_file_md5" => ser(commands::fs::get_file_md5(req_str(a, "path")?).await),

        // file history
        "list_file_history" => ser(commands::file_history::list_file_history(
            req_str(a, "projectPath")?,
            req_str(a, "filePath")?,
        ).await),
        "restore_file_history" => ser(commands::file_history::restore_file_history(
            req_str(a, "projectPath")?,
            req_str(a, "filePath")?,
            req_str(a, "entryId")?,
        ).await),
        "get_file_history_stats" => ser(commands::file_history::get_file_history_stats(req_str(a, "projectPath")?).await),
        "clear_file_history" => ser(commands::file_history::clear_file_history(req_str(a, "projectPath")?).await),

        // project
        "create_project" => ser(commands::project::create_project(req_str(a, "name")?, req_str(a, "path")?)),
        "open_project" => ser(commands::project::open_project(req_str(a, "path")?)),
        "open_project_folder" => ser(commands::project::open_project_folder(app.clone(), req_str(a, "path")?)),
        "open_path_in_project" => ser(commands::project::open_path_in_project(
            app.clone(),
            req_str(a, "projectPath")?,
            req_str(a, "targetPath")?,
        )),
        "export_project_archive" => ser(commands::project_maintenance::export_project_archive(
            req_str(a, "projectPath")?,
            req_str(a, "destination")?,
        ).await),
        "import_project_archive" => ser(commands::project_maintenance::import_project_archive(
            req_str(a, "archivePath")?,
            req_str(a, "destination")?,
        ).await),
        "rebuild_wiki_index" => ser(commands::project_maintenance::rebuild_wiki_index(req_str(a, "projectPath")?).await),

        // search
        "search_project" => ser(commands::search::search_project(
            req_str(a, "projectPath")?,
            req_str(a, "query")?,
            opt_usize(a, "topK"),
            opt_bool(a, "includeContent"),
            opt_de::<Vec<f32>>(a, "queryEmbedding")?,
            opt_de(a, "embeddingConfig")?,
        ).await),
        "embedding_fetch" => ser(commands::search::embedding_fetch(
            req_str(a, "text")?,
            de(req_val(a, "cfg")?)?,
            opt_usize(a, "maxRetries"),
        ).await),
        "embedding_fetch_batch" => ser(commands::search::embedding_fetch_batch(
            de(req_val(a, "texts")?)?,
            de(req_val(a, "cfg")?)?,
        ).await),
        "get_page_links" => ser(commands::search::get_page_links(
            req_str(a, "projectPath")?,
            req_str(a, "filePath")?,
        ).await),
        "web_search" => ser(commands::external_search::web_search(
            req_str(a, "query")?,
            de(req_val(a, "config")?)?,
            opt_usize(a, "maxResults"),
        ).await),
        "anytxt_search" => ser(commands::external_search::anytxt_search(
            req_str(a, "query")?,
            de(req_val(a, "config")?)?,
            opt_usize(a, "maxResults"),
        ).await),

        // vector store
        "vector_upsert" => ser(commands::vectorstore::vector_upsert(
            req_str(a, "projectPath")?,
            req_str(a, "pageId")?,
            de(req_val(a, "embedding")?)?,
        ).await),
        "vector_search" => ser(commands::vectorstore::vector_search(
            req_str(a, "projectPath")?,
            de(req_val(a, "queryEmbedding")?)?,
            req_usize(a, "topK")?,
        ).await),
        "vector_delete" => ser(commands::vectorstore::vector_delete(req_str(a, "projectPath")?, req_str(a, "pageId")?).await),
        "vector_count" => ser(commands::vectorstore::vector_count(req_str(a, "projectPath")?).await),
        "vector_upsert_chunks" => ser(commands::vectorstore::vector_upsert_chunks(
            req_str(a, "projectPath")?,
            req_str(a, "pageId")?,
            de(req_val(a, "chunks")?)?,
        ).await),
        "vector_search_chunks" => ser(commands::vectorstore::vector_search_chunks(
            req_str(a, "projectPath")?,
            de(req_val(a, "queryEmbedding")?)?,
            req_usize(a, "topK")?,
        ).await),
        "vector_delete_page" => ser(commands::vectorstore::vector_delete_page(req_str(a, "projectPath")?, req_str(a, "pageId")?).await),
        "vector_count_chunks" => ser(commands::vectorstore::vector_count_chunks(req_str(a, "projectPath")?).await),
        "vector_clear_chunks" => ser(commands::vectorstore::vector_clear_chunks(req_str(a, "projectPath")?).await),
        "vector_optimize_chunks" => ser(commands::vectorstore::vector_optimize_chunks(req_str(a, "projectPath")?).await),
        "vector_legacy_row_count" => ser(commands::vectorstore::vector_legacy_row_count(req_str(a, "projectPath")?).await),
        "vector_drop_legacy" => ser(commands::vectorstore::vector_drop_legacy(req_str(a, "projectPath")?).await),

        // agent
        "agent_start_turn" => ser(crate::agent_start_turn(
            app.clone(),
            req_str(a, "projectId")?,
            de(req_val(a, "request")?)?,
        ).await),
        "agent_start_turn_stream" => ser(crate::agent_start_turn_stream(
            app.clone(),
            req_str(a, "projectId")?,
            de(req_val(a, "request")?)?,
        ).await),
        "agent_cancel_turn" => ser(crate::agent_cancel_turn(
            app.clone(),
            req_str(a, "projectId")?,
            req_str(a, "sessionId")?,
            opt_str(a, "runId"),
        )),
        "agent_get_session" => ser(crate::agent_get_session(
            app.clone(),
            req_str(a, "projectId")?,
            req_str(a, "sessionId")?,
            opt_usize(a, "limit"),
        )),
        "agent_list_sessions" => ser(crate::agent_list_sessions(app.clone(), req_str(a, "projectId")?)),
        "agent_list_skills" => ser_plain(agent::skills::agent_list_skills(req_str(a, "projectPath")?)),

        // misc app commands
        "clip_server_status" => ser_plain(crate::clip_server_status()),
        "api_server_status" => ser_plain(crate::api_server_status()),
        "api_server_reload_config" => ser_plain(crate::api_server_reload_config()),
        "mcp_server_entry_path" => ser(crate::mcp_server_entry_path(app.clone())),
        "set_proxy_env" => ser_plain(crate::set_proxy_env(de::<proxy::ProxyConfig>(req_val(a, "config")?)?)),
        "set_close_behavior" => ser(crate::set_close_behavior(
            req_str(a, "value")?,
            app.state::<crate::CloseBehaviorState>(),
        )),

        // CLI transports
        "claude_cli_detect" => ser(commands::claude_cli::claude_cli_detect().await),
        "claude_cli_spawn" => ser(commands::claude_cli::claude_cli_spawn(
            app.clone(),
            app.state::<commands::claude_cli::ClaudeCliState>(),
            req_str(a, "streamId")?,
            req_str(a, "model")?,
            de(req_val(a, "messages")?)?,
            req_bool(a, "isolateLocalConfig")?,
            opt_str(a, "workingDirectory"),
        ).await),
        "claude_cli_kill" => ser(commands::claude_cli::claude_cli_kill(
            app.state::<commands::claude_cli::ClaudeCliState>(),
            req_str(a, "streamId")?,
        ).await),
        "codex_cli_detect" => ser(commands::codex_cli::codex_cli_detect().await),
        "codex_cli_spawn" => ser(commands::codex_cli::codex_cli_spawn(
            app.clone(),
            app.state::<commands::codex_cli::CodexCliState>(),
            req_str(a, "streamId")?,
            req_str(a, "model")?,
            req_str(a, "prompt")?,
            req_bool(a, "isolateLocalConfig")?,
            opt_u64(a, "timeoutMinutes"),
            opt_str(a, "workingDirectory"),
        ).await),
        "codex_cli_kill" => ser(commands::codex_cli::codex_cli_kill(
            app.state::<commands::codex_cli::CodexCliState>(),
            req_str(a, "streamId")?,
        ).await),

        // image extraction
        "extract_pdf_images_cmd" => ser(commands::extract_images::extract_pdf_images_cmd(req_str(a, "path")?).await),
        "extract_office_images_cmd" => ser(commands::extract_images::extract_office_images_cmd(req_str(a, "path")?).await),
        "extract_and_save_pdf_images_cmd" => ser(commands::extract_images::extract_and_save_pdf_images_cmd(
            req_str(a, "sourcePath")?,
            req_str(a, "destDir")?,
            req_str(a, "relTo")?,
        ).await),
        "extract_and_save_office_images_cmd" => ser(commands::extract_images::extract_and_save_office_images_cmd(
            req_str(a, "sourcePath")?,
            req_str(a, "destDir")?,
            req_str(a, "relTo")?,
        ).await),

        // file sync
        "start_project_file_watcher" => ser(commands::file_sync::start_project_file_watcher(
            app.clone(),
            app.state::<commands::file_sync::FileSyncState>(),
            req_str(a, "projectId")?,
            req_str(a, "projectPath")?,
            opt_de(a, "sourceWatchConfig")?,
        )),
        "stop_project_file_watcher" => ser(commands::file_sync::stop_project_file_watcher(
            app.state::<commands::file_sync::FileSyncState>(),
        )),
        "rescan_project_files" => ser(commands::file_sync::rescan_project_files(
            app.clone(),
            req_str(a, "projectId")?,
            req_str(a, "projectPath")?,
            opt_de(a, "sourceWatchConfig")?,
        )),
        "get_file_change_queue" => ser(commands::file_sync::get_file_change_queue(req_str(a, "projectPath")?)),
        "retry_file_change_task" => ser(commands::file_sync::retry_file_change_task(
            app.clone(),
            req_str(a, "projectId")?,
            req_str(a, "projectPath")?,
            req_str(a, "taskId")?,
        )),
        "ignore_file_change_task" => ser(commands::file_sync::ignore_file_change_task(
            app.clone(),
            req_str(a, "projectId")?,
            req_str(a, "projectPath")?,
            req_str(a, "taskId")?,
        )),

        _ => Err(format!("unknown command: {name}")),
    }
}
