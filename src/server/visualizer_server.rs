use axum::{
    routing::get,
    Router,
    Json,
    extract::State,
};
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::cors::CorsLayer;
use serde_json::{Value, json};

use crate::memory::MemoryStore;

#[derive(Clone)]
struct ServerState {
    memory: Arc<MemoryStore>,
}

pub async fn spawn_visualizer_server(memory: Arc<MemoryStore>) {
    // Fire-and-forget spawn — this task runs forever (axum::serve).
    // NEVER .await the JoinHandle here, it would block startup.
    let handle = tokio::spawn(async move {
        tracing::info!("[PANOPTICON] 👁️  Visualizer Server starting on http://0.0.0.0:3030");
        
        let state = ServerState { memory };

        // Ensure the public directory exists for ServeDir
        let public_dir = std::path::Path::new("src/server/public");
        if !public_dir.exists() {
            let _ = tokio::fs::create_dir_all(public_dir).await;
        }

        let app = Router::new()
            .route("/api/neo4j", get(api_neo4j))
            .route("/api/turing_grid", get(api_turing_grid))
            .route("/api/working_memory", get(api_working_memory))
            // Fallback for serving the actual interactive HTML dashboard:
            .fallback_service(
                ServeDir::new("src/server/public")
                    .fallback(ServeFile::new("src/server/public/index.html"))
            )
            .layer(CorsLayer::permissive())
            .with_state(state);

        let port: u16 = 3030; // Mesh-governed: creator-key protected
        let addr = format!("0.0.0.0:{}", port);
        match TcpListener::bind(&addr).await {
            Ok(listener) => {
                tracing::info!("[PANOPTICON] 👁️  Visualizer Server bound on {}", addr);
                if let Err(e) = axum::serve(listener, app).await {
                    tracing::error!("[PANOPTICON] ❌ Server error: {}", e);
                }
            }
            Err(e) => {
                tracing::error!("[PANOPTICON] ❌ Failed to bind port {} — is it already in use? Error: {}", port, e);
                tracing::error!("[PANOPTICON] 💡 Check with: lsof -i :{}", port);
            }
        }
    });

    // Non-blocking panic monitor — logs if the spawn dies without blocking startup
    tokio::spawn(async move {
        match handle.await {
            Ok(_) => tracing::warn!("[PANOPTICON] Visualizer server task exited unexpectedly"),
            Err(e) => tracing::error!("[PANOPTICON] ❌ Visualizer spawn PANICKED: {:?}", e),
        }
    });
}


// ─── ENDPOINTS ──────────────────────────────────────────────────────────

async fn api_neo4j(State(state): State<ServerState>) -> Json<Value> {
    let (nodes, edges) = state.memory.synaptic.export_graph().await;
    Json(json!({
        "nodes": nodes,
        "edges": edges
    }))
}

async fn api_turing_grid(State(state): State<ServerState>) -> Json<Value> {
    let grid_lock = state.memory.turing_grid.lock().await;
    // Collect cell coordinates and data
    // We package the data compactly so the frontend plotting engine doesn't stutter on massive blobs.
    let mut cells = Vec::new();
    for (coord_str, cell) in grid_lock.cells.iter() {
        let parts: Vec<&str> = coord_str.split(',').collect();
        let (x, y, z) = if parts.len() == 3 {
            (
                parts[0].parse::<i32>().unwrap_or(0),
                parts[1].parse::<i32>().unwrap_or(0),
                parts[2].parse::<i32>().unwrap_or(0)
            )
        } else {
            (0, 0, 0)
        };
        cells.push(json!({
            "x": x,
            "y": y,
            "z": z,
            "daemon": cell.daemon_active,
            "format": cell.format,
            "length": cell.content.len(),
            "links": cell.links,
        }));
    }
    
    let cursor = grid_lock.get_cursor();
    
    Json(json!({
        "cursor": { "x": cursor.0, "y": cursor.1, "z": cursor.2 },
        "cells": cells
    }))
}

async fn api_working_memory(State(state): State<ServerState>) -> Json<Value> {
    let current_tokens = state.memory.working.current_tokens().await;
    let max_tokens = state.memory.working.max_tokens();
    let events = state.memory.working.get_all_events().await;
    
    // We return just the headers to limit payload sizing.
    let event_summaries: Vec<Value> = events.iter().map(|e| {
        json!({
            "author": e.author_name,
            "platform": e.platform,
            "length": e.content.len(),
        })
    }).collect();

    Json(json!({
        "current_tokens": current_tokens,
        "max_tokens": max_tokens,
        "events": event_summaries
    }))
}
