# Banc de la Task S0 du lot 5 : agents

Jetable, comme le reste de `spike/` : rien d'ici n'entre dans le produit. Il a son propre atelier
Cargo (`[workspace]` vide) et dépend des crates du produit par chemin. Les chiffres et les
verdicts sont dans `docs/superpowers/logs/2026-11-20-p1-lot5-s0-mesures.md` ; les sorties brutes,
dans `results/`.

- Les fonctions d'écriture de `mementum-crdt` (`write_nodes`, `write_inline`, `write_table`,
  `keep_ids`) sont `pub(crate)` : `src/main.rs` inclut leurs fichiers par chemin, à la racine du
  banc, pour que leurs `crate::…` se résolvent ici. La garde ne s'inclut pas ainsi (elle a un
  dossier de sous-modules) : `src/guard.rs` recopie ce que `foreign.rs` lui prend.
- yrs est celui du serveur (`vendor/yrs`, avec son correctif), le même paquet que celui de
  `mementum-crdt`.
- `Cargo.lock` part de celui de la racine.

## Ce que fait chaque commande

| Commande | Step | Ce qu'elle mesure |
|---|---|---|
| `diff [--only <cas>] [--reps N]` | 2 | `src/diff.rs`, prototype de `write_diff`, contre `rewrite_blocks` : 10 000 paragraphes (un bloc changé, inséré, retiré, déplacé ; la moitié ; une échelle de parts changées et insérées ; l'ordre renversé ; le contenu retiré écrit par le même client) et la fixture `all-blocks` |
| `inline-base <d>`, `inline-apply <d>` | 3 | menées par `probe/inline.mjs` : un texte marqué écrit par préfixe et suffixe communs, relu par le vrai Yjs, avec des frappes concurrentes du vrai Yjs |
| `narrow [--reps N]` | 4 | prototypes de `SpaceAccess::narrowed` sur 10 000 et 100 000 pages |
| `version <d> [--reps N]` | 4 bis | encodage, `PageDoc::check_state` et `Store::put_version` d'une page de 1 000 et 10 000 blocs |
| `mcp <port> [--serve both\|2026\|2025] [--token t] [--refuse méthode:401\|mismatch] [--log f]` | 1 | un `/mcp` d'un outil (`echo`), sans état, qui sert les deux révisions et journalise chaque requête |

## Refaire

Une commande lourde à la fois, en release. `D` est un dossier jetable, `T` un dossier de cible.

```sh
CARGO_TARGET_DIR=$T cargo build --release
B=$T/release/agents-bench.exe

$B diff                                   # Step 2, une dizaine de minutes en tout
$B diff --only meme-client                # le ramasse-miettes de R367 (même client)
node probe/inline.mjs $B $D/inline <chemin de node_modules/.pnpm/yjs@13.6.33/node_modules/yjs/dist/yjs.mjs>
$B narrow                                 # Step 4
$B version $D/version                     # Step 4 bis (écrit une base SQLite dans $D)
$B mcp 8103 --serve both --log $D/mcp.jsonl   # Step 1, puis un client MCP sur 127.0.0.1:8103
```

Pour le Step 1, Claude Code a été lancé en mode non interactif seulement, avec une configuration
à lui dans un dossier jetable (jamais `claude mcp add`) :

```sh
BANC_TOKEN=mmt_banc_factice claude -p "Appelle l'outil echo du serveur MCP banc avec le texte bonjour, puis réponds seulement par ce qu'il a rendu." \
  --mcp-config banc.json --strict-mcp-config --tools "" --allowedTools mcp__banc__echo \
  --no-session-persistence --model haiku --max-budget-usd 0.25 --setting-sources project
```

où `banc.json` vaut
`{"mcpServers":{"banc":{"type":"http","url":"http://127.0.0.1:8103/mcp","headers":{"Authorization":"Bearer ${BANC_TOKEN}"}}}}`.
Les requêtes reçues sont dans `results/mcp/*.requetes.jsonl` (le jeton factice n'y paraît que par
sa longueur), les lignes du journal de Claude Code sur le serveur dans `*.journal-mcp.txt`.
