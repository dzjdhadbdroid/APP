-- Rôle d'un personnage dans l'univers : principal ou secondaire (navigation « Personnages »).
ALTER TABLE characters ADD COLUMN role TEXT NOT NULL DEFAULT 'main' CHECK (role IN ('main', 'secondary'));
