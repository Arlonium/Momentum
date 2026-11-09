# Mementum — résultats de la phase 0

Mesures du spike. PC = poste Windows de Lenny (i9-14900K), ARM = Raspberry Pi 4 (4 Go) ou VPS ARM64 2 vCPU / 4 Go.

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
