//! Couche d'accès aux données : connexion, migrations, dépôts (repositories).

pub mod error;
pub mod links;
pub mod nodes;
pub mod timeline;

use std::path::Path;

use rusqlite::Connection;

pub use error::{DbError, DbResult};

/// Migrations ordonnées. La version appliquée est stockée dans `PRAGMA user_version`.
const MIGRATIONS: &[(i64, &str)] = &[(1, include_str!("../../migrations/0001_core.sql"))];

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

fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Sauvegarde manuelle horodatée dans `<dir>/backups/`. Retourne le chemin créé.
pub fn manual_backup(conn: &Connection, dir: &Path) -> DbResult<std::path::PathBuf> {
    let path = dir.join("backups").join(format!("manuel_{:012}.sqlite", unix_secs()));
    backup(conn, &path)?;
    Ok(path)
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
            std::fs::write(dir.join("backups").join(format!("auto_{i:012}.sqlite")), b"x").unwrap();
        }
        auto_backup(&conn, &dir, 3).unwrap();
        let n = std::fs::read_dir(dir.join("backups")).unwrap().count();
        assert_eq!(n, 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn backup_produces_a_readable_copy() {
        let conn = open_in_memory().unwrap();
        nodes::create(&conn, nodes::NodeKind::Character, "Alice").unwrap();
        let path = std::env::temp_dir().join(format!("sitcom_backup_{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        backup(&conn, &path).unwrap();
        let copy = Connection::open(&path).unwrap();
        let n: i64 = copy.query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0)).unwrap();
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
        assert_eq!(v, 1);
    }
}
