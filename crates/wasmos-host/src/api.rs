use crate::{scheduler::SpawnRequest, shell::ShellResponse, AppState};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};
use tower_http::services::ServeDir;

pub fn build_router(state: AppState) -> Router {
    let web_root = std::env::var("NEWOS_WEB_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("webos"));

    Router::new()
        .route("/api/health", get(health))
        .route("/api/shell", post(run_shell))
        .route("/api/tasks", get(list_tasks).post(spawn_task))
        .route("/api/files", get(list_files).post(write_file))
        .route("/api/files/{path}", delete(delete_file))
        .route("/api/apps", get(list_apps))
        .route("/api/gui/events", get(gui_events))
        .nest_service("/desktop", ServeDir::new(web_root.join("desktop")))
        .nest_service("/apps", ServeDir::new(web_root.join("apps")))
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({"ok": true, "service": "wasmos-host"}))
}

#[derive(Deserialize)]
pub struct ShellCommand {
    pub command: String,
}

async fn run_shell(
    State(state): State<AppState>,
    Json(input): Json<ShellCommand>,
) -> Json<ShellResponse> {
    Json(state.shell.execute(&input.command).await)
}

async fn list_tasks(State(state): State<AppState>) -> Json<Vec<serde_json::Value>> {
    Json(state.scheduler.snapshot())
}

async fn spawn_task(
    State(state): State<AppState>,
    Json(req): Json<SpawnRequest>,
) -> impl IntoResponse {
    match state.scheduler.spawn(req).await {
        Ok(task) => (StatusCode::ACCEPTED, Json(task)).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": error.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct WriteFileRequest {
    pub path: String,
    pub contents: String,
}

async fn list_files(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let path = query.get("path").map(String::as_str).unwrap_or("/data");
    match state.vfs.read_dir(path) {
        Ok(entries) => (StatusCode::OK, Json(entries)).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": error.to_string()})),
        )
            .into_response(),
    }
}

async fn write_file(
    State(state): State<AppState>,
    Json(req): Json<WriteFileRequest>,
) -> impl IntoResponse {
    match state.vfs.write_file(&req.path, req.contents.into_bytes()) {
        Ok(_) => (StatusCode::CREATED, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": error.to_string()})),
        )
            .into_response(),
    }
}

async fn delete_file(State(state): State<AppState>, Path(path): Path<String>) -> impl IntoResponse {
    let path = format!("/{}", path);
    match state.vfs.delete(&path) {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": error.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Serialize)]
pub struct AppManifest {
    pub id: String,
    pub title: String,
    pub entrypoint: String,
    pub sandbox: String,
}

async fn list_apps() -> Json<Vec<AppManifest>> {
    Json(vec![
        AppManifest {
            id: "calculator".into(),
            title: "Calculator".into(),
            entrypoint: "/apps/calculator/index.html".into(),
            sandbox: "allow-scripts".into(),
        },
        AppManifest {
            id: "notes".into(),
            title: "Notes".into(),
            entrypoint: "/apps/notes/index.html".into(),
            sandbox: "allow-scripts".into(),
        },
        AppManifest {
            id: "file-manager".into(),
            title: "Files".into(),
            entrypoint: "/apps/file-manager/index.html".into(),
            sandbox: "allow-scripts".into(),
        },
        AppManifest {
            id: "settings".into(),
            title: "Settings".into(),
            entrypoint: "/apps/settings/index.html".into(),
            sandbox: "allow-scripts".into(),
        },
    ])
}

async fn gui_events(State(state): State<AppState>) -> Json<Vec<serde_json::Value>> {
    Json(state.gui.drain_events())
}
