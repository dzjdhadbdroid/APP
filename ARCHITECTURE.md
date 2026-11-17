# Sitcom Studio — Architecture

## Principe directeur
Une seule source de vérité SQLite. Tout élément de l'univers est un **nœud** (`nodes`),
toutes les connexions sont des **liens typés** (`links`). La Universe Map, la Story Timeline,
la recherche et les fiches lisent les mêmes tables : aucune donnée dupliquée (Module 14).

## Stack
Rust + Tauri · React/TypeScript · SQLite (FTS5) · local, hors ligne.

## Arborescence prévue
```
app/
├─ src-tauri/
│  ├─ migrations/        0001_core.sql (fait), 0002_…
│  └─ src/
│     ├─ db/             connexion, migrations, repositories (accès aux données)
│     ├─ domain/         logique métier (personnages, épisodes, événements…)
│     ├─ graph/          Universe Map : voisinage, chemins, isolés, filtres
│     ├─ timeline/       3 ordres + détection d'incohérences temporelles
│     ├─ consistency/    règles d'alertes
│     ├─ search/         FTS5
│     ├─ ai/             trait `LlmProvider` (OpenAI / Anthropic / local), validation humaine
│     ├─ backup/         sauvegardes, export JSON/Markdown/PDF, import
│     └─ commands.rs     commandes Tauri (couche fine)
└─ src/                  React : pages, composants, store, vues graphe/timeline
```

## Modules 12–14 : comment ils s'appuient sur le modèle
- **Map (12)** : `nodes` = sommets, `links` = arêtes typées. Voisinage direct/indirect par CTE
  récursive. Isolés : vue `v_isolated_nodes`. Mise à jour dynamique : chaque commande Tauri
  d'écriture émet un événement `graph:changed` ; le front recharge le sous-graphe concerné.
- **Timeline (13)** : trois ordres distincts, une source :
  réel = `events.story_sort`, diffusion = `episodes.air_order`, présentation = `scenes.position`.
  Statuts, importance, personnages/lieux/épisodes/intrigues liés via `links`.
- **Interconnexion (14)** : navigation Personnage → Relations → Événements → Épisodes →
  Intrigues → Lieux = simples traversées de `links`. Historique d'un lien : `link_history`.
- **Cohérence temporelle** : table `knowledge` (qui apprend quoi, quand). Règle type :
  un personnage impliqué dans un événement dont `story_sort` précède celui où il apprend/rencontre
  le sujet requis → alerte `alerts` (gravité, ignorable).

## Étapes de développement
1. ✅ Schéma noyau + validation (ce commit local)
2. Squelette Tauri + couche `db` + migrations + tests Rust
3. CRUD nœuds/liens + événement `graph:changed`
4. Personnages, lieux, objets (fiches)
5. Épisodes + scènes
6. Intrigues
7. **Universe Map** (module 12)
8. **Story Timeline** (module 13) + règles temporelles
9. Recherche globale, alertes, sauvegardes/historique
10. Module IA (contexte sélectionnable, citations, validation obligatoire)

## Limites actuelles
Aucun code Rust/React compilé à ce stade : le réseau est coupé dans cet environnement
(impossible de télécharger les crates/paquets npm). Seul le SQL est testé.
