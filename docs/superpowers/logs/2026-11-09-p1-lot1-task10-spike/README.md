# Spike de la Task 10 du lot 1 (jetable, non compilé)

Preuves de l'échec du report des éditions faites sur un bloc déplacé de façon concurrente (Ruling R39 du registre `../2026-11-09-p1-lot1-ledger.md`, rapport de la Task 10). Gardé pour que le lot 3 n'ait pas à refaire l'exploration.

- `spike_skip_gc.rs` : test d'exploration (à poser dans `crates/mementum-crdt/tests/`). Il montre ce que yrs 0.28.1 laisse lire d'un bloc supprimé avec `skip_gc` (id illisible, texte relu par `diff_range`), et mesure taille d'état et chargement d'une page de 10 000 blocs après des réécritures complètes.
- `spike_budget_history.rs` : mêmes mesures depuis SQLite, à la manière de `load_10000_block_page_from_store_under_300ms` (à poser dans `crates/mementum-sync/tests/`). Attention : la valeur affichée « p95 » est le maximum des 20 chargements.
- `spike_page_skip_gc.diff` : le `skip_gc` de `PageDoc::empty` utilisé pendant l'essai.

Ces fichiers ne font partie d'aucune crate : ils ne sont ni compilés ni testés.
