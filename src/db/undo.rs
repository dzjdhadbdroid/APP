//! Annuler / rétablir. Des déclencheurs TEMPORAIRES (propres à la connexion, jamais écrits dans
//! le fichier) notent, pour chaque modification, la requête SQL inverse. Une « action » de
//! l'utilisateur = un groupe de requêtes ; annuler = rejouer le groupe à l'envers, ce qui
//! enregistre au passage le groupe inverse (rétablir), et inversement.
//! Hors journal : index de recherche, alertes (recalculées), réglages, PDF joints.

use rusqlite::Connection;

use super::{Atomic, DbResult};

const EXCLUDED_PREFIXES: [&str; 6] = [
    "search_index",
    "alerts",
    "settings",
    "attachments",
    "sqlite_",
    "undo_",
];
/// Nombre maximal d'actions annulables conservées.
const MAX_GROUPS: i64 = 100;

fn q(name: &str) -> String {
    format!("\"{name}\"")
}

/// Installe le journal sur la connexion (à appeler une fois, après les migrations).
pub fn install(conn: &Connection) -> DbResult<()> {
    conn.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS undo_ctl(enabled INTEGER NOT NULL, grp INTEGER NOT NULL, kind TEXT NOT NULL);
         DELETE FROM undo_ctl;
         INSERT INTO undo_ctl VALUES (0, 0, 'u');
         CREATE TEMP TABLE IF NOT EXISTS undo_log(id INTEGER PRIMARY KEY AUTOINCREMENT, grp INTEGER NOT NULL, kind TEXT NOT NULL, sql TEXT NOT NULL);
         DELETE FROM undo_log;",
    )?;
    let tables: Vec<String> = {
        let mut stmt =
            conn.prepare("SELECT name FROM main.sqlite_master WHERE type = 'table' ORDER BY name")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for t in tables
        .iter()
        .filter(|t| !EXCLUDED_PREFIXES.iter().any(|p| t.starts_with(p)))
    {
        let cols: Vec<String> = {
            let mut stmt = conn.prepare(&format!("PRAGMA main.table_info({})", q(t)))?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let sep = " || ',' || ";
        let values = |p: &str| {
            cols.iter()
                .map(|c| format!("quote({p}.{})", q(c)))
                .collect::<Vec<_>>()
                .join(sep)
        };
        let names = cols.iter().map(|c| q(c)).collect::<Vec<_>>().join(",");
        let sets = cols
            .iter()
            .map(|c| format!("'{}=' || quote(old.{})", q(c), q(c)))
            .collect::<Vec<_>>()
            .join(sep);
        let qt = q(t);
        let guard = "WHEN (SELECT enabled FROM undo_ctl) = 1";
        conn.execute_batch(&format!(
            "DROP TRIGGER IF EXISTS temp.undo_{t}_i;
             DROP TRIGGER IF EXISTS temp.undo_{t}_d;
             DROP TRIGGER IF EXISTS temp.undo_{t}_u;
             CREATE TEMP TRIGGER undo_{t}_i AFTER INSERT ON main.{qt} {guard} BEGIN
               INSERT INTO undo_log(grp, kind, sql)
               SELECT grp, kind, 'DELETE FROM {qt} WHERE rowid=' || new.rowid FROM undo_ctl; END;
             CREATE TEMP TRIGGER undo_{t}_d AFTER DELETE ON main.{qt} {guard} BEGIN
               INSERT INTO undo_log(grp, kind, sql)
               SELECT grp, kind, 'INSERT INTO {qt}(rowid,{names_sql}) VALUES(' || old.rowid || ',' || {old_values} || ')' FROM undo_ctl; END;
             CREATE TEMP TRIGGER undo_{t}_u AFTER UPDATE ON main.{qt} {guard} BEGIN
               INSERT INTO undo_log(grp, kind, sql)
               SELECT grp, kind, 'UPDATE {qt} SET rowid=' || old.rowid || ',' || {sets} || ' WHERE rowid=' || new.rowid FROM undo_ctl; END;",
            names_sql = names.replace('\'', "''"),
            old_values = values("old"),
        ))?;
    }
    Ok(())
}

/// Début d'une action : les modifications qui suivent forment un nouveau groupe annulable.
pub fn begin(conn: &Connection) -> DbResult<()> {
    conn.execute(
        "UPDATE undo_ctl SET enabled = 1, grp = grp + 1, kind = 'u'",
        [],
    )?;
    Ok(())
}

/// Fin d'une action : si elle a modifié quelque chose, la pile « rétablir » est vidée.
pub fn end(conn: &Connection) -> DbResult<()> {
    conn.execute("UPDATE undo_ctl SET enabled = 0", [])?;
    let changed: i64 = conn.query_row(
        "SELECT COUNT(*) FROM undo_log WHERE kind = 'u' AND grp = (SELECT grp FROM undo_ctl)",
        [],
        |r| r.get(0),
    )?;
    if changed > 0 {
        conn.execute("DELETE FROM undo_log WHERE kind = 'r'", [])?;
        conn.execute(
            "DELETE FROM undo_log WHERE kind = 'u' AND grp <= (SELECT grp FROM undo_ctl) - ?1",
            [MAX_GROUPS],
        )?;
    }
    Ok(())
}

pub fn can_undo(conn: &Connection) -> bool {
    has(conn, "u")
}

pub fn can_redo(conn: &Connection) -> bool {
    has(conn, "r")
}

fn has(conn: &Connection, kind: &str) -> bool {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM undo_log WHERE kind = ?1)",
        [kind],
        |r| r.get::<_, bool>(0),
    )
    .unwrap_or(false)
}

/// Rejoue le dernier groupe `from` à l'envers ; l'inverse est enregistré dans la pile `to`.
fn replay(conn: &Connection, from: &str, to: &str) -> DbResult<bool> {
    let group: Option<i64> = conn.query_row(
        "SELECT MAX(grp) FROM undo_log WHERE kind = ?1",
        [from],
        |r| r.get(0),
    )?;
    let Some(group) = group else {
        return Ok(false);
    };
    let statements: Vec<String> = {
        let mut stmt =
            conn.prepare("SELECT sql FROM undo_log WHERE grp = ?1 AND kind = ?2 ORDER BY id DESC")?;
        let rows = stmt.query_map(rusqlite::params![group, from], |r| r.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    let tx = Atomic::begin(conn)?;
    // Les clés étrangères ne sont vérifiées qu'à la fin : l'ordre des restaurations n'importe pas.
    tx.execute_batch("PRAGMA defer_foreign_keys = ON")?;
    tx.execute(
        "UPDATE undo_ctl SET enabled = 1, grp = grp + 1, kind = ?1",
        [to],
    )?;
    let result = statements.iter().try_for_each(|s| tx.execute_batch(s));
    tx.execute("UPDATE undo_ctl SET enabled = 0", [])?;
    result?;
    tx.execute(
        "DELETE FROM undo_log WHERE grp = ?1 AND kind = ?2",
        rusqlite::params![group, from],
    )?;
    tx.commit()?;
    Ok(true)
}

pub fn undo(conn: &Connection) -> DbResult<bool> {
    replay(conn, "u", "r")
}

pub fn redo(conn: &Connection) -> DbResult<bool> {
    replay(conn, "r", "u")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{links, nodes, nodes::NodeKind, open_in_memory};

    fn titles(conn: &Connection) -> Vec<String> {
        let mut stmt = conn.prepare("SELECT title FROM nodes ORDER BY id").unwrap();
        let rows = stmt.query_map([], |r| r.get(0)).unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    fn act<T>(conn: &Connection, f: impl FnOnce(&Connection) -> T) -> T {
        begin(conn).unwrap();
        let r = f(conn);
        end(conn).unwrap();
        r
    }

    #[test]
    fn undo_and_redo_create_edit_delete() {
        let conn = open_in_memory().unwrap();
        install(&conn).unwrap();
        assert!(!can_undo(&conn));
        let a = act(&conn, |c| {
            nodes::create(c, NodeKind::Character, "Alice").unwrap()
        });
        let b = act(&conn, |c| {
            nodes::create(c, NodeKind::Place, "Cuisine").unwrap()
        });
        act(&conn, |c| {
            links::create(c, a, b, "lives_in", None, None).unwrap()
        });
        act(&conn, |c| {
            c.execute("UPDATE nodes SET title = 'Alicia' WHERE id = ?1", [a])
                .unwrap()
        });
        act(&conn, |c| nodes::delete(c, a).unwrap());
        assert_eq!(titles(&conn), vec!["Cuisine"]);
        assert!(links::all(&conn).unwrap().is_empty());

        assert!(undo(&conn).unwrap()); // la suppression, avec son lien et sa fiche détail
        assert_eq!(titles(&conn), vec!["Alicia", "Cuisine"]);
        assert_eq!(links::all(&conn).unwrap().len(), 1);
        assert!(undo(&conn).unwrap()); // le renommage
        assert_eq!(titles(&conn), vec!["Alice", "Cuisine"]);
        assert!(redo(&conn).unwrap());
        assert_eq!(titles(&conn), vec!["Alicia", "Cuisine"]);
        assert!(can_redo(&conn));

        // une nouvelle action vide la pile « rétablir »
        act(&conn, |c| {
            nodes::create(c, NodeKind::Object, "Clé").unwrap()
        });
        assert!(!can_redo(&conn));
        while undo(&conn).unwrap() {}
        assert!(titles(&conn).is_empty());
        assert!(!can_undo(&conn));
        assert!(!undo(&conn).unwrap());
    }

    #[test]
    fn changes_outside_an_action_are_not_journaled() {
        let conn = open_in_memory().unwrap();
        install(&conn).unwrap();
        nodes::create(&conn, NodeKind::Character, "Hors action").unwrap();
        assert!(!can_undo(&conn));
        act(&conn, |_| ()); // action vide : rien à annuler
        assert!(!can_undo(&conn));
    }
}
