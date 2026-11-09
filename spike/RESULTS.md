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
