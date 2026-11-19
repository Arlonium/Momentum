# Banc de la Task S0 du lot 4 : Tantivy contre FTS5

Jetable, comme le reste de `spike/` : rien d'ici n'entre dans le produit. Il a son propre
atelier Cargo (`[workspace]` vide), pour ne pas ajouter Tantivy aux dépendances du serveur avant
la décision L1. Les chiffres et le verdict sont dans
`docs/superpowers/logs/2026-11-19-p1-lot4-s0-mesures.md` ; les sorties brutes, dans `results/`.

- Tantivy 0.27.0 (ce que cargo résout le 2026-11-19), fonctions par défaut.
- SQLite : `libsqlite3-sys` épinglé à 0.37.0, la version du `Cargo.lock` de la racine
  (SQLite 3.51.3), lu par `rusqlite` 0.39 ; `fts-check` passe par sqlx 0.9, avec les fonctions
  du `Cargo.toml` de la racine.

## Corpus

Fait par programme (`src/corpus.rs`), toujours le même : un vocabulaire de 60 000 mots (le
français courant, accents compris, puis des mots faits de syllabes), tirés selon une loi de
Zipf. À l'échelle 1 :

- espace A : 10 000 grandes pages titrées « Projet … » (la page 0 a 10 000 blocs et
  « mastodonte » dans son titre, les autres 99 blocs), et 90 000 petites pages d'un bloc
  titrées « Note … » ;
- espace B, jamais permis : 1 000 pages de 100 blocs, dont chaque bloc porte « canalcent » ;
- « canalzero » : un bloc de chacune des pages 1 à 10 ; « canalcent » : un autre bloc de ces
  mêmes pages.

Soit 1 290 901 entrées (une par bloc, une par titre), 1 189 901 blocs, 262 Mo de texte.

## Refaire

Une commande lourde à la fois. `D` est un dossier jetable.

```sh
cargo build --release
B=target/release/search-bench.exe

$B fts-check                       # FTS5 est-il dans le SQLite de sqlx ? (par une requête)
$B tokens                          # questions (b) et (c)
$B r916 $D/r916                    # la note sans statistique d'ensemble, et son témoin
$B payload $D/payload --trials 25  # la charge utile d'une validation, arrêts brutaux
$B build $D/full                   # l'index du plan ; --budget-mb 50, --no-starts,
                                   # --no-title-on-blocks, --scale 0.02 pour un essai
$B open $D/full                    # ouverture, dans un processus neuf
$B rest $D/full                    # mémoire au repos (--no-writer : sans écrivain)
$B query $D/full                   # latences, R902, R920, canal par le temps
$B commits $D/full                 # durée d'une validation (modifie l'index : en dernier)
$B hostile $D/hostile --blocks 1000

$B fts-build $D/fts                # blocks_index du plan (taille), puis table + FTS5
$B fts-query $D/fts
$B fts-write $D/fts                # durée d'une transaction d'index dans le tour d'écriture
$B fts-r916 $D/fts-r916
$B fts-hostile $D/fts-hostile --blocks 1000
```

Les mesures se font en release, une à la fois, et se répètent : les latences d'une même
commande varient de quelques pour cent d'un passage à l'autre sur un poste au repos, davantage
si autre chose compile.
