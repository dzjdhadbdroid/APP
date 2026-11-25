//! Couche d'accès aux données : connexion, migrations, dépôts (repositories).

pub mod alerts;
pub mod analysis;
pub mod attachments;
pub mod error;
pub mod fields;
pub mod history;
pub mod knowledge;
pub mod links;
pub mod nodes;
pub mod pdf_analysis;
pub mod places;
pub mod plots;
pub mod relations;
pub mod script;
pub mod settings;
pub mod tags;
pub mod timeline;
pub mod transfer;
pub mod trash;
pub mod undo;

use std::path::Path;

use rusqlite::Connection;

pub use error::{DbError, DbResult};

/// Bloc atomique imbriquable (SAVEPOINT) : tout est annulé si `commit` n'est pas atteint
/// (retour anticipé par `?`, erreur…). Se comporte comme une `&Connection`.
pub struct Atomic<'a> {
    conn: &'a Connection,
    done: bool,
}

impl<'a> Atomic<'a> {
    pub fn begin(conn: &'a Connection) -> DbResult<Self> {
        conn.execute_batch("SAVEPOINT atomic_op")?;
        Ok(Self { conn, done: false })
    }

    pub fn commit(mut self) -> DbResult<()> {
        self.done = true;
        self.conn.execute_batch("RELEASE atomic_op")?;
        Ok(())
    }
}

impl std::ops::Deref for Atomic<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.conn
    }
}

impl Drop for Atomic<'_> {
    fn drop(&mut self) {
        if !self.done {
            let _ = self
                .conn
                .execute_batch("ROLLBACK TO atomic_op; RELEASE atomic_op");
        }
    }
}

/// Migrations ordonnées. La version appliquée est stockée dans `PRAGMA user_version`.
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../../migrations/0001_core.sql")),
    (
        2,
        include_str!("../../migrations/0002_character_fields.sql"),
    ),
    (3, include_str!("../../migrations/0003_history_search.sql")),
    (4, include_str!("../../migrations/0004_settings.sql")),
    (5, include_str!("../../migrations/0005_script.sql")),
    (6, include_str!("../../migrations/0006_character_role.sql")),
    (7, include_str!("../../migrations/0007_tags.sql")),
    (8, include_str!("../../migrations/0008_trash.sql")),
    (9, include_str!("../../migrations/0009_attachments.sql")),
    (
        10,
        include_str!("../../migrations/0010_attachment_text.sql"),
    ),
];

pub fn open(path: &Path) -> DbResult<Connection> {
    init(Connection::open(path)?)
}

pub fn open_in_memory() -> DbResult<Connection> {
    init(Connection::open_in_memory()?)
}

fn init(mut conn: Connection) -> DbResult<Connection> {
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
    migrate(&mut conn)?;
    Ok(conn)
}

/// Applique, dans l'ordre et chacune dans une transaction, les migrations manquantes.
pub fn migrate(conn: &mut Connection) -> DbResult<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (version, sql) in MIGRATIONS.iter().filter(|(v, _)| *v > current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", *version)?;
        tx.commit()?;
    }
    Ok(())
}

/// Sauvegarde complète et cohérente de la base (fonctionne même base ouverte).
pub fn backup(conn: &Connection, dest: &Path) -> DbResult<()> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let dest = dest
        .to_str()
        .ok_or_else(|| DbError::Invalid("chemin de sauvegarde non UTF-8".into()))?;
    conn.execute("VACUUM INTO ?1", [dest])?;
    Ok(())
}

pub(crate) fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Sauvegarde horodatée `<prefixe>_<secondes>.sqlite` dans `<dir>/backups/`.
pub fn backup_with_prefix(
    conn: &Connection,
    dir: &Path,
    prefix: &str,
) -> DbResult<std::path::PathBuf> {
    let path = dir
        .join("backups")
        .join(format!("{prefix}_{:012}.sqlite", unix_secs()));
    backup(conn, &path)?;
    Ok(path)
}

/// Sauvegarde manuelle horodatée dans `<dir>/backups/`. Retourne le chemin créé.
pub fn manual_backup(conn: &Connection, dir: &Path) -> DbResult<std::path::PathBuf> {
    backup_with_prefix(conn, dir, "manuel")
}

/// Sauvegardes existantes, de la plus récente à la plus ancienne.
pub fn list_backups(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut v: Vec<std::path::PathBuf> = std::fs::read_dir(dir.join("backups"))
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    v.retain(|p| p.extension().is_some_and(|e| e == "sqlite"));
    v.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    v
}

/// Remplace tout le contenu de la base ouverte par celui d'une sauvegarde, puis la met à niveau.
/// Le fichier est vérifié avant : un fichier quelconque ne peut pas écraser la base.
pub fn restore(conn: &mut Connection, from: &Path) -> DbResult<()> {
    {
        let src = Connection::open_with_flags(from, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        src.query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get::<_, i64>(0))
            .map_err(|_| {
                DbError::Invalid("ce fichier n'est pas une sauvegarde Sitcom Studio".into())
            })?;
        let b = rusqlite::backup::Backup::new(&src, &mut *conn)?;
        b.run_to_completion(64, std::time::Duration::from_millis(0), None)?;
    }
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    migrate(conn)
}

/// Sauvegarde automatique (au lancement) ; ne conserve que les `keep` plus récentes.
pub fn auto_backup(conn: &Connection, dir: &Path, keep: usize) -> DbResult<()> {
    let folder = dir.join("backups");
    let path = folder.join(format!("auto_{:012}.sqlite", unix_secs()));
    if !path.exists() {
        backup(conn, &path)?;
    }
    let mut autos: Vec<_> = std::fs::read_dir(&folder)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("auto_"))
        })
        .collect();
    autos.sort(); // noms horodatés sur 12 chiffres : ordre alphabétique = ordre chronologique
    while autos.len() > keep {
        std::fs::remove_file(autos.remove(0))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_backup_keeps_only_the_latest() {
        let conn = open_in_memory().unwrap();
        let dir = std::env::temp_dir().join(format!("sitcom_auto_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("backups")).unwrap();
        for i in 1..=5u64 {
            std::fs::write(
                dir.join("backups").join(format!("auto_{i:012}.sqlite")),
                b"x",
            )
            .unwrap();
        }
        auto_backup(&conn, &dir, 3).unwrap();
        let n = std::fs::read_dir(dir.join("backups")).unwrap().count();
        assert_eq!(n, 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_brings_back_the_saved_state_and_rejects_junk() {
        let mut conn = open_in_memory().unwrap();
        nodes::create(&conn, nodes::NodeKind::Character, "Alice").unwrap();
        let dir = std::env::temp_dir().join(format!("sitcom_restore_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let saved = manual_backup(&conn, &dir).unwrap();
        nodes::create(&conn, nodes::NodeKind::Character, "Bob").unwrap();
        assert_eq!(nodes::all(&conn).unwrap().len(), 2);
        restore(&mut conn, &saved).unwrap();
        let titles: Vec<_> = nodes::all(&conn)
            .unwrap()
            .into_iter()
            .map(|n| n.title)
            .collect();
        assert_eq!(titles, ["Alice"]);
        let junk = dir.join("junk.sqlite");
        std::fs::write(&junk, b"pas une base").unwrap();
        assert!(restore(&mut conn, &junk).is_err());
        assert_eq!(nodes::all(&conn).unwrap().len(), 1); // base intacte
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn backup_produces_a_readable_copy() {
        let conn = open_in_memory().unwrap();
        nodes::create(&conn, nodes::NodeKind::Character, "Alice").unwrap();
        let path =
            std::env::temp_dir().join(format!("sitcom_backup_{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        backup(&conn, &path).unwrap();
        let copy = Connection::open(&path).unwrap();
        let n: i64 = copy
            .query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        drop(copy);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn migrate_is_idempotent() {
        let mut conn = open_in_memory().unwrap();
        migrate(&mut conn).unwrap(); // deuxième passage : ne doit rien refaire
        let v: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, 10);
    }

    #[test]
    fn upgrades_an_existing_v1_database_without_losing_data() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute_batch(MIGRATIONS[0].1).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute(
            "INSERT INTO nodes (kind, title) VALUES ('character', 'Alice')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO characters (id) VALUES (1)", [])
            .unwrap();
        migrate(&mut conn).unwrap();
        let (v, n): (i64, i64) = (
            conn.pragma_query_value(None, "user_version", |r| r.get(0))
                .unwrap(),
            conn.query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))
                .unwrap(),
        );
        assert_eq!((v, n), (10, 1));
        conn.execute("UPDATE characters SET evolution = 'x' WHERE id = 1", [])
            .unwrap(); // colonne ajoutée
    }
}
