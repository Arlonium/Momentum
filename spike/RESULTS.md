# Mementum — résultats de la phase 0

Mesures du spike. PC = poste Windows de Lenny (i9-14900K), ARM = Raspberry Pi 4 (4 Go) ou VPS ARM64 2 vCPU / 4 Go.

## Verdict (9 novembre 2026)

Les quatre questions de la phase 0 ont une réponse chiffrée. Toutes les mesures ont été faites **sur le PC**. **Les mesures ARM restent à faire** (commandes en fin de fichier) ; les verdicts de budget sont donc provisoires.

| Question P0 | Réponse | Choix de la spec (§5.2) |
|---|---|---|
| 1. BlockNote + Yjs ⇄ serveur Rust yrs ⇄ SQLite, en temps réel | **Oui** : frappe visible chez l'autre en 8–11 ms (p95), redémarrage du serveur sans perte, reconnexion hors ligne qui fusionne. Une perte de données sur page vide a été trouvée et corrigée (le serveur crée la racine de la page). | **Confirmé**, avec 2 précisions : chemin Yjs 13 de BlockNote (`@blocknote/core/yjs`) ; yrs compilé avec la feature `sync` |
| 2. Aller-retour MMD sans perte sur 20 types de blocs | **Oui** : fixture relue octet pour octet, 12 800+ cas proptest sans échec ; une page écrite par le serveur s'ouvre dans BlockNote identique, et la page écrite par BlockNote se relit en MMD exact | **Confirmé** (règles de format à ajouter à la spec §6.2) |
| 3. Un agent qui édite pendant qu'un humain tape | **Oui** : 20 appels pendant 200 frappes, 0 perte, 0 conflit, visible en 7–15 ms ; conflit propre (409) si même bloc ; positions UTF-16 exactes (« Café 🚀 prêt ») | **Confirmé** (révision = instantané Yjs, appel atomique) |
| 4. Base de 100 000 lignes JSON + index sur ARM | **Sur PC oui** pour les budgets (fenêtre 2,5 ms, filtre + tri 127 ms, groupes 13 ms) ; **rollups à la volée trop lents** (1,9 s) quel que soit le stockage | **Confirmé** pour le stockage JSON ; **corrigé** : rollups et formules matérialisés (déjà prévu §5.5, désormais obligatoire). **ARM à mesurer** |
| Benchmark Yjs / Loro | **On garde Yjs** : Loro plus lent dès qu'on lit toute la page (×3 serveur, ×16 client) et à la fusion (×2). Mais **Yjs duplique un bloc déplacé en même temps par deux personnes** | **Confirmé**, plus une action P1 : nettoyage des doublons de blocs côté serveur |

**Ce qui change pour la P1 :**
1. Une page de 10 000 blocs s'ouvre en ~1,3 s, budget non tenu. Il faut le cache local (SQLite WASM, §5.9) **et** le chargement paresseux des blocs.
2. Rollups et formules matérialisés et invalidés par dépendances : obligatoire.
3. Passe serveur de déduplication des blocs après fusion (déplacements concurrents).
4. Risque à suivre : BlockNote est passé à Yjs 14 RC par défaut, et yrs 0.28 ne lit pas le texte des nœuds Yjs 14.
5. Toujours obtenir les types Yjs (`get_or_insert_*`) avant d'ouvrir une transaction : avec la feature `sync`, sinon interblocage.

## Mesures détaillées

| Mesure | Budget | PC | ARM | Verdict |
|---|---|---|---|---|
| T1 · Chargement serveur d'une page de 10 000 blocs depuis SQLite (p95, 20 essais) | page jamais vue < 300 ms | 31,2 ms (p50 28,6) | à faire | ✅ provisoire |
| T1 · Démarrage à froid du binaire jusqu'à `/health` | < 1 s | 72 ms | à faire | ✅ provisoire |
| T1 · Mémoire du serveur au repos (RSS) | < 150 Mo | 8,6 Mo | à faire | ✅ provisoire |
| T1 · Propagation A → B via le serveur (test `two_clients_converge`) | < 150 ms | < 150 ms (assertion) | — | ✅ |
| T2 · Frappe → visible chez l'autre (navigateurs, p95 sur 49 frappes, 3 passes) | < 150 ms | 8–11 ms (p50 5–6) | — | ✅ |
| T2 · Ouverture d'une page de 10 000 blocs dans BlockNote (connexion → premier bloc) | page jamais vue < 300 ms | 1 270–1 346 ms (dont synchro 920–948 ms) | à faire | ❌ corrigé : voir notes |
| T3 · Aller-retour MMD ⇄ arbre sur 20 types de blocs (fixture canonique + proptest) | sans perte | fixture relue octet pour octet ; 512 cas + 3 × 4 096 cas sans échec | — | ✅ |
| T5 · Agent qui édite pendant qu'un humain tape (20 appels `str_replace` + `insert`, 200 caractères tapés) | aucune perte, aucun conflit | texte humain complet et ordonné, 20 éditions présentes, 0 conflit, même nombre de blocs navigateur/serveur | — | ✅ |
| T5 · Réponse de l'agent → visible dans le navigateur (p95, 3 passes) | < 150 ms | 7–15 ms (p50 3–6) | — | ✅ |
| T5 · Agent et humain dans le même bloc | 409 propre, document intact | 409 + extrait à jour (test `conflict_when_block_changed`) | — | ✅ |
| T5 · Édition d'agent sur « Café 🚀 déjà » (positions UTF-16) | aucun caractère coupé | « Café 🚀 prêt » côté serveur et côté client Yjs | — | ✅ |
| T4 · MMD ⇄ Yjs au format BlockNote, côté serveur (yrs) | sans perte | la fixture écrite par BlockNote se relit en MMD canonique exact ; une page écrite par le serveur s'ouvre dans BlockNote identique (28 blocs) | — | ✅ |
| T6 · Base de 100 000 lignes : première fenêtre de 200 lignes triée (JSON + index) | < 500 ms | 2,0–2,5 ms | à faire | ✅ provisoire |
| T6 · Filtre ET/OU à 2 niveaux + tri (JSON + index) | < 300 ms | 127–130 ms | à faire (risque : ×5 à ×10 sur Pi) | ⚠️ provisoire |
| T6 · Vue groupée par statut (JSON + index) | < 300 ms | 12–13 ms | à faire | ✅ provisoire |
| T6 · Rollup calculé à la volée sur 100 000 lignes | (pas de budget) | 1,9 s en JSON, 0,6 s en table typée | à faire | ❌ corrigé : rollups matérialisés |
| T6 · Différentiel SQL ⇄ référence Rust (200 filtres aléatoires à 3 niveaux, 2 000 lignes) | identique | identique | — | ✅ |

Notes T1 :
- L'instantané d'une page de 10 000 blocs pèse ~2,1 Mo.
- Les poignées de main d'un document vide (mise à jour sans effet) ne sont ni stockées ni diffusées.
- yrs doit être compilé avec la feature `sync` pour partager `Awareness` entre tâches tokio.

Notes T2 :
- **BlockNote 0.57 est passé à Yjs 14 RC** (`@blocknote/core/y`, paquet `@y/y`) et garde un chemin Yjs 13 (`@blocknote/core/yjs` + y-prosemirror 1.x), utilisé par le spike. Essai de compatibilité : le format binaire v1 passe entre Yjs 13, Yjs 14 et yrs (structure, attributs), **mais yrs ne voit pas le texte d'un nœud Yjs 14**, rangé directement dans le nœud au lieu d'un `XmlText`. Risque à suivre : le jour où BlockNote abandonne le chemin Yjs 13, le serveur Rust devra comprendre le modèle Yjs 14 (yrs ne le fait pas en 0.28).
- **Perte de données trouvée puis corrigée** : deux clients qui écrivent sur une page vide (l'un hors ligne) créaient chacun la racine BlockNote ; à la fusion, ProseMirror en supprimait une avec son texte. Correction : le serveur écrit la racine (`blockGroup > blockContainer > paragraph`) à la création de la page et la persiste. Test `new_page_is_seeded_with_blocknote_root` + e2e `offline_edits_merge_on_reconnect` stables.
- Ouverture de 10 000 blocs : ~75 % du temps est la synchro initiale (instantané Yjs de 2,1 Mo appliqué en JS), le reste le rendu ProseMirror. Pistes pour la P1 : cache local (SQLite WASM, prévu §5.9), chargement paresseux des blocs hors écran, pages géantes découpées.
- y-prosemirror 1.x journalise parfois « Caught error while handling a Yjs update RangeError » (restauration de la sélection après une fusion distante) : rattrapé par la bibliothèque, sans effet sur la convergence.
- Types custom nécessaires : callout, equation, bookmark, pageLink, databaseRef, syncedBlock, toc, unknown, plus les contenus en ligne mention et math. Le H4 et le divider existent en natif dans BlockNote 0.57. Multi-colonnes via `@blocknote/xl-multi-column` (GPL-3.0).

Notes T6 (bench `cargo run --release -p spike-db -- bench --rows 100000`, PC, p95 sur 20 essais) :

| Opération | JSON + index d'expression | Table typée par source |
|---|---|---|
| Première fenêtre (200 lignes triées par échéance) | 2,0 ms | 2,0 ms |
| Filtre statut = En cours | 95,5 ms | 78,8 ms |
| Filtre ET/OU + tri par priorité | 130,3 ms | 98,3 ms |
| Groupes par statut | 12,1 ms | 5,9 ms |
| Rollup : somme des estimations par projet | 1 856 ms | 600 ms |
| Insertion d'une ligne | 0,2 ms | — |
| Modification d'une cellule | 0,2 ms | — |

- Jeu de données de 100 000 lignes construit en 3,3 s.
- **Le stockage JSON tient** : la table typée ne gagne qu'environ 20 % sur les filtres. Le choix d'index et de requête compte bien plus. Exemple : `GROUP BY CAST(json_extract…)` ignorait l'index et prenait 707 ms ; avec l'expression exacte de l'index, 12 ms.
- **Les rollups (et les formules) ne peuvent pas être calculés à la lecture** sur 100 000 lignes, quel que soit le stockage : il faut les matérialiser et les invalider par dépendances (§5.5 de la spec). C'est confirmé.
- Filtres sans tri indexé : ~100 ms sur le PC, donc probablement 0,5 à 1 s sur un Pi 4. Pistes : clé primaire entière (rowid) pour arrêter le parcours tôt, index couvrants sur les propriétés filtrées, fenêtre servie depuis le cache client. **À mesurer sur ARM avant de trancher.**

Notes T7 (page de 10 000 blocs, PC ; `cargo run --release -p spike-crdt-bench` et `node --expose-gc crdtbench-js/bench.mjs`) :

| Charge | yrs 0.28 (serveur) | Loro 1.17 (serveur) | yjs 13.6 (client) | loro-crdt 1.17 (client) |
|---|---|---|---|---|
| W1 construire 10 000 blocs | 439 ms | 76 ms | 57 ms | 519 ms |
| W2 instantané | 1 453 Ko | 1 033 Ko | 1 648 Ko | 1 017 Ko |
| W3 charger l'instantané | 18 ms | 0,8 ms (paresseux) | 34 ms | 2,8 ms (paresseux) |
| W3b charger puis lire tous les blocs | 28 ms | 83 ms | 35 ms | 580 ms |
| W4 mémoire après chargement | 20,5 Mo | 1,1 Mo (paresseux) | 25 Mo de tas JS | 2,5 Mo de tas JS (+ mémoire WASM non comptée) |
| W5 fusion 2 × 1 000 éditions + 50 déplacements | 37 ms, **25 blocs en double** | 83 ms, 0 doublon | — | — |
| W6 état après 10 000 éditions | 1 613 Ko | 1 350 Ko | — | — |

- **Verdict CRDT : on garde Yjs** (règle du plan : Loro devait être au moins 2 fois meilleur sur W3, W4 ou W5). Loro ne gagne que grâce au chargement paresseux (W3, W4) ; dès qu'on lit toute la page, il est 3 fois plus lent côté serveur et 16 fois plus lent côté client (appels JS ⇄ WASM), et sa fusion est 2 fois plus lente. La liaison loro-prosemirror n'a donc pas eu besoin d'être évaluée.
- **Point faible de Yjs confirmé** (`yrs_concurrent_moves_duplicate_blocks`) : un déplacement de bloc vaut suppression + réinsertion, donc deux personnes qui déplacent le même bloc en même temps le dupliquent (25 sur 25 ici). Loro, avec son arbre déplaçable, n'a aucun doublon. **Action P1** : passe de nettoyage côté serveur après fusion (un même id de `blockContainer` en double → garder une seule copie), et déplacements de pages dans l'arbre faits par le serveur (déjà prévu en §5.4).
- Les benchmarks publics comparant ces bibliothèques étaient périmés ; ces chiffres les remplacent pour notre usage.

## Mesures ARM à faire (Raspberry Pi 4 ou Hetzner CAX11)

Depuis `spike/`, avec Rust installé sur la machine :

```bash
cargo run --release -p spike-server --bin measure
cargo test --release -p spike-db -- --ignored budgets_100k --nocapture
cargo run --release -p spike-db -- bench --rows 100000
cargo run --release -p spike-crdt-bench
```

Reporter les valeurs dans la colonne ARM ci-dessus. Le budget le plus à risque est « filtre ET/OU + tri < 300 ms » (127 ms sur PC).
