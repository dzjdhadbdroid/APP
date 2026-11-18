#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("erreur SQLite : {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("élément introuvable : {0}")]
    NotFound(i64),
    #[error("type d'élément inconnu : {0}")]
    UnknownKind(String),
    #[error("valeur invalide : {0}")]
    Invalid(String),
    #[error("erreur de fichier : {0}")]
    Io(#[from] std::io::Error),
}

pub type DbResult<T> = Result<T, DbError>;
