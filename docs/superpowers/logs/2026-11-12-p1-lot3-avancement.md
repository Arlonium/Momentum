# Lot 3, partie A : où en est l'exécution

Ce document est fait pour reprendre l'exécution du plan là où elle s'est arrêtée, sans rien savoir de la session qui l'a menée. Il complète le registre du lot, qui garde les décisions ; lui garde l'avancement, ce qui était en cours, et la liste des reports.

- Plan : [`docs/superpowers/plans/2026-11-12-p1-lot3-collaboration-blocs-riches.md`](../plans/2026-11-12-p1-lot3-collaboration-blocs-riches.md) (14 tâches ; la Task 13 est récrite à la fin du fichier, en 13a, 13b et 13c).
- Registre des décisions (Rulings R129 à R219) : [`2026-11-12-p1-lot3-ledger.md`](2026-11-12-p1-lot3-ledger.md), une section par tâche et par tour de correction.
- Relecture finale, constats et vague de correction : [`2026-11-12-p1-lot3-relecture-finale.md`](2026-11-12-p1-lot3-relecture-finale.md).
- Essai du bloc déplacé (Task 13) : [`2026-11-12-p1-lot3-essai-deplacement.md`](2026-11-12-p1-lot3-essai-deplacement.md).
- Rapport de l'implémenteur de la Task 13a (le code est sur une branche locale, pas sur `main`) : [`2026-11-13-p1-lot3-task13a-rapport.md`](2026-11-13-p1-lot3-task13a-rapport.md).
- Plan de la partie B, relu et corrigé le 2026-11-13, ouvert par sa première tâche cliente : [`docs/superpowers/plans/2026-11-12-p1-lot3b-blocs-riches-organisation.md`](../plans/2026-11-12-p1-lot3b-blocs-riches-organisation.md).

**État écrit le 2026-11-13 vers 3 h 40 du matin : Lenny a remis le travail en pause** (première pause le 2026-11-12 à 18 h, reprise le 2026-11-13 peu après minuit). Tous les agents sont arrêtés à un point sain. Ce qui est fini est sur `main`, poussé ; **deux travaux finis ou presque sont sur des branches locales, jamais poussées** (voir « État à la pause »). **Mettre ce fichier à jour à chaque tâche close, et le supprimer à la clôture de la partie A**, avec celui de la relecture finale (le registre et `docs/REPRISE.md` suffiront alors).

## Comment l'exécution est menée (R140)

Une session « contrôleur » exécute le plan tâche par tâche, directement sur `main`, sans branche ni worktree par tâche :

1. un sous-agent implémente la tâche en TDD (test vu rouge d'abord), commite par petits commits, écrit ses écarts au registre (`Ruling RNN: … — … — …`) ;
2. un autre sous-agent relit la tâche (conformité au plan, puis qualité), **en prouvant ses constats par des tests jetables**, et en relisant toute règle de protection comme une attaque ;
3. chaque constat « Important » repart chez l'implémenteur (tour de correction), puis une re-relecture ciblée vérifie le tour ; cinq tours au plus ;
4. les constats « Mineurs » ne bloquent pas : ils sont listés plus bas.

Les dossiers de travail du contrôleur (briefs, rapports, paquets de relecture) sont sous `.superpowers/sdd/`, ignoré par git : ils ne sont pas dans le dépôt. Tout ce qu'il faut pour reprendre est ici, dans le fichier de la relecture finale, au registre et dans l'historique git.

**Numéros des Rulings.** Des plages sont réservées, pour que des travaux menés de front ne se croisent pas. **Pris au registre** : jusqu'à R208 (piste S de la relecture finale et son tour de correction), R210 à R219 (piste C). **Pris hors du registre, à y porter** : R220 à R222 (Task 13, dans le plan), R223 à R229 (Task 13a, dans son rapport), R240 à R257 (plan de la partie B), R290 à R297 (Task C6 de la partie B, plus bas ; R291 et R294 retirés). **Libres** : R209, R258, R259, puis R350 à R359 pour la suite du tour de la piste S ; R230 à R239 pour la Task 13b ; R260 à R289 (piste S de la partie B), R298 à R329 (sa piste C), R330 à R349 (relais, documentation, contrôleur) ; une décision hors de tout cela prend R360 ou plus.

Règles tenues pendant toute l'exécution, à tenir encore : commits au nom du dépôt (`LnYkLl`), sans aucune ligne d'attribution d'outil ; `main` se pousse au fil du travail (accord de Lenny du 2026-11-12 pour le lot 3), après contrôle, jamais en force ; aucun test sur les ports 5175 et 8090 ni sur `data/instance` ; aucun budget relâché ; **la migration `0003` a changé plusieurs fois (R139, R191, R197) et changera encore (Task 13b : la table `block_moves` part ; partie B, R243) : aucun binaire de ce lot n'ouvre les données d'une instance que quelqu'un regarde avant la clôture** (une base ouverte par un binaire intermédiaire refuse de démarrer après un changement de `0003`).

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
| 12. Tests de la limite R39 (bloc déplacé) | **close** | 1 | aucun |
| 13. Édition gardée pendant un déplacement (R39, R138) | récrite en 13a, 13b, 13c. **13a : close, sur `main`** (relue deux fois) ; 13b et 13c : à faire | 1 (13a) | R220 à R239 |
| 14. Documentation et recette de la partie A | à faire | — | — |
| Relecture finale | passes sécurité et intégration rendues ; vague de correction : les 16 constats sont faits ; **piste C relue et close** (ses e2e restent à relire) ; **piste S relue : 1 critique corrigé, 3 importants et 5 mineurs à corriger** ; passe documentation à faire | 2 (piste C), 1 en cours (piste S) | R198 à R208, R210 à R219 |

Dernières mesures (poste de Lenny, 2026-11-13 entre 3 h 30 et 5 h, à la tête poussée, **sans clone neuf**) : `cargo fmt` et `cargo clippy -D warnings` propres ; `cargo test --workspace` : **723 verts**, 15 ignorés (les mesures en release et les tests de la limite R39) ; client : **516 verts et 12 marqués `test.fails`** (les scénarios du bloc déplacé), typecheck propre ; licences npm propres au dernier relevé (aucun paquet ajouté sur `main`) ; e2e : suite entière relancée sur `main` à la pause, correctif du défaut critique compris : **159 verts**, 2 `fixme` voulus (bloc déplacé), et 1 échec, `space_without_a_role_is_not_found` : une course du test lui-même, qui comparait l'écran pendant que la barre latérale disait encore « Chargement… » (reproduite à coup sûr en retardant la lecture de l'arbre) ; le test attend maintenant la fin du chargement, et passe dix fois de suite ; la suite entière n'a pas été relancée après cette correction d'un seul test ; budgets du navigateur, mesurés dans l'atelier de la part e2e sur un poste occupé : 1 000 blocs à 160 ms au p95 (budget 300), 10 000 blocs en 1,38 s (budget 3 s) ; côté serveur, en release : frappe vue par un autre à 2,0 à 2,3 ms au p95, 42,8 ms quand elle déclenche la coupe d'une version (budget 150).

## État à la pause du 2026-11-13 (3 h 40 du matin)

> **Reprise le 2026-11-13 à 5 h 10** (« relance le travail »). Quatre travaux sont repartis de front, chacun dans son arbre, dans l'ordre de « Ce qui reste » : (1) la fin du tour de correction de la piste S, dans l'arbre principal, avec une décision de plus sur le doute de C1 (à la relecture du stockage, une entrée du journal où un client paraît deux fois est écartée et journalisée, et la page se charge sans elle) ; (2) la fin de la relecture de la Task 13a, dans son atelier, P1 et P3 d'abord ; (3) la fin du tour de la Task C6 de la partie B, dans son atelier, puis son rebase sur `main` ; (4) la relecture des e2e de la piste C, dans un atelier neuf (`.claude/worktrees/relecture-e2e`, branche `relecture-e2e`, ports `5178` et `8085`). Ce qui suit décrit l'état **à la pause** ; il est mis à jour à chaque travail clos.
>
> **Depuis la reprise** (tenu à jour) : **piste S : le tour de correction 1 est fini et poussé** (les onze constats de la relecture sécurité traités : R207 à R209, R258, R259, R350 à R356 ; 744 tests Rust verts) ; sa re-relecture est rendue : neuf constats sur onze levés, **restent la fin des plafonds des commentaires (réactions, part de chaque compte) et le coût d'un `PUT /mmd` à contenu en ligne ou à tableau très large, plus deux mineurs neufs : tour de correction 2** : les plafonds et les deux mineurs sont faits et poussés (R357, R358, R365, R366 ; 752 tests Rust verts) ; **reste le coût de `PUT /mmd`, en cours** (un agent neuf, dans l'arbre principal). **Client : « Supprimer le fil »** (suite des plafonds) est **fait, relu et repris sur `main`** (neuf commits, de `03cece3` à `c072081` ; R360 à R364 et R370 au registre). **Partie B, piste serveur : sa Task S1 (reports du lot 2 : inscription et session dans une transaction, invitation sans effet non consommée, titre de la page d'une invitation en attente) est faite, relue, re-relue et reprise sur `main`** (neuf commits, de `f55ca82` à `7a0b3a7` ; R260 à R266 ; 764 tests Rust verts) ; **sa Task S2 (store : fichiers et favoris, migration `0004`) est en cours** dans l'atelier `.claude/worktrees/lot3b-serveur`. **Relais X1 (les mêmes reports, côté client) : en cours** dans l'atelier `.claude/worktrees/lot3b-x1` (ports `5178` et `8085`). **Task C6 de la partie B : finie, relue deux fois, reprise sur `main`** par avance rapide (dix-huit commits, de `391d9fe` à `c2ebc75` ; R290 à R298 au registre, pièges dans `CLAUDE.md`) ; sa branche et son atelier sont supprimés. **La partie B est ouverte au registre** (R240 à R257 recopiés). **E2e de la piste C : relus et corrigés, sur `main`** (les quinze rougissent quand on défait le correctif qu'ils gardent ; un test fragile sous charge et quatre mineurs corrigés, six commits, de `7560471` à `90aea8b` ; suite e2e entière : 171 verts ; le détail est au registre, « Piste C, les e2e relus »). **La piste C de la relecture finale est close.** **Task 13a : faite, relue deux fois, reprise sur `main`** par avance rapide le 2026-11-13 (vingt et un commits, de `76286f4` à `c31df15` ; R220 à R239 au registre ; la note « Déplacements concurrents » de `CLAUDE.md` récrite, le piège « correctif de y-prosemirror » ajouté) ; Vitest sur `main` : 880 verts et 32 échecs attendus. **Plus aucune branche locale ne porte de travail hors de `main`**, hors les ateliers en cours. Reste de la Task 13 : la 13b (R371 à R379), après la fin de la piste S, puis la 13c avec la Task 14.

### Ce qui est sur `main`, poussé

- **Tâches 1 à 12** de la partie A, closes et relues.
- **Relecture finale, piste C (client)** : les neuf constats sont faits. C1 à C8 sont relus (une relecture, deux tours de correction, une re-relecture) ; C9 et les e2e de C6 et C8 ont été écrits dans un atelier contre le vrai serveur, puis repris sur `main` ; ils ne sont pas encore relus.
- **Relecture finale, piste S (serveur)** : les sept corrections S1 à S7 (R198 à R206), puis, après leur relecture, **la correction du défaut critique C1** (`869a0a0`, R207 et R208 : une mise à jour où un client paraît deux fois vidait la page chez tous ; présent depuis le lot 1). **Le reste de ce que la relecture a trouvé n'est pas corrigé** : trois importants (I1, I2, I3) et cinq mineurs. Tout est dans le [fichier de la relecture finale](2026-11-12-p1-lot3-relecture-finale.md), section « Relecture de la piste S ».
- **Plan de la partie B**, relu par un relecteur indépendant et corrigé (`98b06e8`, 21 tâches).
- Un e2e qui échouait de temps en temps, `space_without_a_role_is_not_found`, corrigé à la pause (il comparait l'écran pendant que la barre latérale chargeait encore).

### Ce qui n'est pas sur `main` : une branche locale, jamais poussée (deux à la pause)

Elle n'existe que dans le dépôt du poste de Lenny (`git branch` la montre ; son atelier est sous `.claude/worktrees/`, ignoré par git). **Un clone neuf ne l'a pas.** Si le poste n'est plus là, la Task 13a est à refaire d'après ce qui suit et son rapport. (La seconde branche de la pause, celle de la Task C6, est reprise sur `main` depuis.)

**1. Task 13a, la forme à clés** — branche `worktree-agent-acd5330e6b63d3097`, atelier `.claude/worktrees/agent-acd5330e6b63d3097`, base `ca29845`, onze commits, de `4ff23e5` à `65089b2`. L'atelier a son `node_modules` avec le correctif installé, et le fichier non suivi `task-13a-report.md`.

- **État : écrite en entier par son implémenteur, qui la rend « faite, avec des réserves ».** Son rapport complet est recopié dans [`2026-11-13-p1-lot3-task13a-rapport.md`](2026-11-13-p1-lot3-task13a-rapport.md) : ce qui est livré, les comptes de structures geste par geste, l'état de chaque scénario S1 à S11, les mesures, les Rulings R223 à R229, ce que la documentation devra dire.
- Ce qu'elle livre : `patches/y-prosemirror@1.3.7.patch` (déclaré dans `pnpm-workspace.yaml`, pas dans `package.json` : R226), `apps/web/src/editor/followBlock.ts` branché dans `Editor.tsx`, `prosemirror-state` 1.4.4 en dépendance directe (décision du contrôleur), `keyedBlocks.test.ts`, `followBlock.test.ts`, `blockMoves.test.ts` durci, les aides `apps/web/src/test/editors.ts` et `node.ts` (**la tâche C1a de la partie B doit les reprendre, pas les recréer**), six e2e neufs, et 136 fixtures pour la 13b. Mesures de l'implémenteur : Vitest 670 verts et 32 échecs attendus ; suite e2e 154 verts ; budgets sans recul, mais bruités.
- **Trois écarts à valider, que le contrôleur tient pour acceptables sous réserve de la relecture** : R223 (la branche à clés ne joue que si un bloc présent avant et après a changé d'ordre : sans cela, 4 gestes ordinaires sur 21 changeaient, ce qui était un critère d'arrêt ; conséquence : hors réordonnancement, un élément change encore d'`id` sur place, **et la Task 13b ne doit pas compter sur « un élément ne change jamais d'`id` »**) ; R224 (une garde de plus dans `restoreRelativeSelection`) ; R228 (`followBlock` suit aussi les sélections de nœud et les blocs tenus pendant un glisser ; il corrige au passage un défaut présent sur `main` : un changement reçu pendant qu'on tient un bloc au bout de la souris vidait ce bloc au dépôt).
- **Sa relecture indépendante a été arrêtée par la pause à la fin de la lecture : aucun constat n'est prouvé.** Le relecteur a rendu neuf pistes, toutes « lues, non prouvées », dans l'ordre où les éprouver :
  - **P1, la plus sérieuse, probable régression par rapport à `main`** : après une Entrée au milieu d'un bloc qui a des enfants (cas où R223 garde le code d'origine, qui renomme l'ancien élément), `followBlock` voit que l'`id` du bloc du curseur a changé et tire le curseur de l'autre personne dans le bloc neuf qui porte l'ancien `id` : Tom tape à la fin d'une puce qui a des sous-puces, Mona presse Entrée au milieu, la suite de la frappe de Tom part à la fin de la première moitié. Essai : la page `PARENTED` de `followBlock.test.ts`, `cursor(tom, "x", 6)`, `pressAt(mona, "x", 3, "Enter")`, échange, puis le curseur de Tom avec et sans l'extension. Correction à éprouver : ne suivre que si la position relative est morte (`isGone`), et garder « l'`id` a changé » pour le seul cas sans position relative (l'annulation).
  - **P3** : les essais au hasard (`keyed_and_unkeyed_tabs_converge`, `tokens_typed…`) montent leurs éditeurs sans `followBlock`, et tapent par `typeAt(id, offset)`, pas au curseur : le correctif et l'extension ensemble ne sont éprouvés que par vingt tests dirigés et les e2e. À écrire d'abord : le hasard avec l'extension, la frappe à la sélection courante, à grande échelle (centaines de graines, blocs imbriqués, types mêlés, colonnes, tableaux, annuler et rétablir, un onglet corrigé contre un onglet d'origine).
  - **P2** : un vrai clic sur une image sélectionne sans doute le nœud de contenu, qui n'a pas d'`id` : la sélection ne serait pas suivie, et Suppr retirerait le contenu d'un voisin ; l'aide `selectBlock` des tests sélectionne le conteneur, pas ce que fait un clic.
  - **P4** : annuler un déplacement supprime le conteneur recréé avec **tous** ses descendants, quel qu'en soit l'auteur (un bloc qu'un autre a rentré dessous par Tab disparaît en entier) : la limite écrite de R220 ne le dit pas.
  - **P5** (R227) : un glisser de plusieurs blocs recrée les blocs **croisés**, pas ceux du geste ; « le texte des autres blocs n'est pas touché » est faux dans ce cas : à porter à la limite écrite et à la 13b.
  - **P6** : deux blocs du même `id` (deux glissers du même bloc) reçoivent chez chaque client un `id` neuf par `UniqueID`, sans écriture ; la première transaction locale l'écrit, et les fils de commentaires du bloc sont perdus ; un cas mêlé reste à éprouver (jumeau renommé chez moi, puis je déplace un voisin).
  - **P7** : une position relative périmée après une annulation ; **P8** : deux endroits de `appendTransaction` qui lèveraient s'ils étaient atteints (une exception y bloquerait l'éditeur) ; **P9** : `binding.mapping` garde les descendants du bloc recréé à chaque déplacement local (fuite, sans doute mineure).
  - Pas regardé du tout : la mesure de R223 refaite, les gestes ordinaires manquants (couper-coller d'un bloc, glisser vers un autre niveau ou dans une colonne, fusion, tableaux, « Transformer en »), la composition (IME), la lecture seule, l'aperçu d'une version, le côté serveur (garde, dédoublonnage, fixtures), le coût, les mutations, les 32 `test.fails` et les e2e neufs.
  - Son verdict provisoire, qu'il dit ne reposer sur aucune preuve : rien à redire à la lecture dans `updateMovedChildren` ; le risque est dans `followBlock` ; « pas telle quelle » si P1 se confirme, « reprise après correction de `followBlock`, avec la limite récrite » si P3 ne trouve ni divergence ni exception.
- À corriger à la reprise sur `main` : le message du commit `996bb6c` dit à tort qu'un dépôt laisse une sélection de nœud (il laisse un curseur de texte à la fin du bloc déposé). La branche part de `ca29845` : elle est à rebaser sur `main`, qui a beaucoup changé sous `apps/web`.

**2. Partie B, Task C6, « fil d'Ariane et récents » : reprise sur `main` le 2026-11-13** (dix-huit commits, de `391d9fe` à `c2ebc75`). Elle a été relue par un relecteur indépendant (deux constats importants, corrigés : « Récents » déplaçait l'arbre sous le pointeur, et les récents d'un compte restaient dans le navigateur après sa déconnexion), re-relue (les deux levés), et un dernier mineur a été corrigé (l'ordre de « Récents » figé sous le pointeur, R298 ; cette correction-là n'est pas re-relue). Tout est au registre, « Partie B, piste client » : R290 à R298, les mesures, ce qui est à porter au README. Vitest sur `main` après reprise : 558 verts et les 12 échecs attendus ; typecheck et licences propres ; suite e2e entière dans l'atelier : 171 verts.

### Partie B : ce qui est décidé, ce qui n'est pas ouvert

- Le plan corrigé fait foi. **Choix de produit pris par le contrôleur le 2026-11-13, à confirmer par le propriétaire** (ils sont en tête du plan) : les tables de la partie B vont dans une migration `0004` (pas dans `0003`), pour qu'une instance puisse passer à la partie A sans attendre B ; un favori n'avance pas la version de l'espace ; une invitation n'est pas consommée si le compte a déjà ce niveau par un autre chemin (M7) ; la copie d'une sous-page dont on ne peut pas modifier la page parente va dans « Privé » ; mentionner une page en montre le titre à qui lit la mention (forme de la conception) ; pas de quota de fichiers avant le lot 7 ; « Récents » sous les trois sections (R296).
- **Fait le 2026-11-13** : le registre a sa section « Partie B » (les décisions R240 à R257 recopiées, les trois titres « piste serveur », « piste client », « relais et clôture ») ; `CLAUDE.md` a « Pièges rencontrés à la partie B du lot 3 », avec « Serveur » et « Client ».
- La piste serveur de la partie B (S1 d'abord) attend que la piste S de la relecture finale soit close : mêmes fichiers, même arbre. Ports des e2e : piste C `5177` et `8084`, piste S et relais `5178` et `8085`.

### Ateliers et dossiers laissés sur le poste

| Chemin (sous `.claude/worktrees/`) | Contenu | À garder ? |
|---|---|---|
| `agent-acd5330e6b63d3097` | Task 13a (branche du même nom, préfixée `worktree-`) | **oui**, jusqu'à sa reprise sur `main` |
| `relecture-e2e` | atelier de la relecture des e2e de la piste C et de leurs corrections (branche `relecture-e2e`) | jusqu'à la reprise de ces corrections sur `main` |
| `target-relecture-e2e` | son dossier de compilation | supprimable |
| `agent-a31f94a866585f811` | prototypes jetables de l'essai du bloc déplacé | utile à la 13b ; supprimable après |
| `target-lot3b-client` | dossier de compilation partagé de la piste cliente de la partie B | supprimable (il se recompile) |

Le dossier `target/` du dépôt se nettoie sans risque quand aucune compilation ne tourne (`target/debug/incremental` a pesé 28 Go le 2026-11-13 ; les agents compilent depuis avec `CARGO_INCREMENTAL=0`). Sous Windows, `git worktree remove` échoue sur les chemins longs de `node_modules` (« Filename too long ») : supprimer le dossier par PowerShell (`Remove-Item -LiteralPath` avec le préfixe des chemins longs), puis `git worktree prune`.

## Ce que la Task 12 a mesuré (à tenir pour acquis)

y-prosemirror 1.3.7 ne regarde que l'état final du document : après chaque transaction, il passe tout le document à `updateYFragment` (`sync-plugin.js`), qui saute le préfixe et le suffixe identiques puis traite les enfants du milieu **par position**. Deux nœuds de même nom face à face : l'élément Yjs est récrit en place (attributs, dont `id`, et texte par diff). Noms différents : l'élément est supprimé et recréé.

| Geste | Ce qu'il écrit dans Yjs | Une frappe concurrente |
|---|---|---|
| Glisser un bloc au même niveau, blocs de même type, sans enfants | aucun élément créé ni supprimé ; chaque conteneur de la plage **change d'`id`** et son texte est récrit par diff | dans **n'importe quel bloc de la plage** (déplacé ou non), elle atterrit **dans le bloc voisin**, chez tous et sur le serveur |
| Idem, avec un bloc d'un autre type dans la plage (un titre) | l'élément de contenu est supprimé et recréé | **perdue** |
| Bloc avec enfants | le groupe des enfants est supprimé et recréé ailleurs avec les mêmes `id` | dans un enfant : **perdue** |
| Tab, Maj+Tab | nouveau conteneur au même `id`, original supprimé | **perdue** |
| Déplacement au clavier (Mod+Maj+↑/↓) | récriture en place, comme le glisser | chez le voisin |
| **Deux déplacements concurrents, sans aucune frappe** | — | la page s'abîme : un bloc qui disparaît dans le texte d'un autre, un bloc en double avec le même `id` ; le dédoublonnage du serveur ne le voit pas (il ne parcourt la page que si la mise à jour insère des éléments) |

Le tableau complet, scénario par scénario (S1 à S11) et test par test, est au registre, « Task 12, correction 1 ».

## Task 13 : où elle en est

Le plan d'origine (R138) reposait sur « un déplacement vaut copie + suppression », faux pour la plupart des gestes. L'essai borné a rendu son verdict : **faisable, mais pas en entier dans ce lot**. La tâche est récrite à la fin du plan :

- **13a, client, la forme à clés (R220)** : **écrite, sur une branche locale, relecture à finir** (voir « État à la pause »). Avec elle, un bloc glissé est recréé seul au même `id` ; une frappe faite dans un autre bloc reste à sa place (S4, S5), deux glissers de blocs différents sont justes (S11). Régressions acceptées par R220, **à faire confirmer par le propriétaire** : ce qui est tapé dans le bloc déplacé passe de « chez un voisin » à « perdu » tant que la 13b n'est pas là ; annuler un déplacement retire ce que d'autres ont écrit dans le bloc depuis (et, d'après la piste P4, ses sous-blocs) ; un correctif de dépendance à porter à chaque montée de version de y-prosemirror.
- **13b, serveur, le report à l'arrivée sans état (R221)** : rien d'écrit. Le serveur reconnaît un bloc recréé sous l'`id` d'un bloc vivant que le jeu de suppressions couvre, et reporte sur la copie ce que ce jeu ne couvre pas. La table `block_moves` de la migration `0003` est retirée. Elle vient **après la 13a reprise sur `main`** (elle rejoue ses fixtures) **et après la piste S** (mêmes fichiers : `guard.rs`, `page.rs`, `actor.rs`). Elle doit valoir pour n'importe quel bloc recréé, pas seulement « celui du geste » (R227), et ne pas compter sur des `id` stables hors réordonnancement (R223).
- **13c, la limite écrite (R222)**, avec la Task 14 : une frappe faite sans avoir vu le déplacement et arrivée après lui reste perdue (yrs 0.28.1 n'offre pas la lecture qu'il faudrait ; contournement mesuré : 6,9 s sur 10 000 blocs). Elle aura son plan à part après le lot 3.

Si la relecture de la 13a conclut qu'il faut y renoncer : la 13b tombe avec elle, la limite mesurée à la Task 12 est écrite telle quelle (README, conception §11, `CLAUDE.md`), et la partie B n'en dépend pas (aucun de ses gestes n'est un réordonnancement).

## Ce qui reste, dans l'ordre

Les points 1, 2 et 3 n'ont aucun fichier commun et se mènent de front, chacun dans son arbre.

1. **Piste S, finir le tour de correction 1** (arbre principal) : I1 (appliquer le fichier `.patch`, voir les tests rouges, écrire le correctif), I3, I2 avec m7, puis m1, m2, m3, m6 ; les décisions sont dans le fichier de la relecture finale. Puis une re-relecture ciblée du tour, C1 compris, et le doute resté ouvert sur C1 (une entrée hostile déjà au journal).
2. ~~Task 13a, finir la relecture~~ : **fait**, la tâche est sur `main` depuis le 2026-11-13.
3. ~~Partie B, Task C6, finir le tour~~ : **fait**, reprise sur `main` le 2026-11-13.
4. ~~Faire relire les e2e de C9~~ : **relus** le 2026-11-13 ; reprendre sur `main` les corrections qui en sortent (atelier `relecture-e2e`). Et **côté client**, après I2 : le bouton « Supprimer le fil » pour l'accès complet, les messages des refus neufs, et les textes des plafonds mis d'accord après m3.
5. **Task 13b**, après les points 1 et 2 ; relue.
6. **Task 13c et Task 14** : documentation relue contre le code final (le fichier de la relecture finale liste ce qu'elle doit dire ; le rapport de la 13a aussi), la limite R222 écrite, R220 à R229 au registre, et la checklist de recette `docs/recette/2026-11-12-p1-lot3a-recette.md`. Y porter aussi : la phrase « les deux coupes » de `CLAUDE.md` et de R190 (reports de la Task 10), et les mineurs de documentation de la Task 12.
7. **Troisième passe de la relecture finale** : la documentation contre le code ; plus une passe ciblée sur la Task 13.
8. **Toutes les suites sur un clone neuf**, comme la CI (commandes dans `docs/REPRISE.md`), plus les budgets en release et ceux du navigateur, au calme (les mesures du 2026-11-13 sont bruitées : plusieurs suites tournaient de front). À surveiller : `general_access_makes_a_private_page_a_space_page` a échoué une fois sous cette charge, puis passé seul.
9. **Essai de migration sur une copie d'une base du lot 2** (jamais sur `data/instance` elle-même).
10. **Clôture de la partie A** : registre (mesures de clôture, reports, ce qui attend le propriétaire), `docs/REPRISE.md` et README à jour, ce fichier, celui de la relecture finale et le rapport de la 13a supprimés ou fondus au registre.
11. **Partie B**, d'après son plan : sa piste serveur dès que le point 1 est clos (S1, puis S2 après la 13b ou la 13b reportée par écrit) ; sa piste cliente après le point 3 (C7 attend S5 ; C1a attend que la 13a soit close, livrée ou abandonnée).
12. Après le lot 3 : un plan à part pour les frappes tardives pendant un déplacement (R222).

**Ordre de grandeur**, d'après le rythme mesuré le 2026-11-13 (une tâche fait deux à trois heures entre l'écriture, la relecture et un ou deux tours de correction) : dix à quatorze heures de travail d'agents pour finir la partie A ; trente à trente-cinq pour la partie B, dont la piste cliente fixe la durée.

## Ce qui attend le propriétaire

- **Un défaut critique vieux du lot 1 est corrigé sur `main`** (C1 : une mise à jour forgée par quelqu'un qui peut modifier une page la vidait chez tous). **Une instance qui tourne avec un binaire d'avant le 2026-11-13 l'a encore.** La mettre à jour demande un binaire de ce lot, donc d'attendre la clôture de la partie A (la migration `0003` change encore).
- **L'historique se lit à partir de « Peut modifier »** (décidé à la relecture finale, fait dans le code : S5 et C7). Avant, quelqu'un à qui on partageait une page pouvait lire, dans une version d'avant, un texte effacé avant le partage. La conception ne dit pas le niveau ; la décision se défait en deux lignes.
- **La forme à clés et ses régressions acceptées** (R220, R223), et **la limite qui reste** (R222) : voir « Task 13 », plus haut.
- **Les choix de produit de la partie B** pris par le contrôleur (plus haut, « Partie B »), dont « Récents » sous les trois sections.
- **Les lignes de la P1 du §4.5 qui ne sont dans aucun lot**, à placer : oEmbed, Mermaid, icône et couverture de page, emojis personnalisés, pleine largeur, petite police, verrouillage, « Modifié il y a… », aperçu de lien du signet, visionneuse PDF, bloc « fil d'Ariane ».
- **Restaurer une version remplace tout le contenu**, comme `PUT /mmd` : une frappe faite au même instant par quelqu'un d'autre est perdue (R188). L'écran le dit quand d'autres ont la page ouverte. L'écriture par différence du lot 5 (R40) le réglera pour les deux.
- **Récrire ou restaurer une grande page occupe son acteur longtemps** : 5 s pour 10 000 blocs, 48 s pour 30 000 (I3, antérieur au lot) ; un plafond de blocs et, si possible, un correctif sont décidés, pas encore faits.
- **Les deux branches locales** (Task 13a, Task C6) ne sont pas poussées : les pousser sur le dépôt, public, est une décision qui lui revient.
- **L'adresse du dépôt** : GitHub redirige vers `Arlonium/Momentum` ; le dépôt local pointe encore sur l'ancienne adresse, qui marche par redirection.
- **Des sous-agents se sont vu refuser l'écriture de leur fichier de rapport** par l'outil ; leurs comptes rendus sont passés par message, et la documentation en garde l'essentiel. Rien n'a été perdu.
- Les choix R132 à R138 du plan ont été validés avec lui le 2026-11-12 ; ceux pris pendant l'exécution sont au registre, dans les plans ou dans ce fichier, chacun avec son coût s'il est faux.

## Reports mineurs

Constats « Mineurs » des relectures de tâche, non corrigés. ✔ : levé depuis par une tâche suivante. ➜ : repris par un constat de la relecture finale (à faire). Les autres ont été vus par la passe intégration, qui a conseillé de les laisser, sauf mention.

**Task 1 (trou d'horloge)**
- Le réessai des files retenues coûte O(connexions × clients) : `has_reached` reconstruit le vecteur d'état par connexion et par tour (`actor.rs` `release_held`, `page.rs`) ; le calculer une fois par tour.
- ✔ Une mise à jour retenue déjà couverte par l'étape 2 était réécrite au journal et relayée (levé par R181).
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
- ➜ Un canal à demi ouvert, sans bruit, laisse `live` vrai et le minuteur coupé (S6 et C6).
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
- Passe sécurité : la liste des présents donne à un invité l'UUIDv7 de chaque compte présent, donc sa date de création (à écrire dans le coût de R136) ; la trame `roster` coûte O(N²·M) pour N arrivées, et un lecteur peut la déclencher en boucle.
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
- ➜ Aucun plafond (S4, C8) ; chaque commentaire sur un bloc relit la page (S7).

**Task 9 (commentaires, client)**
- Deux relectures par écriture locale (la sienne, puis sa propre trame).
- ➜ Première lecture en `401` sans « Réessayer » quand la connexion de synchro reste ouverte (C2).
- Le bouton de survol n'est pas montré sur un bloc qui a déjà un fil ouvert.
- Sur macOS, Cmd+Option+M est un raccourci système (non vérifié) ; sur un clavier allemand, Ctrl+Alt gauches + M avale « µ » quand le navigateur ne signale pas AltGr (coût écrit de R176).
- ✔ Un brouillon dont la personne perd le droit d'écrire restait en mémoire sans se montrer (levé par C5, R211).
- Deux brouillons de modification dans le même fil : seul le premier se rouvre.
- Un refus du serveur après un remontage pendant l'envoi n'est affiché que dans l'ancienne zone (en partie levé par R211 : le refus est gardé par le brouillon).

**Task 10 (historique, serveur)**
- **À porter à la Task 14** : `CLAUDE.md` et R190 parlent de « deux coupes », alors que la copie de l'exception se répète à chaque restauration de la plus ancienne d'un historique de 100 : la date, la raison et les auteurs des versions ordinaires s'effacent une à une (le contenu n'est jamais perdu).
- `Hub::shutdown` dans `serve` n'a pas de test (extraire un `serve_until`).
- Une suppression en attente qui ne se résout jamais laisse `has_pending_deletions` vrai : les doublons sans effet reviennent pour cette page (S2 retire cette branche).
- Restaurer ou récrire une grande page : 5 s d'acteur occupé (voir plus haut).
- ➜ Des restaurations en file s'exécutent après le retrait du droit (S1) ; l'historique lu dès « Peut lire » (S5).

**Task 11 (historique, client)**
- Le piège de Tab et le clic sur le voile sont dupliqués entre `HistoryDialog` et `Confirm`.
- La mise en garde « d'autres personnes ont cette page ouverte » disparaît quand la session est coupée.
- Le coût de l'aperçu d'une grande version n'est pas mesuré (un second éditeur complet) : à mesurer à la clôture.
- `ui/Avatar.tsx` (menu du compte, membres) garde sa propre règle d'initiales.
- Budgets du navigateur à relancer à la clôture (le démarrage a grossi : le panneau des commentaires est dans le paquet de démarrage, 129 Ko).

**Task 12 (tests du bloc déplacé)**

À faire au Step 1 de la Task 13a, « durcir la cible », avant de rien corriger :
- tout est figé à Mona = 2 < Tom = 3, or l'issue change avec l'ordre des identifiants de client (prouvé : S6 sans doublon dans l'ordre inverse) : un second jeu de scénarios et de fixtures, identifiants inversés ;
- la garde des préconditions s'inverse hors de `test.fails` (`if (!locallyDone) return` : un scénario repassé en `test` serait vert sur une précondition manquée) : elle doit lever ;
- `test.fails` accepte n'importe quelle exception (S6 : le glisser de Tom n'a pas de témoin) : affirmer que l'échec est l'assertion attendue ;
- fixtures du navigateur et tests de l'acteur manquants pour S2, S3, S5 à S11 ;
- la comparaison côté Rust ne lit que le premier niveau (ni enfants ni type) ;
- les aides `drag` et `tab` court-circuitent le vrai point d'entrée (dépôt, touche) : une correction posée sur le geste ne serait pas exercée ;
- aller-retour : l'état final de l'acteur rechargé dans deux éditeurs ne doit rien faire récrire.

À porter à la Task 14 :
- la note « Déplacements concurrents » de `CLAUDE.md` dit « en doublent un » (vrai pour un seul ordre d'identifiants) et « que le serveur ne dédoublonne pas » comme un fait (lu, non rejoué) ;
- « des textes sans rien en commun » (registre) est inexact pour l'exemple donné ;
- l'en-tête de `moves.rs` cite les scénarios au lieu des tests de forme ; un diff Vitest est inversé (attendu et reçu) ;
- `crates/mementum-crdt/src/dedupe.rs` et `tests/move_edit.rs` portent encore « un déplacement vaut copie + suppression ».
