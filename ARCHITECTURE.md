# Sitcom Studio — Architecture (100 % Rust)

## Principe directeur
Une seule source de vérité SQLite. Tout élément de l'univers est un **nœud** (`nodes`),
toutes les connexions sont des **liens typés** (`links`). La Universe Map, la Story Timeline,
la recherche et les fiches lisent les mêmes tables : aucune donnée dupliquée (Module 14).
Toute écriture passe par `StudioApp::apply`, qui recharge aussitôt les données partagées :
une modification est visible immédiatement dans tous les modules.

## Stack
Un seul crate Rust · **egui/eframe** (interface) · **rusqlite** (SQLite embarqué, FTS5) · local, hors ligne.

## Arborescence
```
migrations/                0001_core.sql (noyau), 0002_character_fields.sql, 0003_history_search.sql, 0004_settings.sql, 0005_script.sql
src/
├─ main.rs                 point d'entrée, dossier de données, sauvegarde auto au lancement
├─ db/                     accès aux données
│  ├─ mod.rs               connexion, migrations (user_version), sauvegardes
│  ├─ nodes.rs             CRUD des nœuds, recherche plein texte
│  ├─ fields.rs            champs détaillés des fiches (table de définitions par type)
│  ├─ knowledge.rs         ce que sait chaque personnage, et quand il l'apprend
│  ├─ alerts.rs            alertes calculées, stockées, ignorables
│  ├─ history.rs           versions d'une fiche (50 max), restauration réversible
│  ├─ transfer.rs          export JSON/Markdown, import JSON atomique
│  ├─ settings.rs          réglages clé/valeur
│  ├─ script.rs            blocs de scénario (action/dialogue/mise en scène), actes, séquences
│  ├─ places.rs            sous-lieux (hiérarchie, anti-cycle)
│  ├─ relations.rs         relations entre personnages (proximité, évolution, savoirs)
│  ├─ links.rs             liens typés, voisinage direct/indirect (CTE récursive)
│  └─ timeline.rs          3 ordres : réel / diffusion / présentation
├─ ai/                     module IA (facultatif, hors ligne par défaut)
│  ├─ prompt.rs            contexte, consignes, vérification des citations (Rust pur, testé)
│  ├─ context.rs           construit le contexte depuis la base
│  └─ provider.rs          Anthropic et API compatible OpenAI / modèle local
├─ consistency.rs          règles d'incohérence (Rust pur, testé)
├─ graph/layout.rs         placement par forces (Rust pur, testé)
├─ pdf.rs                  écriture PDF sans dépendance (Rust pur, testé)
├─ story_date.rs           dates fictives partielles → clé de tri (Rust pur, testé)
└─ ui/                     interface egui
   ├─ mod.rs               navigation, tableau de bord, fiches, recherche
   ├─ graph_view.rs        Module 12 : Universe Map
   ├─ alerts_view.rs       alertes de cohérence
   ├─ script_view.rs       onglet Scénario des scènes et des épisodes
   ├─ relations_view.rs    module Relations
   ├─ settings_view.rs     thème, zoom, éléments consultés récemment
   ├─ ai_view.rs           assistant IA
   ├─ data_view.rs         sauvegardes, restauration, export, import
   └─ timeline_view.rs     Module 13 : Story Timeline
```

## État d'avancement
| Fonction | État |
|---|---|
| Schéma, migrations, CRUD, liens, recherche FTS5 | écrit, testé en SQL |
| Fiches : champs détaillés par type (26 pour un personnage), notes, connexions éditables | écrit |
| Contenu des scènes (champ texte libre) | écrit |
| Alertes de cohérence : connaissance préalable, relations contradictoires, ignorables | écrit |
| Universe Map : zoom/déplacement, filtres, exploration, isolés, surlignage | écrit |
| Timeline : chronologie réelle, diffusion (réordonnable), scènes, filtre par élément | écrit |
| Sauvegarde auto (10 conservées) + manuelle, restauration d'une sauvegarde (avec sauvegarde de sécurité) | écrit |
| Historique des modifications d'une fiche + restauration réversible | écrit |
| Export JSON complet, export Markdown, import JSON (additif, atomique) | écrit |
| Recherche dans les champs détaillés et les dialogues | écrit |
| Éditeur de scénario structuré (actes, séquences, dialogues, mise en scène, ordre des scènes) | écrit |
| Règles d'incohérence : connaissance, relations, événement mentionné trop tôt, lieu interdit, objet qui disparaît | écrit |
| Sous-lieux (hiérarchie, anti-cycle, lien `contains` automatique) | écrit |
| Module Relations (catégorie, proximité 0–10, évolution, ce que chacun sait) | écrit |
| Chronologie : comparée (plusieurs personnages), par saison, filtre de période | écrit |
| Tableau de bord : épisodes en cours, consultés récemment ; Paramètres (thème, zoom) | écrit |
| Assistant IA : contexte choisi, 11 tâches prédéfinies, citations vérifiées, jamais d'écriture automatique | écrit |
| Export PDF (Rust pur, validé avec qpdf) | écrit |

## Limites actuelles
Le réseau est coupé dans l'environnement de développement : `eframe` et `rusqlite` n'ont pas pu
être téléchargés, donc **l'application complète n'a jamais été compilée**. Vérifié : syntaxe de
tous les fichiers, tests réels de `graph/layout.rs`, `story_date.rs` et `consistency.rs` (compilés avec rustc),
`ai/prompt.rs` (8 tests), et toutes les requêtes SQL rejouées dans SQLite. Les tests Rust de `db/` et la compilation
de `ui/` restent à lancer : `cargo test` puis `cargo run`.
