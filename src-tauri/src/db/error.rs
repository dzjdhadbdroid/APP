use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("erreur SQLite : {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("élément introuvable : {0}")]
    NotFound(i64),
    #[error("type d'élément inconnu : {0}")]
    UnknownKind(String),
    #[error("verrou de base de données empoisonné")]
    Poisoned,
}

pub type DbResult<T> = Result<T, DbError>;

// Nécessaire pour renvoyer l'erreur depuis une commande Tauri.
impl Serialize for DbError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}
