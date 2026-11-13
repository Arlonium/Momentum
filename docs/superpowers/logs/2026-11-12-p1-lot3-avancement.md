# Lot 3, partie A : où en est l'exécution

Ce document est fait pour reprendre l'exécution du plan là où elle s'est arrêtée, sans rien savoir de la session qui l'a menée. Il complète le registre du lot, qui garde les décisions ; lui garde l'avancement, ce qui était en cours, et la liste des reports.

- Plan : [`docs/superpowers/plans/2026-11-12-p1-lot3-collaboration-blocs-riches.md`](../plans/2026-11-12-p1-lot3-collaboration-blocs-riches.md) (14 tâches ; la Task 13 est récrite à la fin du fichier, en 13a, 13b et 13c).
- Registre des décisions (Rulings R129 à R211) : [`2026-11-12-p1-lot3-ledger.md`](2026-11-12-p1-lot3-ledger.md), une section par tâche et par tour de correction.
- Relecture finale, constats et vague de correction : [`2026-11-12-p1-lot3-relecture-finale.md`](2026-11-12-p1-lot3-relecture-finale.md).
- Essai du bloc déplacé (Task 13) : [`2026-11-12-p1-lot3-essai-deplacement.md`](2026-11-12-p1-lot3-essai-deplacement.md).
- Plan de la partie B, écrit, ni relu ni exécuté : [`docs/superpowers/plans/2026-11-12-p1-lot3b-blocs-riches-organisation.md`](../plans/2026-11-12-p1-lot3b-blocs-riches-organisation.md).

**État écrit le 2026-11-12 au soir, à la pause demandée par Lenny, puis tenu à jour depuis la reprise du 2026-11-13** (section « Depuis la reprise », juste après le tableau des tâches). Ce qui était en cours à la pause est décrit plus bas et dans le fichier de la relecture finale. **Mettre ce fichier à jour à chaque tâche close, et le supprimer à la clôture de la partie A**, avec celui de la relecture finale (le registre et `docs/REPRISE.md` suffiront alors).

## Comment l'exécution est menée (R140)

Une session « contrôleur » exécute le plan tâche par tâche, directement sur `main`, sans branche ni worktree par tâche :

1. un sous-agent implémente la tâche en TDD (test vu rouge d'abord), commite par petits commits, écrit ses écarts au registre (`Ruling RNN: … — … — …`) ;
2. un autre sous-agent relit la tâche (conformité au plan, puis qualité), **en prouvant ses constats par des tests jetables**, et en relisant toute règle de protection comme une attaque ;
3. chaque constat « Important » repart chez l'implémenteur (tour de correction), puis une re-relecture ciblée vérifie le tour ; cinq tours au plus ;
4. les constats « Mineurs » ne bloquent pas : ils sont listés plus bas.

Les dossiers de travail du contrôleur (briefs, rapports, paquets de relecture) sont sous `.superpowers/sdd/`, ignoré par git : ils ne sont pas dans le dépôt. Tout ce qu'il faut pour reprendre est ici, dans le fichier de la relecture finale, au registre et dans l'historique git.

**Numéros des Rulings.** Le registre va jusqu'à R211, mais des plages sont réservées, pour que des travaux menés de front ne se croisent pas : **R198 à R209** à la piste S de la relecture finale (aucun pris ; R198 et R199 prévus pour S2), **R210 à R219** à la piste C (R210 et R211 pris), **R220 à R239** à la Task 13 (R220 à R222 pris par son découpage, écrits dans le plan, pas encore au registre), **R240 à R257** au plan de la partie B, dont l'exécution prendra **R260 à R349**. Une décision neuve hors de ces travaux prend R258 ou R259, puis R350 et plus.

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
| 13. Édition gardée pendant un déplacement (R39, R138) | **essai rendu, tâche récrite en 13a, 13b, 13c ; aucune ligne écrite** | — | R220 à R222 (dans le plan) |
| 14. Documentation et recette de la partie A | à faire | — | — |
| Relecture finale | passes sécurité et intégration rendues ; **vague de correction arrêtée à 2 constats sur 16** ; passe documentation à faire | — | R210, R211 |

Dernières mesures (poste de Lenny, 2026-11-12, à la tête poussée, **sans clone neuf**) : `cargo fmt` et `cargo clippy -D warnings` propres ; 681 tests Rust verts (les ignorés : les mesures en release et les tests de la limite R39) ; **462 tests du client verts et 12 marqués `test.fails`** (les scénarios du bloc déplacé), typecheck propre, relancés à la pause ; licences npm propres (aucun paquet ajouté depuis) ; la dernière suite complète de bout en bout date de la Task 11 : 148 parcours verts et 2 marqués `fixme` (ceux de la Task 12), **non relancée depuis les deux correctifs du client** (`8a8ced1`, `ddfaf93`) ; budgets du navigateur relancés à la Task 11 : page de 1 000 blocs à 142 ms au p95 (budget 300), page de 10 000 blocs en 1,2 s (budget 3 s) ; côté serveur, la frappe qui déclenche la coupe d'une version reste vue par un autre en 39 ms au p95 (budget 150). Le code Rust n'a pas changé depuis `2e03359`.

## Depuis la reprise (2026-11-13)

Quatre travaux ont été relancés de front, sans fichier commun. L'état constat par constat est tenu dans le [fichier de la relecture finale](2026-11-12-p1-lot3-relecture-finale.md).

- **Piste C de la relecture finale** : C2, C3, C4 faits ; parts clientes de C6, C7, C8 faites contre un serveur simulé (`abdffee`, `9151e88`, `29ab75a`, `973b78e`, `b3a4c36`, `38149d4` ; R212 à R217). Vitest : 497 verts et les 12 échecs attendus. **En relecture indépendante.** Restent, après la piste S : les e2e de C6, C7, C8, puis C9.
- **Piste S** : S2 fait (`414df88`, R198 et R199) ; S1 en cours ; puis S3, S5, S4, S6, S7.
- **Task 13a** : en cours, dans un atelier git à part (un agent neuf ; ses Rulings, R223 à R229, viennent dans son rapport et seront portés au registre à la reprise sur `main`).
- **Plan de la partie B** : relu par un relecteur indépendant (7 constats bloquants, 12 importants, 17 mineurs ; verdict « exécutable après corrections ») ; **en cours de correction** dans le fichier du plan. Décidé par le contrôleur à cette occasion, à confirmer par le propriétaire : les tables de la partie B vont dans une migration `0004` (pas dans `0003`) ; un favori n'avance pas la version de l'espace ; une invitation n'est pas consommée si le compte a déjà ce niveau par un autre chemin ; la copie d'une sous-page dont on ne peut pas modifier la page parente va dans « Privé » ; mentionner une page en montre le titre à qui lit la mention (forme de la conception) ; pas de quota de fichiers avant le lot 7.

Règle ajoutée pour les agents qui commitent dans le même arbre : `git commit -m "…" -- <chemins>`, pour ne rien emporter de ce qu'un autre a mis à l'index.

## Ce qui était en cours à la pause

Quatre sous-agents travaillaient. Tous ont été arrêtés à un point sain : **rien n'est à moitié fait dans l'arbre de travail**, et rien n'existe ailleurs que sur `main` (aucune branche de travail à reprendre).

### 1. Vague de correction de la relecture finale

Le détail est dans le [fichier de la relecture finale](2026-11-12-p1-lot3-relecture-finale.md) : chaque constat, sa preuve, la décision, les tests à garder, et l'état.

- **Piste C (client)** : C1 (le titre) et C5 (les brouillons sans droit d'envoi) sont faits, commités et poussés (`8a8ced1`, `ddfaf93`, registre `a1b970c`, R210 et R211) ; **pas encore re-relus**, et les e2e n'ont pas été relancés. C2 : rien d'écrit, conception décidée. C3, C4, C6, C7, C8 : pas commencés. C9 (sept e2e) : pas commencé, attend la piste S.
- **Piste S (serveur)** : **aucun commit**. S2 était en cours : ses tests sont écrits mais jamais compilés, gardés dans [`2026-11-12-p1-lot3-piste-s-s2-tests.patch`](2026-11-12-p1-lot3-piste-s-s2-tests.patch) ; son correctif est conçu, pas écrit. S1, S3, S4, S5, S6, S7 : pas commencés.

### 2. Task 13a (la forme à clés, client)

**Rien n'est écrit ni commité** : l'implémenteur n'avait fait que lire quand la pause est arrivée. Son atelier était propre et a été retiré. Ce qu'il a appris en lisant, et qui n'est ni dans le compte rendu de l'essai ni dans le plan :

1. **Quel fichier de y-prosemirror est chargé.** Une seule copie dans `node_modules/.pnpm/`. Ses `exports` : `import` vers `./src/y-prosemirror.js` (le code est dans `src/plugins/sync-plugin.js`), `require` vers `./dist/y-prosemirror.cjs` (`updateYFragment` vers la ligne 1219). BlockNote 0.57 l'importe en ESM : Vite en développement, le build et Vitest devraient tous charger `src/`. **Déduit de la lecture, pas vérifié.** Corriger les deux fichiers ; le prouver par un test Vitest qui appelle `updateYFragment` importé en ESM et en CJS (`createRequire`). Le cache `node_modules/.vite` doit être invalidé par le changement du verrou.
2. **Où pnpm écrit `patchedDependencies`** : avec la version de pnpm du dépôt, `pnpm patch-commit` l'écrit probablement dans `pnpm-workspace.yaml`, pas dans `pnpm.patchedDependencies` du `package.json` comme le dit le plan. À vérifier, puis corriger le plan.
3. **Où la branche à clés jouera** : `updateYFragment` reçoit le binding comme `meta`. Elle ne joue que pour les listes dont tous les enfants ont un `id` : enfants d'un `blockGroup`, d'un `columnList`, d'une `column`. Ailleurs (la racine, le contenu d'un `blockContainer`, les tableaux, le texte), c'est le code d'origine.
4. **Départager un échange de voisins** : pour deux plus longues sous-suites croissantes de même longueur, lire la sélection par `meta.prosemirrorView.state.selection` (absente quand `meta` est un objet nu, `prosemirrorToYXmlFragment`). Un premier calcul en O(n log n), puis un second sans les enfants sélectionnés ; garder le second s'il est aussi long. Pas de calcul en O(n²) : un grand glisser a un milieu de 10 000 enfants. L'aide `drag` des tests doit poser la sélection comme le dépôt de ProseMirror (une `NodeSelection` du bloc déposé).
5. **Charger les deux versions de `updateYFragment` dans un même test** (pour `keyed_and_unkeyed_tabs_converge`), piste retenue, pas essayée : recalculer le code d'origine dans le test en appliquant à l'envers `patches/y-prosemirror@1.3.7.patch` au `sync-plugin.js` installé (le test lève si les blocs ne collent pas, ce qui prouve que le correctif est installé) ; récrire chaque import en URL `file://` absolue ; vérifier que le module généré partage **les mêmes instances** de `yjs` et de `ySyncPluginKey` que le test (sinon `instanceof` et l'origine suivie par l'annulation cassent) ; l'onglet « d'origine » remplace `_prosemirrorChanged` sur son seul binding. Risque : la façon dont Vitest charge un fichier hors de `node_modules`. Repli : une copie gelée du fichier d'origine dans les tests.
6. **`followBlock` ne peut pas importer `prosemirror-state` depuis `apps/web`** : ce n'est pas une dépendance directe, et pnpm ne la remonte pas. **Décidé à la pause par le contrôleur** : la déclarer dans `apps/web/package.json`, à la version déjà résolue dans le verrou (1.4.4) ; aucun paquet neuf n'entre dans l'arbre, et le contrôle des licences doit rester vert. À écrire au registre avec la Task 13a. La transaction ajoutée (`appendTransaction`) doit reprendre la méta `ySyncPluginKey` de la transaction distante (`isChangeOrigin`) et `addToHistory: false`, sinon l'état du plugin de synchro repasse à `isChangeOrigin: false` et les décorations de curseur sont seulement reportées. Elle n'écrit rien dans Yjs (le verrou du binding est tenu par `_typeChanged`).
7. **Fixtures** : `crates/mementum-sync/tests/moves.rs` inclut par `include_bytes!` quatre noms figés (`base.bin`, `mona-drags-x-under-z.bin`, `tom-types-in-x.bin`, `tom-types-in-y.bin`). Les fixtures neuves par scénario et par ordre s'y **ajoutent** (en sous-dossiers) ; ces quatre fichiers restent, réécrits.
8. **Un doute sur « mêmes comptes qu'avant »**, à mesurer en premier sur le code d'origine : si BlockNote, pour Entrée au début d'un bloc, garde l'`id` sur le bloc vide du haut et en donne un neuf à celui qui garde le texte, le code d'origine renomme l'élément sur place, alors que la forme à clés viderait le texte et créerait un élément neuf : les comptes différeraient, et c'est un critère d'arrêt du plan. Même prudence pour toute transaction qui supprime un bloc et en modifie un voisin. Piste : ne prendre la branche à clés que quand **les deux** milieux sont non vides (insertion seule ou suppression seule : code d'origine tel quel).

Les sept reports de la relecture de la Task 12 sont le Step 1 de la Task 13a (« durcir la cible ») : voir plus bas.

L'atelier de l'essai est conservé sur le poste de Lenny, hors du dépôt (`.claude/worktrees/agent-a31f94a866585f811`, 207 Mo) : ses prototypes jetables servent à la Task 13b. Il n'est pas nécessaire pour reprendre : le compte rendu de l'essai donne le cœur du correctif.

### 3. Plan de la partie B

**Écrit et commité** (`178fe8d`), 19 tâches en deux pistes (serveur S1 à S6, client C1 à C10, relais X1, X2, D), décisions R240 à R257. **Ni relu par le contrôleur, ni validé par le propriétaire, ni exécuté.** Ce que son auteur signale, à trancher avant de lancer une tâche :

1. **Migration (R243)** : les trois tables neuves (`files`, `attachments`, `favorites`) vont-elles dans `0003` ou dans une `0004` ? Cela dépend de si une instance doit s'ouvrir avec la partie A avant la fin de B. À trancher avant sa tâche S2.
2. **Le correctif de y-prosemirror (Task 13a) n'est pas sur `main`** : ses tâches C1 à C5, C8 et C9 l'attendent ; C6 peut ouvrir la piste client.
3. **Reports du lot 2** (M5, M7, M9, M11, m1) : reconstruits dans le code, faute des rapports de relecture du lot 2 ; M7 est le plus incertain, et renverse un choix du lot 2 (une invitation sans effet n'est plus consommée).
4. **Fichiers servis en `attachment`** : l'affichage d'une image ou d'une vidéo servie ainsi n'est pas vérifié dans un navigateur ; s'il échoue, sa tâche S4 est rouverte.
5. **Choix sans appui dans la conception** : la copie d'une page de racine va dans « Privé » ; les récents sont gardés dans le navigateur ; un favori avance `lsn` ; un invité ne mentionne que les présents ; le signet n'a pas d'aperçu distant ; « duplication » couvre la page et le bloc.
6. **Lignes de la P1 du §4.5 sans lot**, à placer par le propriétaire : oEmbed, Mermaid, icône et couverture de page, emojis personnalisés, pleine largeur, petite police, verrouillage, « Modifié il y a… », aperçu de lien du signet, visionneuse PDF, bloc « fil d'Ariane ».

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

## Task 13 : ce que l'essai a conclu

Le plan d'origine (R138) reposait sur « un déplacement vaut copie + suppression », faux pour la plupart des gestes. L'essai borné a rendu son verdict : **faisable, mais pas en entier dans ce lot**. La tâche est récrite à la fin du plan :

- **13a, client, la forme à clés (R220)** : un correctif de y-prosemirror (`pnpm patch`, `updateYFragment` apparie les enfants du milieu par `id` de bloc) fait d'un réordonnancement « le seul bloc déplacé est supprimé et recréé au même `id` » ; une extension `followBlock` fait suivre le curseur. Régressions acceptées, **à faire confirmer par le propriétaire** : ce qui est tapé dans le bloc déplacé passe de « chez un voisin » à « perdu » tant que la 13b n'est pas là ; annuler un déplacement retire ce que d'autres ont écrit dans le bloc depuis ; un correctif de dépendance à porter à chaque montée de version.
- **13b, serveur, le report à l'arrivée sans état (R221)** : le serveur reconnaît un bloc recréé sous l'`id` d'un bloc vivant que le jeu de suppressions couvre, et reporte sur la copie ce que ce jeu ne couvre pas. La table `block_moves` de la migration `0003` est retirée. Après la 13a (elle rejoue ses fixtures) **et après la piste S** (mêmes fichiers).
- **13c, la limite écrite (R222)**, avec la Task 14 : une frappe faite sans avoir vu le déplacement et arrivée après lui reste perdue (yrs 0.28.1 n'offre pas la lecture qu'il faudrait ; contournement mesuré : 6,9 s sur 10 000 blocs). Elle aura son plan à part après le lot 3.

## Ce qui reste, dans l'ordre

1. **Finir la vague de correction** de la relecture finale : piste S (S2 d'abord, puis S1, S3, S5, S4, S6, S7) et piste C (C2, C3, C4, C6, C7, C8), de front, sans fichier commun ; puis C9 (les e2e). Faire re-relire chaque correction, C1 et C5 compris.
2. **Task 13a**, dans un atelier git à part (le correctif change `node_modules` et le verrou), relue, puis reprise sur `main`. Elle peut se mener en même temps que le point 1 : elle ne touche qu'à l'éditeur et à ses tests.
3. **Task 13b**, après la 13a et la piste S ; relue.
4. **Task 13c et Task 14** : documentation relue contre le code final (le fichier de la relecture finale liste ce qu'elle doit dire), la limite R222 écrite, R220 à R222 au registre, et la checklist de recette `docs/recette/2026-11-12-p1-lot3a-recette.md`. Y porter aussi : la phrase « les deux coupes » de `CLAUDE.md` et de R190 (reports de la Task 10), et les mineurs de documentation de la Task 12.
5. **Troisième passe de la relecture finale** : la documentation contre le code ; plus une passe ciblée sur la Task 13.
6. **Toutes les suites sur un clone neuf**, comme la CI (commandes dans `docs/REPRISE.md`), plus les budgets en release et ceux du navigateur.
7. **Essai de migration sur une copie d'une base du lot 2** (jamais sur `data/instance` elle-même).
8. **Clôture de la partie A** : registre (mesures de clôture, reports, ce qui attend le propriétaire), `docs/REPRISE.md` et README à jour, ce fichier et celui de la relecture finale supprimés.
9. **Partie B** : relire le plan, faire trancher ses points ouverts, puis l'exécuter.
10. Après le lot 3 : un plan à part pour les frappes tardives pendant un déplacement (R222).

## Ce qui attend le propriétaire

- **L'historique passe à « Peut modifier »** (décision du contrôleur à la relecture finale, pas encore dans le code : S5 et C7). Aujourd'hui il se lit dès « Peut lire » : quelqu'un à qui on partage une page peut lire, dans une version d'avant, un texte effacé avant le partage (prouvé). La conception ne dit pas le niveau ; la décision se défait en deux lignes.
- **La forme à clés et ses régressions acceptées** (R220), et **la limite qui reste** (R222) : voir « Task 13 », plus haut.
- **Restaurer une version remplace tout le contenu**, comme `PUT /mmd` : une frappe faite au même instant par quelqu'un d'autre est perdue (R188). L'écran le dit quand d'autres ont la page ouverte. L'écriture par différence du lot 5 (R40) le réglera pour les deux.
- **Récrire ou restaurer une page de 10 000 blocs occupe son acteur 5 secondes** (antérieur à ce lot). Aucun budget ne porte dessus.
- **Le plan de la partie B** et ses points ouverts (plus haut), dont les lignes de la P1 sans lot.
- **L'adresse du dépôt** : GitHub redirige vers `Arlonium/Momentum` ; le dépôt local pointe encore sur l'ancienne adresse, qui marche par redirection.
- **Un sous-agent s'est vu refuser l'écriture de son fichier de rapport** par l'outil, en cours de route ; ses comptes rendus sont passés par message. Rien n'a été perdu.
- Les choix R132 à R138 du plan ont été validés avec lui le 2026-11-12 ; ceux pris pendant l'exécution (R140 à R211, R220 à R222) sont au registre ou dans le plan, chacun avec son coût s'il est faux.

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
