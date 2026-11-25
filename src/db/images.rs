//! Image d'une fiche (portrait d'un personnage, plan d'un lieu…). Réduite à 640 px et
//! convertie en PNG avant d'être stockée dans la base, donc sauvegardée avec elle.

use std::collections::HashSet;
use std::io::Cursor;

use rusqlite::{params, Connection};

use super::{DbError, DbResult};

pub const MAX_INPUT_BYTES: usize = 20 * 1024 * 1024;
const MAX_SIDE: u32 = 640;

/// Décode (PNG ou JPEG), réduit si besoin et enregistre l'image de la fiche.
pub fn set(conn: &Connection, node: i64, bytes: &[u8]) -> DbResult<()> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(DbError::Invalid(
            "image trop volumineuse (20 Mo maximum)".into(),
        ));
    }
    let img = image::load_from_memory(bytes)
        .map_err(|_| DbError::Invalid("image illisible (formats acceptés : PNG, JPEG)".into()))?;
    let img = if img.width() > MAX_SIDE || img.height() > MAX_SIDE {
        img.thumbnail(MAX_SIDE, MAX_SIDE)
    } else {
        img
    };
    let mut png = Vec::new();
    img.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| DbError::Invalid(format!("conversion de l'image impossible : {e}")))?;
    conn.execute(
        "INSERT INTO node_images (node_id, data) VALUES (?1, ?2)
         ON CONFLICT(node_id) DO UPDATE SET data = excluded.data, added_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')",
        params![node, png],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, node: i64) -> DbResult<Option<Vec<u8>>> {
    match conn.query_row(
        "SELECT data FROM node_images WHERE node_id = ?1",
        [node],
        |r| r.get(0),
    ) {
        Ok(d) => Ok(Some(d)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn delete(conn: &Connection, node: i64) -> DbResult<()> {
    conn.execute("DELETE FROM node_images WHERE node_id = ?1", [node])?;
    Ok(())
}

/// Fiches qui ont une image.
pub fn ids(conn: &Connection) -> DbResult<HashSet<i64>> {
    let mut stmt = conn.prepare("SELECT node_id FROM node_images")?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{nodes, nodes::NodeKind, open_in_memory};

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([200, 50, 50, 255]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn stores_resizes_replaces_and_cascades() {
        let conn = open_in_memory().unwrap();
        let a = nodes::create(&conn, NodeKind::Character, "Alice").unwrap();
        assert!(get(&conn, a).unwrap().is_none());
        set(&conn, a, &png(1600, 800)).unwrap();
        let stored = image::load_from_memory(&get(&conn, a).unwrap().unwrap()).unwrap();
        assert_eq!((stored.width(), stored.height()), (640, 320));
        set(&conn, a, &png(10, 10)).unwrap(); // remplace
        let stored = image::load_from_memory(&get(&conn, a).unwrap().unwrap()).unwrap();
        assert_eq!(stored.width(), 10);
        assert_eq!(ids(&conn).unwrap().len(), 1);
        assert!(set(&conn, a, b"pas une image").is_err());
        nodes::delete(&conn, a).unwrap(); // supprimée avec la fiche
        assert!(ids(&conn).unwrap().is_empty());
        let b = nodes::create(&conn, NodeKind::Place, "Cuisine").unwrap();
        set(&conn, b, &png(5, 5)).unwrap();
        delete(&conn, b).unwrap();
        assert!(get(&conn, b).unwrap().is_none());
    }
}
