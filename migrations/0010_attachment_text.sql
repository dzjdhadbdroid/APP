-- Texte extrait des PDF rattachés (affiché dans l'app et trouvé par la recherche globale).
ALTER TABLE attachments ADD COLUMN text TEXT NOT NULL DEFAULT '';
