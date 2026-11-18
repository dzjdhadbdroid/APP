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
migrations/0001_core.sql   schéma noyau (nœuds, liens, événements, scènes, alertes, FTS5)
src/
├─ main.rs                 point d'entrée, dossier de données, sauvegarde auto au lancement
├─ db/                     accès aux données
│  ├─ mod.rs               connexion, migrations (user_version), sauvegardes
│  ├─ nodes.rs             CRUD des nœuds, recherche plein texte
│  ├─ links.rs             liens typés, voisinage direct/indirect (CTE récursive)
│  └─ timeline.rs          3 ordres : réel / diffusion / présentation
├─ graph/layout.rs         placement par forces (Rust pur, testé)
├─ story_date.rs           dates fictives partielles → clé de tri (Rust pur, testé)
└─ ui/                     interface egui
   ├─ mod.rs               navigation, tableau de bord, fiches, recherche
   ├─ graph_view.rs        Module 12 : Universe Map
   └─ timeline_view.rs     Module 13 : Story Timeline
```

## État d'avancement
| Fonction | État |
|---|---|
| Schéma, migrations, CRUD, liens, recherche FTS5 | écrit, testé en SQL |
| Fiches (titre, résumé, notes, connexions éditables) | écrit |
| Universe Map : zoom/déplacement, filtres, exploration, isolés, surlignage | écrit |
| Timeline : chronologie réelle, diffusion (réordonnable), scènes, filtre par élément | écrit |
| Sauvegarde auto (10 conservées) + manuelle | écrit |
| Fiches détaillées (champs psychologie, biographie…) | **à faire** (colonnes en base, pas encore dans l'UI) |
| Éditeur de scénario (actes/séquences/dialogues) | **à faire** |
| Détection d'incohérences temporelles / alertes | **à faire** (tables `knowledge`, `alerts` prêtes) |
| Chronologie comparative de plusieurs personnages, vue saison | **à faire** |
| Historique des modifications / restauration, export JSON/Markdown/PDF, import | **à faire** |
| Module IA (trait `LlmProvider`, validation humaine) | **à faire** |

## Limites actuelles
Le réseau est coupé dans l'environnement de développement : `eframe` et `rusqlite` n'ont pas pu
être téléchargés, donc **l'application complète n'a jamais été compilée**. Vérifié : syntaxe de
tous les fichiers, tests réels de `graph/layout.rs` et `story_date.rs` (compilés avec rustc),
et toutes les requêtes SQL rejouées dans SQLite. Les tests Rust de `db/` et la compilation
de `ui/` restent à lancer : `cargo test` puis `cargo run`.
