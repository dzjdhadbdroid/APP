//! Sitcom Studio — application desktop 100 % Rust (egui/eframe + SQLite).

mod db;
mod graph;
mod story_date;
mod ui;

use std::path::PathBuf;

use eframe::egui;

/// Dossier de données local de l'utilisateur (modifiable via SITCOM_STUDIO_DATA).
fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("SITCOM_STUDIO_DATA") {
        return d.into();
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(d) = std::env::var_os("APPDATA") {
            return PathBuf::from(d).join("SitcomStudio");
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(h) = std::env::var_os("HOME") {
            return PathBuf::from(h).join("Library/Application Support/SitcomStudio");
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(d) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(d).join("sitcom-studio");
        }
        if let Some(h) = std::env::var_os("HOME") {
            return PathBuf::from(h).join(".local/share/sitcom-studio");
        }
    }
    PathBuf::from("sitcom-studio-data")
}

fn main() -> eframe::Result<()> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir).expect("impossible de créer le dossier de données");
    let conn = db::open(&dir.join("sitcom_studio.sqlite"))
        .expect("impossible d'ouvrir la base de données");
    // Protection contre la perte de données : copie au lancement, 10 conservées.
    if let Err(e) = db::auto_backup(&conn, &dir, 10) {
        eprintln!("sauvegarde automatique impossible : {e}");
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([1000.0, 700.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Sitcom Studio",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(ui::StudioApp::new(conn, dir)))
        }),
    )
}
