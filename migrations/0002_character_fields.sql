-- Champs de fiche personnage demandés au cahier des charges et absents du schéma initial.
ALTER TABLE characters ADD COLUMN age TEXT;
ALTER TABLE characters ADD COLUMN habits TEXT;
ALTER TABLE characters ADD COLUMN qualities TEXT;
ALTER TABLE characters ADD COLUMN flaws TEXT;
ALTER TABLE characters ADD COLUMN objectives TEXT;
ALTER TABLE characters ADD COLUMN contradictions TEXT;
ALTER TABLE characters ADD COLUMN catchphrases TEXT;
ALTER TABLE characters ADD COLUMN behaviors TEXT;
ALTER TABLE characters ADD COLUMN evolution TEXT;
ALTER TABLE characters ADD COLUMN key_events TEXT;
ALTER TABLE characters ADD COLUMN personality_changes TEXT;
