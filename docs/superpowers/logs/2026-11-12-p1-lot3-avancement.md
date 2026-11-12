# Lot 3, partie A : où en est l'exécution

Ce document est fait pour reprendre l'exécution du plan là où elle s'est arrêtée, sans rien savoir de la session qui l'a menée. Il complète le registre du lot, qui garde les décisions ; lui garde l'avancement, ce qui était en cours, et la liste des reports à trier à la relecture finale.

- Plan : [`docs/superpowers/plans/2026-11-12-p1-lot3-collaboration-blocs-riches.md`](../plans/2026-11-12-p1-lot3-collaboration-blocs-riches.md) (14 tâches).
- Registre des décisions (Rulings R129 à R197) : [`2026-11-12-p1-lot3-ledger.md`](2026-11-12-p1-lot3-ledger.md), une section par tâche et par tour de correction.
- État écrit le 2026-11-12 en fin de journée, juste avant de pousser `main` à la demande de Lenny. **Mettre ce fichier à jour à chaque tâche close, et le supprimer à la clôture de la partie A** (le registre et `docs/REPRISE.md` suffiront alors).

## Comment l'exécution est menée (R140)

Une session « contrôleur » exécute le plan tâche par tâche, directement sur `main`, sans branche ni worktree par tâche :

1. un sous-agent implémente la tâche en TDD (test vu rouge d'abord), commite par petits commits, écrit ses écarts au registre (`Ruling RNN: … — … — …`, numéro qui suit le dernier du fichier) ;
2. un autre sous-agent relit la tâche (conformité au plan, puis qualité), **en prouvant ses constats par des tests jetables**, et en relisant toute règle de protection comme une attaque ;
3. chaque constat « Important » repart chez l'implémenteur (tour de correction), puis une re-relecture ciblée vérifie le tour ; cinq tours au plus ;
4. les constats « Mineurs » ne bloquent pas : ils sont listés plus bas, pour la relecture finale.

Les dossiers de travail du contrôleur (briefs, rapports, paquets de relecture) sont sous `.superpowers/sdd/`, ignoré par git : ils ne sont pas dans le dépôt. Tout ce qu'il faut pour reprendre est ici, au registre et dans l'historique git.

Règles tenues pendant toute l'exécution, à tenir encore : commits au nom du dépôt (`LnYkLl`), sans aucune ligne d'attribution d'outil ; rien n'est poussé sans une demande explicite ; aucun test sur les ports 5175 et 8090 ni sur `data/instance` ; aucun budget relâché ; **la migration `0003` a changé plusieurs fois (R139, R191, R197) : aucun binaire de ce lot n'ouvre les données d'une instance que quelqu'un regarde avant la clôture** (une base ouverte par un binaire intermédiaire refuse de démarrer après un changement de `0003`).

## État des tâches

| Tâche | État | Tours de correction | Rulings |
|---|---|---|---|
| 1. Trou d'horloge retenu (R131) | **close** | 0 | R145, R146 |
| 2. Tableaux mal formés lus en grille (R54) | **close** | 1 | R147 à R149 |
| 3. Migration `0003`, version de l'espace (`lsn`, `WriteTx`) | **close** | 1 | R150, R151 |
| 4. Canal de l'espace, `GET /events/{spaceId}` | **close** | 1 | R152, R153 |
| 5. Barre latérale en direct (client) | **close** | 2 | R154 à R157 |
| 6. Trame de page, présence vérifiée (serveur) | **close** | 1 | R158 à R162 |
| 7. Avatars des présents (client) | **close** | 1 | R163 (retiré) à R167 |
| 8. Commentaires (serveur) | **close** | 1 | R168 à R173 |
| 9. Commentaires (client) | **close** | 3 | R174 à R178, R187 |
| 10. Historique (serveur) | **close** | 3 | R179 à R186, R188 à R191, R197 |
| 11. Historique (client) | **close** | 2 | R192 à R196 |
| 12. Tests de la limite R39 (bloc déplacé) | **tour de correction 1 en cours** | — | aucun |
| 13. Édition gardée pendant un déplacement (R39, R138) | **essai borné en cours ; aucune ligne de production** | — | voir plus bas |
| 14. Documentation et recette de la partie A | à faire | — | — |

Dernières mesures (poste de Lenny, 2026-11-12, à la tête poussée) : `cargo fmt` et `cargo clippy -D warnings` propres ; 681 tests Rust verts (15 ignorés : les mesures en release et les tests de la limite R39) ; 436 tests du client verts et 12 marqués `it.fails` (les scénarios du bloc déplacé) ; typecheck et licences npm propres ; la dernière suite complète de bout en bout date de la Task 11 : 148 parcours verts et 2 marqués `fixme` (ceux de la Task 12) ; budgets du navigateur relancés à la Task 11 : page de 1 000 blocs à 142 ms au p95 (budget 300), page de 10 000 blocs en 1,2 s (budget 3 s) ; côté serveur, la frappe qui déclenche la coupe d'une version reste vue par un autre en 39 ms au p95 (budget 150).

## Ce qui était en cours au moment de pousser

Deux sous-agents travaillaient encore. **Leur travail n'est pas dans ce qui est poussé** (sauf mention) ; s'il n'a pas été repris depuis, il est à refaire à partir de ce qui suit.

### Task 12, tour de correction 1

Les tests commités (`79feb6c` : `apps/web/e2e/block-move.spec.ts`, deux e2e `test.fixme` ; `crates/mementum-sync/tests/moves.rs`, deux tests `#[ignore]`) sont justes, mais la relecture a montré que la preuve ne suffit pas, et que le registre concluait trop large. Demandé à l'implémenteur :

1. corriger, au registre (section « Task 12 ») et en tête de l'e2e, ce qui généralise au-delà du prouvé ;
2. une suite Vitest commitée, `apps/web/src/editor/blockMoves.test.ts` : deux éditeurs BlockNote sur deux `Y.Doc` (`withCollaboration`), un test par geste, **chacun avec l'attente du comportement voulu**, vu rouge puis marqué (`it.fails` quand l'échec est certain), plus un témoin vert par geste ;
3. deux tests de l'acteur de plus dans `moves.rs`, bâtis avec **des octets produits par y-prosemirror** (fixtures sous `crates/mementum-sync/tests/fixtures/moves/`), pas à la main avec yrs ;
4. la note « Déplacements concurrents » de `CLAUDE.md`, mise d'accord avec ce qui est mesuré.

Au moment de pousser, les points 2 et 3 étaient commités (`95cc73d` : la suite Vitest, douze scénarios marqués `it.fails` ; `2f8b27c` : les tests de l'acteur à la forme du navigateur et leurs fixtures, ignorés) ; les points 1 et 4 (registre, en-tête de l'e2e, note de `CLAUDE.md`) n'étaient pas encore faits, et **ce tour n'a pas été relu**. À reprendre : vérifier que le registre et `CLAUDE.md` disent ce qui est mesuré (le tableau ci-dessous), puis faire relire le tour (les tests prouvent-ils chaque ligne du tableau ; un témoin vert par geste).

### Ce que la relecture de la Task 12 a mesuré (à tenir pour acquis)

y-prosemirror 1.3.7 ne regarde que l'état final du document : après chaque transaction, il passe tout le document à `updateYFragment` (`sync-plugin.js`), qui saute le préfixe et le suffixe identiques puis traite les enfants du milieu **par position**. Deux nœuds de même nom face à face : l'élément Yjs est récrit en place (attributs, dont `id`, et texte par diff). Noms différents : l'élément est supprimé et recréé.

| Geste | Ce qu'il écrit dans Yjs | Une frappe concurrente |
|---|---|---|
| Glisser un bloc au même niveau, blocs de même type, sans enfants | aucun élément créé ni supprimé ; chaque conteneur de la plage **change d'`id`** et son texte est récrit par diff | dans **n'importe quel bloc de la plage** (déplacé ou non), elle atterrit **dans le bloc voisin** (« Bloc  de TomY »), chez tous et sur le serveur |
| Idem, avec un bloc d'un autre type dans la plage (un titre) | l'élément de contenu est supprimé et recréé | **perdue** |
| Bloc avec enfants | le groupe des enfants est supprimé et recréé ailleurs avec les mêmes `id` | dans un enfant : **perdue** |
| Tab, Maj+Tab | nouveau conteneur au même `id`, original supprimé (la forme de `move_block`) | **perdue** |
| Déplacement au clavier (Mod+Maj+↑/↓) | récriture en place, comme le glisser | chez le voisin |
| **Deux déplacements concurrents, sans aucune frappe** | — | la page s'abîme : « Bloc YY », un bloc qui disparaît dans le texte d'un autre, un bloc en double avec le même `id` ; le dédoublonnage du serveur ne le voit pas (il ne parcourt la page que si la mise à jour insère des éléments, `crates/mementum-crdt/src/guard.rs`) |

La note « Déplacements concurrents » de `CLAUDE.md` (« un déplacement vaut copie + suppression ») est donc fausse pour un glisser au même niveau ; elle reste vraie pour Tab, Maj+Tab et les enfants d'un bloc glissé.

### Task 13 : pourquoi rien n'est écrit, et l'essai en cours

Le plan de la Task 13 (R138) repose sur « un déplacement vaut copie + suppression » : le serveur reconnaît un bloc inséré avec l'`id` d'un bloc vivant dont l'original est supprimé, et reporte sur la copie ce que le jeu de suppressions ne couvre pas. Les mesures ci-dessus montrent que c'est faux pour la plupart des gestes : un glisser n'insère aucun bloc ; l'auteur du déplacement ne supprime que le milieu qui diffère, pas « tout ce qu'il connaissait » ; il n'y a pas de copie où reporter ; tous les blocs de la plage sont touchés, et deux glissers suffisent à abîmer la page sans aucune frappe.

Décision du contrôleur : **aucune ligne de production de la Task 13 avant un verdict**. L'essai borné que le plan prévoyait à l'étape 1 de la tâche, avec sa clause de sortie, a été élargi et confié à un sous-agent, dans un atelier jetable (`.claude/worktrees/`, ignoré par git). Ses questions :

1. changer la forme à la source : faire comparer les enfants **par `id` de bloc** (une différence à clés) au lieu de par position dans `updateYFragment`, par un correctif de la dépendance (`pnpm patch`) ou autrement, pour qu'un réordonnancement devienne « le seul bloc déplacé est supprimé et réinséré » ; ce que cela règle geste par geste, ce que cela casse (annulation, curseurs, coût par frappe sur 10 000 blocs, collage, colonnes, tableaux), et si deux onglets dont un seul a le correctif convergent ;
2. une fois la forme ramenée à « copie + suppression du bloc déplacé », le report côté serveur de R138 tient-il (les trois lectures de yrs 0.28.1 que le plan demandait ; ce que devient un descendant non vu d'un élément supprimé) ;
3. le serveur seul peut-il reconnaître une récriture en place et savoir ce que l'auteur avait vu ;
4. la piste (a) de R39 (les déplacements demandés au serveur) ;
5. deux déplacements concurrents : ce qu'on veut, et si le dédoublonnage existant suffit ;
6. ce qui reste hors d'atteinte.

Il doit rendre l'un de trois verdicts : faisable maintenant (dessin découpé en tâches), faisable mais pas dans ce lot (avec le plus petit pas sûr), ou clause de sortie (la limite reste, décrite exactement). **Si ce verdict n'est pas au registre quand tu reprends, l'essai est à refaire** : commence par là, ou applique la clause de sortie du plan (les tests de la Task 12 restent marqués, la limite R39 est écrite telle que mesurée dans le README, la conception §11 et `CLAUDE.md`).

## Ce qui reste, dans l'ordre

1. **Finir la Task 12** (preuve élargie, voir plus haut), la faire relire.
2. **Trancher la Task 13** d'après le verdict de l'essai ; si elle est faite, elle demande son propre découpage, écrit et relu avant exécution.
3. **Task 14** : documentation relue contre le code final, et checklist de recette `docs/recette/2026-11-12-p1-lot3a-recette.md`. Y porter : la phrase « les deux coupes » de `CLAUDE.md` et de R190 (voir les reports de la Task 10), et la note « Déplacements concurrents ».
4. **Relecture finale de tout ce qui a changé depuis `6b6f104`**, en trois passes (sécurité ; intégration client et serveur ; documentation contre le code), chaque constat prouvé par un test jetable ; une vague de correction ; des re-relectures ciblées. Lui donner la liste des reports ci-dessous.
5. **Toutes les suites sur un clone neuf**, comme la CI (commandes dans `docs/REPRISE.md`), plus les budgets en release et ceux du navigateur.
6. **Essai de migration sur une copie d'une base du lot 2** (jamais sur `data/instance` elle-même).
7. Registre : mesures de clôture, reports, ce qui attend le propriétaire ; `docs/REPRISE.md` et le README à jour ; supprimer ce fichier.
8. **Le plan de la partie B** (« Blocs riches et organisation » : fichiers envoyés, blocs riches utilisables depuis l'éditeur, mentions et `[[`, duplication, favoris, récents, fil d'Ariane, reports mineurs du lot 2). Le menu « … » de la barre du haut (`apps/web/src/page/PageMenu.tsx`) est prêt à recevoir ses entrées.

## Ce qui attend le propriétaire

- **L'historique se lit dès « Peut lire »** (plan, Task 10). Quelqu'un à qui on partage une page aujourd'hui peut lire, dans une version d'avant, un texte effacé avant le partage. La conception ne dit pas le niveau.
- **Restaurer une version remplace tout le contenu**, comme `PUT /mmd` : une frappe faite au même instant par quelqu'un d'autre est perdue (R188). L'écran le dit quand d'autres ont la page ouverte. L'écriture par différence du lot 5 (R40) le réglera pour les deux.
- **Récrire ou restaurer une page de 10 000 blocs occupe son acteur 5 secondes** (`write_blocks` sur un document déjà réécrit ; mesuré à la Task 10, antérieur à ce lot). Aucun budget ne porte dessus.
- **Le déplacement de blocs à plusieurs** (Task 13) : le défaut est plus large que ce que le plan croyait, et sa correction touche au chemin de chaque frappe. Voir plus haut.
- **Un sous-agent s'est vu refuser l'écriture de son fichier de rapport** par l'outil, en cours de route ; ses comptes rendus sont passés par message. Rien n'a été perdu.
- Les choix R132 à R138 du plan ont été validés avec lui le 2026-11-12 ; ceux pris pendant l'exécution (R140 à R197) sont au registre, chacun avec son coût s'il est faux.

## Reports mineurs, à trier à la relecture finale

Constats « Mineurs » des relectures de tâche, non corrigés. Ceux marqués ✔ ont été levés depuis par une tâche suivante.

**Task 1 (trou d'horloge)**
- Le réessai des files retenues coûte O(connexions × clients) : `has_reached` reconstruit le vecteur d'état par connexion et par tour (`actor.rs` `release_held`, `page.rs`) ; le calculer une fois par tour.
- ✔ Une mise à jour retenue déjà couverte par l'étape 2 était réécrite au journal et relayée (levé par R181 : une mise à jour qui ne change rien n'est ni écrite ni relayée).
- Le hub journalise « fermée à la demande » une fermeture après un refus au réessai.
- `PageDoc::leaves_gap` et `Shape::leaves_gap` ne servent plus qu'aux tests ; faire de `leaves_gap` un `awaits(u)?.is_some()`.
- Pas de test Rust de la libération croisée (A retient, B comble : attribution au compte de A, relais à B, pas de renvoi à A), ni de plusieurs retenues libérées dans l'ordre.
- Une mise à jour à trou hors schéma est retenue sans `Shape::check` ; le motif journalisé est trompeur.
- `CLAUDE.md` dit que le départ passe par `Conn::close` (il passe par `leave`).

**Task 2 (tableaux)**
- R148 : quand toutes les lignes portent une fusion et que quelqu'un ajoute une colonne pendant qu'un autre ajoute une ligne, la lecture perd les fusions là où `fixTables` bourre la ligne ; piste : la couverture partagée par le plus de lignes.
- Des soustractions nues, protégées par invariant (`table.rs`), alors que le registre dit « arithmétique saturée ».
- Le générateur du test de propriétés ne va que de 0 à 200 (négatifs, `i64::MIN`, ±2^32, flottants, chaînes : couverts par des essais non commités).
- L'arrêt anticipé de la borne du bourrage n'est pas gardé par un test.
- « À la première frappe » (conception §6.2) est sans doute inexact : `fixTables` tourne à tout changement du document.

**Task 3 (store)**
- Un titre envoyé à l'identique avance `lsn`.
- `DerefMut` prête la `Transaction` brute : un `UPDATE spaces SET lsn` à la main échapperait à l'annonce (convention documentée, non gardée par un test).
- `WriteTx` vit dans `spaces.rs` alors qu'elle sert à cinq modules.
- Trois copies des options de connexion dans les tests ; `tests/migration.rs` près de 1 000 lignes.
- `tokio` en dépendance normale du store hérite de fonctions dont seule `sync` sert.

**Task 4 (canal de l'espace)**
- L'ordre de R141 (`lsn` lu avant les pages) n'est gardé par aucun test.
- Ressources : pas de plafond de canaux par compte ; tampon de lecture de 128 Kio par canal (`read_buffer_size(4096)` suffirait) ; chaque annonce réveille tous les canaux de l'instance, trois lectures par canal et par version.
- Un message de plus de 1 Kio est journalisé « client parti ».
- `check()` construit une réponse HTTP pour lire un statut ; `CLOSE_GRACE` est dupliqué.
- Un client qui ne lit pas mais envoie des pongs non sollicités reste ouvert.
- « Au moins un pas par transaction » (`events.rs`, R133, conception) à aligner sur ce que fait `bump_lsn`.

**Task 5 (barre latérale)**
- **À trancher** : un canal à demi ouvert, sans bruit, laisse `live` vrai et le minuteur coupé ; le client ne voit pas les pings ; il n'y a pas d'équivalent de R107 pour `/events`. Piste : une trame répétée par le serveur et un délai côté client, ou rouvrir le canal au retour sur l'onglet et sur `online`.
- ✔ Les e2e de `sharing.spec.ts` et `auth.spec.ts` qui retiennent la page sans retenir l'arbre (levé à la Task 7).
- Couverture : aucune trame illisible testée ; le branchement d'`App.tsx` n'est vu par aucun test Vitest ; `AT_ONCE` de `live-tree.spec.ts` pourrait passer par le minuteur si la mise en place dépassait 8 s.
- Une action locale coûte une lecture de l'arbre et une relecture du compte de plus.
- Trois commentaires de `useSpaceTree.ts` disent encore que le minuteur ne tourne que canal tombé (il tourne aussi après une lecture échouée).
- Après un échec réseau suivi d'un `401` ou d'un `404`, canal ouvert, le minuteur relit toutes les dix secondes et rappelle `onSessionLost` ou `onGone` à chaque tour.
- Trois tests de `useLiveTree.test.ts` à rendre plus probants (voir le registre, Task 5, correction 2).

**Task 6 (présence, trame de page)**
- R161 : `ConnInfo` permet `actor: Some` avec `name: None` ; un champ unique `account` le rendrait impossible.
- Couverture : R88 et un `Kick` entre l'inscription et `Admit` ; une trame 100 envoyée par une connexion en lecture seule.
- `actor.rs` et `presence.rs` grossissent (`Holders` irait dans un module).
- **Passe sécurité** : la liste des présents donne à un invité l'UUIDv7 de chaque compte présent, donc sa date de création ; la trame `roster` coûte O(N²·M) pour N arrivées, et un lecteur peut la déclencher en boucle.
- Piste de fond : rattacher un `ClientID` au compte qui a écrit des mises à jour sous ce client (le journal attribue les écritures).
- Le test de la limite « après l'arrêt de l'acteur » n'a pas de témoin positif.

**Task 7 (avatars)**
- `RosterUser` est écrit à la main côté client alors que le serveur écrit la trame par `json!` : rien n'empêche les deux de diverger.
- `avatarInk.ts` recopie en dur la valeur de `--mm-text-strong` ; le contraste de `.mm-avatar-more` n'est ni calculé ni testé.
- Le commit `9c600a7` porte deux sujets.

**Task 8 (commentaires, serveur)**
- `Located::comment()` rend un `500` pour un cas impossible (deux fonctions le rendraient impossible par les types).
- `has_block` et un bloc rangé dans une colonne : non vérifié.
- Caractères de format Unicode admis dans le corps et l'emoji (traités à l'affichage par la Task 9).

**Task 9 (commentaires, client)**
- Deux relectures par écriture locale (la sienne, puis sa propre trame).
- Première lecture en `401` sans « Réessayer » quand la connexion de synchro reste ouverte.
- Le bouton de survol n'est pas montré sur un bloc qui a déjà un fil ouvert.
- Sur macOS, Cmd+Option+M est un raccourci système (non vérifié) ; sur un clavier allemand, Ctrl+Alt gauches + M avale « µ » quand le navigateur ne signale pas AltGr (coût écrit de R176).
- Un brouillon dont la personne perd le droit d'écrire reste en mémoire sans se montrer.
- Deux brouillons de modification dans le même fil : seul le premier se rouvre.
- Un refus du serveur après un remontage pendant l'envoi n'est affiché que dans l'ancienne zone.

**Task 10 (historique, serveur)**
- **À porter à la Task 14** : `CLAUDE.md` et R190 parlent de « deux coupes », alors que la copie de l'exception se répète à chaque restauration de la plus ancienne d'un historique de 100 : la date, la raison et les auteurs des versions ordinaires s'effacent une à une (le contenu n'est jamais perdu).
- `Hub::shutdown` dans `serve` n'a pas de test (extraire un `serve_until`).
- Une suppression en attente qui ne se résout jamais laisse `has_pending_deletions` vrai : les doublons sans effet reviennent pour cette page (sans danger).
- Restaurer ou récrire une grande page : 5 s d'acteur occupé (voir plus haut).

**Task 11 (historique, client)**
- Le piège de Tab et le clic sur le voile sont dupliqués entre `HistoryDialog` et `Confirm`.
- La mise en garde « d'autres personnes ont cette page ouverte » disparaît quand la session est coupée.
- Le coût de l'aperçu d'une grande version n'est pas mesuré (un second éditeur complet).
- `ui/Avatar.tsx` (menu du compte, membres) garde sa propre règle d'initiales.
- Budgets du navigateur à relancer à la clôture (le démarrage a grossi de quelques centaines d'octets).
