//! Couche fine Tauri : chaque écriture émet `graph:changed` pour que la
//! Universe Map et la Timeline se rafraîchissent immédiatement.

use std::sync::Mutex;

use rusqlite::Connection;
use tauri::{AppHandle, Emitter, State};

use crate::db::{
    links::{self, Link},
    nodes::{self, Node, NodeKind},
    DbError, DbResult,
};

pub struct AppState(pub Mutex<Connection>);

impl AppState {
    fn conn(&self) -> DbResult<std::sync::MutexGuard<'_, Connection>> {
        self.0.lock().map_err(|_| DbError::Poisoned)
    }
}

fn notify(app: &AppHandle) {
    let _ = app.emit("graph:changed", ());
}

#[tauri::command]
pub fn list_nodes(state: State<AppState>, kind: NodeKind) -> DbResult<Vec<Node>> {
    nodes::list_by_kind(&*state.conn()?, kind)
}

#[tauri::command]
pub fn create_node(
    app: AppHandle,
    state: State<AppState>,
    kind: NodeKind,
    title: String,
) -> DbResult<i64> {
    let id = nodes::create(&*state.conn()?, kind, &title)?;
    notify(&app);
    Ok(id)
}

#[tauri::command]
pub fn delete_node(app: AppHandle, state: State<AppState>, id: i64) -> DbResult<()> {
    nodes::delete(&*state.conn()?, id)?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub fn create_link(
    app: AppHandle,
    state: State<AppState>,
    src_id: i64,
    dst_id: i64,
    link_type: String,
    label: Option<String>,
    closeness: Option<i64>,
) -> DbResult<i64> {
    let id = links::create(
        &*state.conn()?,
        src_id,
        dst_id,
        &link_type,
        label.as_deref(),
        closeness,
    )?;
    notify(&app);
    Ok(id)
}

#[tauri::command]
pub fn node_links(state: State<AppState>, node_id: i64) -> DbResult<Vec<Link>> {
    links::for_node(&*state.conn()?, node_id)
}

#[tauri::command]
pub fn reachable(state: State<AppState>, start: i64, max_depth: i64) -> DbResult<Vec<(i64, i64)>> {
    links::reachable(&*state.conn()?, start, max_depth)
}
