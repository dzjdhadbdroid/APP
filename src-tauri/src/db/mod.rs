//! Couche d'accès aux données : connexion, migrations, dépôts (repositories).

pub mod error;
pub mod links;
pub mod nodes;

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

#[cfg(test)]
mod tests {
    use super::*;

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
