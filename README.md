# APP

Sitcom Studio — gestionnaire d'univers de sitcom, 100 % Rust (egui + SQLite).

```
cargo test   # tests
cargo run    # lancer l'application
```

Voir [ARCHITECTURE.md](ARCHITECTURE.md) pour le détail et l'état d'avancement.
Données dans `~/.local/share/sitcom-studio` (Linux), `%APPDATA%\SitcomStudio` (Windows), `~/Library/Application Support/SitcomStudio` (macOS) ; surchargeable avec `SITCOM_STUDIO_DATA`.
