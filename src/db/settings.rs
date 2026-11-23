//! Réglages clé/valeur stockés localement.

use rusqlite::{params, Connection, OptionalExtension};

use super::DbResult;

pub fn get(conn: &Connection, key: &str) -> DbResult<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?)
}

pub fn set(conn: &Connection, key: &str, value: &str) -> DbResult<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    #[test]
    fn set_get_and_overwrite() {
        let conn = open_in_memory().unwrap();
        assert_eq!(get(&conn, "ai.model").unwrap(), None);
        set(&conn, "ai.model", "a").unwrap();
        set(&conn, "ai.model", "b").unwrap();
        assert_eq!(get(&conn, "ai.model").unwrap().as_deref(), Some("b"));
    }
}
