pub mod commands;
pub mod db;

use std::sync::Mutex;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let conn = db::open(&dir.join("sitcom_studio.sqlite"))?;
            app.manage(commands::AppState(Mutex::new(conn)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_nodes,
            commands::create_node,
            commands::delete_node,
            commands::create_link,
            commands::node_links,
            commands::reachable,
        ])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de Sitcom Studio");
}
