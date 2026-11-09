# Mementum — résultats de la phase 0

Mesures du spike. PC = poste Windows de Lenny (i9-14900K), ARM = Raspberry Pi 4 (4 Go) ou VPS ARM64 2 vCPU / 4 Go.

## Verdict (9 novembre 2026)

Les quatre questions de la phase 0 ont une réponse chiffrée. Toutes les mesures ont été faites **sur le PC**. **Les mesures ARM restent à faire** (commandes en fin de fichier) ; les verdicts de budget sont donc provisoires.

Ce tableau intègre les corrections de la **relecture finale** (même jour, section suivante). Deux conclusions de la première version affirmaient plus que leurs tests ne prouvaient : les positions UTF-16 de l'agent (un vrai bogue, corrigé) et la fidélité MMD ⇄ Yjs (vraie seulement pour ce que le modèle connaît).

| Question P0 | Réponse | Choix de la spec (§5.2) |
|---|---|---|
| 1. BlockNote + Yjs ⇄ serveur Rust yrs ⇄ SQLite, en temps réel | **Oui** : frappe visible chez l'autre en 8–11 ms (p95), reconnexion hors ligne qui fusionne, redémarrage du serveur sans perte **quand aucun client n'écrit pendant l'arrêt** (le cas « tué en pleine édition » n'est pas testé, et la compaction du spike n'est pas sûre en concurrence). Une perte de données sur page vide a été trouvée et corrigée (le serveur crée la racine de la page), **pour les éditions faites après la première synchro**. | **Confirmé**, avec 2 précisions : chemin Yjs 13 de BlockNote (`@blocknote/core/yjs`) ; yrs compilé avec la feature `sync` |
| 2. Aller-retour MMD sans perte sur 20 types de blocs | **Oui pour le sous-ensemble modélisé** : fixture relue octet pour octet, 12 800+ cas proptest MMD ⇄ arbre sans échec ; une page écrite par le serveur s'ouvre dans BlockNote identique, et la page écrite par BlockNote se relit en MMD exact. **Non en général** : ce que le modèle Rust ne connaît pas se perd sans erreur (enfants des blocs non conteneurs, props BlockNote hors défaut, saut de ligne, bloc d'une version plus récente) | **Confirmé** pour le format ; **corrigé** : le schéma P1 doit tout modéliser ou tout transporter (voir « Ce qui change pour la P1 ») |
| 3. Un agent qui édite pendant qu'un humain tape | **Oui** : 20 appels pendant 200 frappes, 0 perte, 0 conflit, visible en 13–16 ms (p95, depuis l'envoi de l'appel) ; conflit propre (409) si même bloc ; positions UTF-16 exactes (« Café 🚀 prêt »), **y compris après du texte formaté ou une mention depuis la relecture** (le spike corrompait ce cas en répondant 200) | **Confirmé** (révision = instantané Yjs, appel atomique) |
| 4. Base de 100 000 lignes JSON + index sur ARM | **Sur PC oui** pour les budgets (fenêtre 2,5 ms, filtre + tri 127 ms, groupes 13 ms) ; **rollups à la volée trop lents** (1,9 s en JSON, 0,6 s en table typée) | **Confirmé sur PC** pour le stockage JSON, la table typée restant le repli jusqu'à la mesure ARM ; **corrigé** : rollups matérialisés (mesuré), formules aussi (par prudence, non mesuré). **ARM à mesurer** |
| Benchmark Yjs / Loro | **On garde Yjs**, avec une règle de décision amendée (voir notes T7) : Loro est plus de 20 fois meilleur au chargement paresseux (W3, W4), mais 3 à 16 fois plus lent dès qu'on lit toute la page. **Un déplacement concurrent avec Yjs duplique le bloc et perd les éditions faites en même temps sur ce bloc** | **Confirmé**, plus une action P1 : déplacements de blocs faits par le serveur (une déduplication seule ne récupère pas les éditions perdues) |

**Ce qui change pour la P1 :**
1. **Pages géantes.** Une page de 10 000 blocs s'ouvre en ~1,3 s, budget non tenu. Le coût est la **liaison avec l'éditeur** (y-prosemirror construit et affiche les 10 000 nœuds pendant l'application de l'état), pas Yjs, qui charge l'instantané en 34 ms (T7, W3). Le cache local ne sert pas pour une page jamais vue. Il faut virtualiser l'affichage ou découper les pages géantes, ou revoir le budget pour elles.
2. **Rollups matérialisés** et invalidés par dépendances : obligatoire. Formules aussi, à confirmer par une mesure en P2.
3. **Déplacements de blocs.** Un déplacement concurrent duplique le bloc **et** perd les éditions faites sur l'original. Les déplacements doivent passer par le serveur ; la passe de déduplication ne suffit pas.
4. **Fidélité du schéma** (`mementum-schema`). Il faut :
   - modéliser toutes les props BlockNote, ou porter un sac de props opaque ;
   - une syntaxe MMD d'enfants pour **tout** bloc ;
   - un `<br/>` pour le saut de ligne (`hardBreak`) ;
   - transporter tel quel un nœud inconnu (attributs et contenu en JSON dans `data`) ;
   - un **versionnage de schéma**, car y-prosemirror **supprime du document partagé** tout nœud ou marque qu'il ne sait pas construire. Un client trop ancien doit passer en lecture seule, sinon il efface chez tout le monde les blocs d'une version plus récente.
5. **Positions des agents** : toujours compter sur le texte brut (`diff()`), jamais sur `XmlTextRef::get_string()`, qui écrit les marques en balises.
6. **Racine de page** : la racine écrite par le serveur ne protège que les éditions faites après la première synchro. Avec le rendu local d'abord (§5.9), il faut une racine déterministe ou une édition bloquée jusqu'à la synchro, avec un e2e « taper avant la synchro ».
7. **Écritures d'une page** : un seul acteur par page, qui enchaîne application, écriture et compaction ; jamais deux salles pour une même page ; test d'un serveur tué en pleine édition.
8. **Agent** :
   - régénérer les ids des blocs insérés ;
   - calculer révision et instantané dans la même section critique que l'écriture ;
   - répondre 422 à un MMD non canonique au lieu de le déformer.
9. **Yjs 14** : risque à suivre. BlockNote est passé à Yjs 14 RC par défaut, et yrs 0.28 ne lit pas le texte des nœuds Yjs 14.
10. **Interblocage yrs** : toujours obtenir les types Yjs (`get_or_insert_*`) avant d'ouvrir une transaction. Avec la feature `sync`, l'oubli provoque un interblocage.

## Relecture finale (9 novembre 2026)

La relecture de toute la branche, prévue par la méthode, avait été oubliée à la fin de la Task 8. Elle a été faite après coup, sur un clone neuf :
- un relecteur indépendant a lu le code, les tests et les dépendances ;
- toutes les suites ont été relancées ;
- les mesures du PC ont été recoupées.

Les décisions sont consignées dans le registre (`docs/superpowers/logs/2026-11-09-phase0-ledger.md`, section « Relecture finale »).

**Corrigé dans le spike :**
- **Positions de l'agent.** `str_replace` comptait sur `get_string()`, qui écrit les marques en balises. Résultat : « Du **gras** puis 🚀 déjà » devenait « 🚀prêtà » avec une réponse 200, et une cible placée après une mention était refusée (422). Le comptage se fait désormais sur le texte brut ; tests `unicode_safe_after_marks` et `replace_after_marks_and_mention`.
- **Fins de ligne.** Sur un clone Windows neuf (`core.autocrlf=true`), la fixture MMD était extraite en CRLF et `fixture_is_canonical` échouait. Un `.gitattributes` impose désormais LF.
- **Latence agent → navigateur.** Elle était chronométrée après la réponse HTTP, alors que le serveur diffuse avant de répondre. Elle se mesure maintenant depuis l'envoi de l'appel : p95 13–16 ms au lieu de 7–15 ms. Le budget reste très largement tenu.
- **Fixtures réécrites par les tests.** La suite e2e réécrivait les fixtures commitées avec des ids neufs à chaque passage. L'export ne se fait plus que sur demande (`EXPORT_FIXTURES=1`).
- **Faux vert sur la page de 10 000 blocs.** Le test, marqué « échec attendu », passait sans rien mesurer si la page `bench-10k` n'existait pas. Il est désormais ignoré dans ce cas, avec la consigne de lancer `measure`.

**Corrigé dans les conclusions** (tableau ci-dessus, conception §5.2, §5.5, §6.2, §6.3 et §11) :
- fidélité MMD limitée au sous-ensemble modélisé ;
- diagnostic des pages géantes ;
- perte d'éditions lors des déplacements concurrents ;
- règle de décision CRDT ;
- table typée gardée en repli ;
- redémarrage et racine de page nuancés.

**Recoupement sur clone neuf (PC)** :

| Mesure | RESULTS.md | Clone neuf |
|---|---|---|
| Chargement serveur de la page de 10 000 blocs (p95) | 31,2 ms | 33 ms |
| Démarrage à froid | 72 ms | 60–64 ms (416 ms au tout premier lancement, analyse antivirus d'un exécutable neuf) |
| Mémoire au repos | 8,6 Mo | 9,7 Mo |
| Frappe → distant (p95) | 8–11 ms | 9–10 ms |
| Ouverture de la page de 10 000 blocs | 1 270–1 346 ms | 1 273–1 365 ms |
| Base de 100 000 lignes : fenêtre / filtre + tri / groupes (p95) | 2,5 / 127–130 / 12–13 ms | 2,7 / 133 / 13 ms |

**Reste ouvert :**
- **Q4 sur ARM** : la phase 0 n'est close que pour les questions 1 à 3.
- **Erreur « RangeError » de y-prosemirror** : elle peut faire sauter la mise à jour de la vue après une fusion distante, et la frappe locale suivante peut alors effacer le changement distant. Risque à confirmer en P1, et à surveiller dans les e2e.

## Mesures détaillées

| Mesure | Budget | PC | ARM | Verdict |
|---|---|---|---|---|
| T1 · Chargement serveur d'une page de 10 000 blocs depuis SQLite (p95, 20 essais) | part serveur du budget « page jamais vue < 300 ms » (le budget complet, mesuré en T2, n'est pas tenu) | 31,2 ms (p50 28,6) | à faire | ✅ provisoire (part serveur) |
| T1 · Démarrage à froid du binaire jusqu'à `/health` | < 1 s | 72 ms | à faire | ✅ provisoire |
| T1 · Mémoire du serveur au repos (RSS) | < 150 Mo | 8,6 Mo | à faire | ✅ provisoire |
| T1 · Propagation A → B via le serveur (test `two_clients_converge`) | < 150 ms | < 150 ms (assertion) | — | ✅ |
| T2 · Frappe → visible chez l'autre (navigateurs, p95 sur 49 frappes, 3 passes) | < 150 ms | 8–11 ms (p50 5–6) | — | ✅ |
| T2 · Ouverture d'une page de 10 000 blocs dans BlockNote (connexion → premier bloc) | page jamais vue < 300 ms | 1 270–1 346 ms (dont synchro 920–948 ms) | à faire | ❌ corrigé : voir notes |
| T3 · Aller-retour MMD ⇄ arbre sur 20 types de blocs (fixture canonique + proptest) | sans perte | fixture relue octet pour octet ; 512 cas + 3 × 4 096 cas sans échec | — | ✅ |
| T5 · Agent qui édite pendant qu'un humain tape (20 appels `str_replace` + `insert`, 200 caractères tapés) | aucune perte, aucun conflit | texte humain complet et ordonné, 20 éditions présentes, 0 conflit, même nombre de blocs navigateur/serveur | — | ✅ |
| T5 · Appel de l'agent → visible dans le navigateur (p95, 3 passes ; chronomètre lancé à l'envoi depuis la relecture) | < 150 ms | 13–16 ms (p50 7–8) ; première version : 7–15 ms, chronométrée à tort après la réponse | — | ✅ |
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
- Ouverture de 10 000 blocs : la « synchro » (~75 % du temps) n'est pas Yjs. y-websocket ne passe à `synced` qu'après `applyUpdate`, et cet appel exécute de façon synchrone l'observateur y-prosemirror, qui construit et affiche les 10 000 nœuds ProseMirror. Yjs seul charge l'instantané en 34 ms (T7, W3). *Diagnostic corrigé à la relecture : la première version l'attribuait à Yjs.* Pistes pour la P1 : virtualiser l'affichage ou découper les pages géantes. Le cache local (SQLite WASM, §5.9) n'aide que pour une page déjà vue, et y-prosemirror lie tout le fragment d'un coup : il n'existe pas de chargement paresseux prêt à l'emploi.
- y-prosemirror 1.x journalise parfois « Caught error while handling a Yjs update RangeError » (restauration de la sélection après une fusion distante). L'exception est levée dans l'observateur : si `restoreRelativeSelection` échoue, `dispatch(tr)` est sauté et la vue ProseMirror reste en retard. À la frappe locale suivante, y-prosemirror réécrit Yjs d'après cette vue périmée, ce qui peut effacer le changement distant. La convergence tient, mais pas forcément les données. *Risque relevé à la relecture, à confirmer en P1* (première version : « sans effet »).
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
- **Le stockage JSON tient sur PC.** La table typée gagne environ 20 % sur les filtres, mais 2 fois sur les groupes (12,1 → 5,9 ms) et 3 fois sur les rollups calculés à la volée (1 856 → 600 ms). Elle reste le repli si l'ARM échoue (règle du plan). Le choix d'index et de requête compte bien plus. Exemple : `GROUP BY CAST(json_extract…)` ignorait l'index et prenait 707 ms ; avec l'expression exacte de l'index, 12 ms.
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

- **Verdict CRDT : on garde Yjs, avec une règle amendée à la relecture.** Prise à la lettre, la règle du plan (« Loro au moins 2 fois meilleur sur W3, W4 ou W5 ») était remplie : W3 donne 18 contre 0,8 ms, W4 20,5 contre 1,1 Mo. Loro ne gagne pourtant que grâce au chargement paresseux. Dès qu'on lit toute la page (W3b), il est 3 fois plus lent côté serveur et 16 fois plus lent côté client (appels JS ⇄ WASM). Or un éditeur lit toute la page. Quant à W5, il compare une fusion fausse (Yjs, 25 doublons) à une fusion correcte : ce n'est pas un argument de vitesse. Règle amendée : « Loro au moins 2 fois meilleur sur une charge où l'éditeur lit toute la page ». Elle n'est pas remplie, donc on garde Yjs. Non mesurés : la mémoire après lecture complète (W4b), qui compte sur un Pi avec beaucoup de salles ouvertes, et la liaison loro-prosemirror. La façade `mementum-crdt` garde Loro comme plan B.
- **Point faible de Yjs confirmé** (`yrs_concurrent_moves_duplicate_blocks`) : un déplacement de bloc vaut suppression + réinsertion, donc deux personnes qui déplacent le même bloc en même temps le dupliquent (25 sur 25 ici). Loro, avec son arbre déplaçable, n'a aucun doublon. Pire, relevé à la relecture : la copie part de l'état local, donc **les éditions faites en même temps sur l'original par l'autre personne sont perdues** (même cas qu'un agent qui édite un bloc pendant qu'un humain le glisse ailleurs : 200, puis l'édition disparaît). Garder une seule copie ne les récupère pas. **Action P1** : déplacements de blocs faits par le serveur (opération dédiée, appliquée sur l'état fusionné), passe de déduplication en filet de sécurité, et un test qui compte les éditions conservées sur les blocs déplacés. Les déplacements de pages dans l'arbre sont déjà faits par le serveur (§5.4).
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
