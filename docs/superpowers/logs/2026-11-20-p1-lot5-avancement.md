# Phase 1, lot 5 « Agents » : avancement

Écrit le 21 novembre 2026, par la Task D, quand le code des quatorze tâches précédentes est sur `main` (`aad795e`). Plan : [`2026-11-20-p1-lot5-agents.md`](../plans/2026-11-20-p1-lot5-agents.md) (décisions R1210 à R1249) ; registre : [`2026-11-20-p1-lot5-ledger.md`](2026-11-20-p1-lot5-ledger.md) (R1250 et plus, par plages réservées à chaque tâche, débordements à partir de R1560 puis de R1600) ; mesures de la Task S0 : [`2026-11-20-p1-lot5-s0-mesures.md`](2026-11-20-p1-lot5-s0-mesures.md) ; recette du propriétaire : [`2026-11-20-p1-lot5-recette.md`](../../recette/2026-11-20-p1-lot5-recette.md).

## Où en est le lot

- **Le code et sa documentation sont finis sur `main`** : quinze tâches (S0 à S7, M1, M2, C1, C2, X1, X2, et D pour la documentation et la recette), de `69e9665` (le plan, le 20 novembre à 14 h 47) à `aad795e` (la dernière tâche de code, le 21 novembre à 13 h 24), puis la Task D et son tour de correction. Chaque tâche a eu une relecture, une seule, et ses constats ont été corrigés en un tour (méthode de R935, plus bas). Tout a été poussé au fil du travail (R934).
- **Le lot a attendu la clôture du lot 4** (décision L16, R1249) entre ses quatre premières tâches (S0, S2, S3, M1, le 20 novembre) et S1 : le lot 4 a été clos le 21 novembre à 3 h 33 (`5d10757`, R1596), et S1 est partie dans l'heure.
- **Restent, dans l'ordre** : la passe complète de fin de lot, une fois (« Fin du lot » du plan, et la liste plus bas) ; la liste des défauts ; la correction groupée ; la passe de contrôle ; la vérification de `claude mcp add` avec Alex (R1464, R1488) ; la recette du propriétaire ; la clôture, qui fige `0006`.
- **Les dix-huit décisions du plan** (L1 à L18) sont retenues par défaut depuis le 20 novembre (Lenny : « tu es autonome »), L13, L16 et L17 tranchées par le coordinateur à la relecture du plan, L18 ajoutée à la relecture de M1. Elles sont listées, avec les choix pris en route, en tête de la recette.
- **La migration `0006`** (jetons, journal d'activité, date des pages, `doc_updates.token_id`, `doc_versions` rebâtie pour `before_agent`) peut changer jusqu'à la clôture du lot ; elle a gagné un index au tour de correction de S1 (R1279). Aucun binaire du lot n'ouvre une instance que quelqu'un regarde.

## Les tâches

| Tâche | Ce qui est livré | Commits sur `main` (code ; registre) | Rulings |
|---|---|---|---|
| S0 | les mesures avant de bâtir, sur un banc jetable (`spike/agents-bench/`) : un vrai Claude Code devant les deux révisions de MCP, l'écriture par différence, le contenu en ligne par préfixe et suffixe, la restriction d'un accès, la version d'avant un agent | `ac893f3` ; `8bf3ea3`, `283563b` | R1250 à R1258 |
| M1 | crate `mementum-mcp` : le protocole, sans réseau, en deux révisions ; les six descripteurs d'outils ; la mise en texte bornée et étiquetée ; table de cas `spec/mcp/messages.json`, schémas `spec/mcp/tools.json` | `2896d64`, `999a43e`, `aeb2c48` ; `ec2e663`, `371f2a1` | R1405 à R1419, R1560 |
| S2 | `PageDoc::write_diff`, l'écriture d'une page par différence, et `diff_cost` ; les frappes du vrai Yjs | `abac2a5`, `bf7e97c`, `d21f16a`, `ff95c55`, `a4a62c4`, `7387f9a`, `d4292e1`, `ec38933` ; `160a58e`, `32c3fd8`, `b983c38` | R1280 à R1299, R1561 |
| S3 | les opérations d'édition (`ops.rs`), les alias de blocs et les empreintes (`anchors.rs`), la fusion à trois ; table de cas `spec/agents/operations.json` | `e49e8d2`, `ff080bb`, `4cc1892`, `927dbc7`, `e132b16` ; `b67a332`, `6465b2b` | R1300 à R1319, R1562 à R1571 |
| — | la relecture des mesures de S0, portée au plan | `68eedae` | R1572 à R1576 |
| S1 | migration `0006` ; jetons, journal d'activité, date des pages dans le store ; le changement de mot de passe qui révoque les jetons | `ee97575`, `38f1dd4`, `771caa9`, `c5ed3a8` ; `4da74a6`, `aecfafc` | R1265 à R1279, R1600 |
| S4 | l'écriture d'un agent dans l'acteur de la page, en une ou deux commandes, avec sa version `before_agent` hors de l'acteur ; `PUT /mmd` et la restauration par différence | `eb6231f`, `d979aa4`, `49cf4da`, `536b0d8`, `3658737`, `8a59f8b`, `c033c4a` ; `0a1c31f`, `f232e5c` | R1320 à R1339, R1601, R1602 |
| S5 | l'identité par jeton : la porte des routes, la restriction dans `access.rs` et `mementum-acl`, le débit, le journal d'activité | `e244855`, `34b6eb7`, `8214c26`, `629d237`, `61009ba`, `ecb8614`, `19b4481` ; `d93f043`, `e0c17a6` | R1340 à R1361 |
| S6 | les routes des jetons (`/api/v1/tokens`, leur activité) ; le changement de mot de passe qui dit ses jetons révoqués | `73f41a1`, `a90fd1b`, `e4b9ccb`, `5757165` ; `0487759`, `d468394` | R1365 à R1379 |
| C1 | l'écran « Jetons d'API » : création, jeton montré une fois, commandes par shell, révocation | `d14e323`, `dbc07da`, `0e752df`, `2ee2768`, `3341d43`, `61e6d72`, `96c995c`, `0b9de87`, `a1965a0` ; `eb50c6f`, `84c3939` | R1445 à R1464, R1620 à R1624 |
| C2 | l'activité d'un jeton, et « par un agent » dans l'historique (`Version.agentAuthors`) | `f01b334`, `4f07ff0`, `74c1963`, `3ee85d3`, `164b6c1`, `f00a4c0`, `fd27811` ; `daf9022`, `51ce1c7` | R1465 à R1479, R1625 à R1628 |
| S7 | les opérations des agents dans le serveur (`agent/`) : document par tranches, opérations, création en lot, `/me` et membres sans adresse, filtres de la recherche, `If-Match` | `05d1184`, `62a9cdd`, `0f3f4a7`, `c7647f7`, `d0aa6a4` ; `4989bb1`, `af85594` | R1380 à R1404, R1603 à R1611 |
| M2 | `/mcp` et `/mcp/readonly`, les six outils branchés sur `agent/` | `6de740c`, `04431cb`, `683b016`, `d4c4d7a`, `2d06a6e`, `276a2d9`, `1e20a99`, `bacfac4`, `df67bad` ; `e9dcd65`, `14bd035` | R1420 à R1444 |
| X1 | un agent et un humain de bout en bout (Chromium, client MCP de Node), et un vrai Claude Code dans les deux révisions | `2e8a8d7`, `ea1875d`, `65f3067` ; `e04656f`, `44040ff` | R1480 à R1489, R1629 à R1632 |
| X2 | les budgets : mesures, deux budgets neufs au §5.10, mesures bornées dans la CI ; la voie des jetons dans la file d'écriture ; l'acteur sans connexion arrêté après 3 s | `1d8557a`, `5b6ca33`, `7bf39b8`, `f5e6c65`, `daa5500`, `8cf952f`, `e9a6d4a`, `43baf5b`, `c910443`, `5750260`, `4a318b8`, `50856a8`, `21d714d` ; `40a972e`, `45f6990` | R1640 à R1662 |
| D | cette documentation, la recette, la spécification MMD, la conception, le README, `docs/REPRISE.md`, `CLAUDE.md` ; puis son tour de correction, après sa relecture | les commits `docs :` et `spec :` qui suivent `aad795e` | R1500 à R1509, R1663 |

Les commits du registre de chaque tâche portent aussi ses pièges dans `CLAUDE.md`. Les tâches de `mementum-server` (S5, S6, S7, M2) ont été menées en file ; C1 et S7, puis C2 et S7, de front. Il n'y a pas de commit de fusion : chaque atelier a été reporté sur `main` sans fusion, rebasé au besoin.

### Ce que chaque tâche a appris, en une ligne

- **S0** : un vrai Claude Code (2.1.329) parle la 2026-07-28 et retombe seul sur `initialize` devant un serveur de la seule 2025-11-25 ; forcé en 2025-11-25, il ne se branche pas sur un serveur de la seule 2026-07-28 : les deux révisions restent (R1250). L'écriture par différence n'a besoin d'aucun seuil (R1254) ; la version d'avant un agent, tenue par l'acteur, ferait attendre une frappe 150 à 300 ms : elle sort de l'acteur (R1257).
- **M1** : une sortie d'outil n'est que du texte, refus compris : un `structuredContent` aurait porté le contenu des pages hors de leur borne, et Claude Code ne passerait au modèle que lui (R1412) ; 56 Kio au plus par sortie (R1414).
- **S2** : une frappe dans le texte que l'écriture remplace par préfixe et suffixe n'est pas perdue, poussée au bord du texte écrit (R1283) ; des milliers de blocs retirés croissent plus vite que la page, dans `vendor/yrs`, où l'on ne touche à rien (R1284, R1294).
- **S3** : un alias se résout par un début de hachage, sans table (R1224, R1302) ; la fusion à trois de `replace_page` juge contenu et place à part (R1304) ; la seconde commande d'une opération destructrice refait le plan avec la base de l'agent, jamais celle de l'état encodé (R1562).
- **S1** : un jeton mort reste lisible quatre-vingt-dix jours, et la règle est dans les lectures, pas seulement dans la purge (R1270) ; « écrite par d'autres » compte un autre jeton (R1278).
- **S4** : `PUT /mmd` et la restauration passent par l'écriture par différence ; une écriture qui ne change rien n'écrit rien (R1331) ; l'acteur ne relit plus la page après une écriture d'agent, qui tombe de 110 à 75 ms sur 10 000 blocs (R1337).
- **S5** : la porte des jetons est une couche de route, pas l'extracteur seul, pour fermer aussi les routes qui ne lisent aucun compte (R1340) ; la restriction est gardée sous la forme du jeton, pas son identifiant (R1347, R1354) ; un appel abandonné par son client laisse sa ligne (R1355).
- **S6** : une portée qu'on ne voit pas est une seule réponse, octet pour octet (R1367) ; la création n'est liée au mot de passe vérifié que dans sa transaction (R1370, R120).
- **C1** : la commande prête à coller dépend du shell, et les gestes doivent venir dans l'ordre où le presse-papiers les sert (R1462) ; le jeton montré, Précédent est retenu par le routeur (R1460).
- **C2** : `agentAuthors` est tiré des auteurs par la route, si bien qu'il en est une part par construction (R1470) ; rien ne bouge sous le pointeur quand l'activité arrive (R1477).
- **S7** : une tranche se borne par dichotomie, pas un rendu par bloc retiré (R1401, le constat bloquant de sa relecture) ; une lecture ne reprend qu'à un bloc qui a son alias, sans coupe silencieuse (R1402) ; un filtre ne lit l'auteur et la date que des pages que le compte voit (R1603).
- **M2** : un refus du débit sur `/mcp` est un refus d'outil, que le modèle lit, tant que le corps est petit (R1421, R1434) ; une citation de l'entrée de l'agent va entre les bornes (R1439).
- **X1** : `replace_block` garde la frappe concurrente dans le premier bloc de premier niveau de son type du MMD neuf, et ne la garde pas sans lui (R1629, qui récrit LA-a) ; un croisement se fabrique en retenant les trames du serveur, jamais par une course (R1482).
- **X2** : le débit de R1241 tient le budget de la frappe pour des opérations sur un bloc, pas pour des écritures qui changent toute une grande page (R1644, R1660) ; une page de 10 000 blocs fige l'onglet 0,7 s à chaque changement distant, défaut d'avant le lot (R1656, R1661).

## Mesures clés

Sur le poste de développement (Windows 11, 24 cœurs, disque NVMe), en release, une mesure à la fois, souvent pendant qu'un autre agent compilait ; chiffres de la CI sous Linux quand ils existent (job `budgets`, mesures de S4).

| Mesure | Valeur | Source |
|---|---|---|
| Écriture par différence d'un bloc sur 10 000 paragraphes, contre la récriture complète | 51 ms contre 636 ms ; tous les textes changés, 91 ms | R1284, S2 |
| Écriture d'une opération d'agent sur 10 000 blocs, acteur et appel du hub | 73 à 79 ms au p50 et au p95 (CI : 61 et 69) | R1337, S4 |
| Opération destructrice avec sa version `before_agent` (10 000 blocs) | 242 à 263 ms l'appel ; les frappes d'un autre pendant, 12,5 ms au p95, 78 au pire | R1337, S4 |
| Lecture complète d'une page de 10 000 blocs par tranches de 48 Kio | 13 tranches, une seule demande à l'acteur, 323 ms ; une tranche gardée 10 ms | R1403, S7 |
| Une page par jeton dans la portée, 100 000 pages, avec le cache de la restriction | 0,14 ms au p50 (2,64 sans cache ; 0,09 par session) ; la lecture d'un jeton 0,07 ms | R1347, S5 |
| Recherche filtrée par la route, 10 000 pages | 62,6 à 62,9 ms au p95 (plancher de 50 ms compris) | R1642, X2 |
| Recherche filtrée, 100 000 pages, compte qui voit tout | 217 à 258 ms au p95 (hors budget, posé à 10 000) | R1611, R1642 |
| Frappe vue par un autre pendant 120 `str_replace` d'un agent (10 000 blocs) | 65,6 ms au p95, 99 au pire | R1655, X2 |
| Frappe pendant 60 `replace_block` qui coupent chacun une version | 63,4 ms au p95, 128 au pire | X2 |
| Frappe pendant qu'un agent crée 2 400 pages, puis pendant leur indexation | 10,9 puis 5,4 ms au p95 | X2 |
| Frappe pendant que trois jetons du compte écrivent au débit du compte | 78,9 ms au p95, 104,7 au pire | X2 |
| Appel d'un agent vu dans le navigateur, page de 1 000 blocs (×1 ; ×1,5) | 27 à 28 ms au p95 ; 31 ms | R1643 |
| …page de 10 000 blocs | 830 ms au p50 (l'onglet figé 0,7 s) | R1656 |
| Frappe pendant un `str_replace` en `replace_all` sur 10 000 paragraphes | 198 ms au p50, 1,06 s au p95 | R1660 |
| Mémoire en propre au repos après 500 lectures d'agent, acteurs arrêtés (Windows) | 116 Mo (36 Mo sans les lectures) ; 461 Mo 2 s après la dernière lecture | R1645, R1657 |
| Démarrage jusqu'à `/health` : dossier neuf ; index de 10 000 pages | 76 ms ; 25 ms | X2 |
| Un appel refusé par le débit | moins de 0,2 ms, corps de 64 Kio lu compris | R1651 |
| L'activité d'un jeton au journal plein ; un paquet du journal (256 lignes) | 1,2 ms ; 5,5 ms au p50 | R1648, R1649 |
| Claude Code en mode non interactif (X1, deux lancements, modèle haiku) | 0,012 $ | X1 |

## Défauts trouvés par les relectures, et corrigés

Chaque tâche a eu une relecture indépendante, puis un tour de correction, sans re-relecture. Les constats qui comptent :

- **S2** : `diff_cost` n'était pas une borne (de 2 à 7 fois sous le temps écrit) ; recalibré par le nombre d'éléments Yjs de la page (R1294), et l'essai hostile mêle désormais des écritures du serveur (R1299).
- **S3** : trois constats importants perdaient ou dupliquaient la suppression d'un humain en `rebase` (R1318, R1319) ; un plan pouvait s'écrire sur une autre révision que celle qu'il avait lue (R1562).
- **S1** : la coupe d'avant agent oubliait « un autre jeton » (R1278).
- **S4** : une page née avec son contenu ne coupait pas de version avant son premier remplacement (R1601) ; une frappe dans l'ancre d'une insertion faisait recouper la version (R1336) ; l'écriture d'agent tenait l'acteur 110 ms (R1337).
- **S5** (relecture de sécurité, aucun bloquant) : un appel par jeton abandonné par son client ne laissait aucune ligne (R1355) ; vingt jetons d'un compte pouvaient vider le cache des accès de l'instance (R1354) ; la restauration passait sans l'option corbeille (R1345 récrit).
- **S6** : un corps perdu après un changement de mot de passe réussi était dit en échec (R1375) ; la révocation abandonnée par son client n'allait pas au bout (R1376).
- **C1** : copier le jeton puis une commande faisait coller la commande à la place du jeton (R1462) ; Précédent emportait le jeton montré (R1460).
- **C2** : la phrase des comptes partait pendant une relecture et le filtre remontait sous le pointeur (R1476, R1477).
- **S7** (un bloquant) : une liste numérotée de 12 000 éléments rendait chaque tranche quadratique, en partie sur un fil du moteur (R1401) ; une tranche pouvait perdre des blocs sans le dire (R1402) ; la durée d'une recherche filtrée disait combien de pages non vues passaient le filtre (R1603) ; un fil ouvert pendant la version d'un agent ne protégeait pas son bloc (R1604) ; une connexion ouverte pendant cette version ne faisait pas refuser `replace_page` (R1605).
- **M2** (douze points) : un appel refusé par le débit faisait lire 2 Mio par refus (R1434) ; un corps de plus de 2 Mio recevait un `413` hors JSON-RPC (R1437) ; une citation de l'entrée de l'agent pouvait sortir de la borne (R1439) ; `/mcp/readonly` n'avait qu'une garde (R1443).
- **X1** : la règle de `replace_block` du plan (LA-a) était fausse (R1629) ; le parcours T5 comptait les croisements à l'heure (R1630).
- **X2** : la mémoire des pages ouvertes par un agent (R1645, R1657) ; la révocation passait devant les écritures déjà en file de son jeton (R1658) ; le budget de la frappe ne vaut que pour des opérations sur un bloc (R1660).

## Rouges de la CI pendant le lot

- La graine 1041 de l'essai hostile, rouge une fois sur trois sous Linux et Windows (`32c3fd8`) : la cause était l'essai, qui tirait au hasard le client de ses pages ; corrigé (R1561).
- La limite connue d'un `403 token_not_allowed` perdu avant le corps de `PUT /mmd` (`e0c17a6`) : levée sous Linux, dont le noyau garde assez d'octets ; le test est restreint à Windows (`c70d197`).
- `cli_set_password_recovers_account` attendait `403` de `GET /me` par jeton, que S7 a ouvert (`98b66af`, `8679e72`) : corrigé (`0bf8dc0`).
- `the_query_is_never_logged_and_never_in_an_address`, rouge sous Linux deux passages sur trois (`0bf8dc0`, `3a8be51`) : seul dans son binaire (`aad795e`), le piège R266.
- À rejouer à la passe de fin, hors des ajouts du lot : `e2e/attr-guard-paste.spec.ts` « rich-blocks » et `e2e/collab.spec.ts` « two_browsers_converge » (`c70d197`, verts au passage suivant) ; `e2e/sharing.spec.ts` « general_access_makes_a_private_page_a_space_page », déjà noté instable.

## Ce qui reste pour la fin du lot

Rassemblé des sections « À reprendre à la fin du lot » du registre ; la passe de fin les joue une fois, avec tout ce qu'elle lance (« Fin du lot » du plan).

1. **Essais au hasard larges** : l'essai hostile large (`MEMENTUM_FUZZ_SEEDS=45000 … hostile_at_random`), dû dans tous les cas depuis que S2 y a mêlé les écritures du serveur (R1299), qui dira si les retraits par identifiant de R1281 (seule mise à jour donnée à yrs hors de `PageDoc::read`) y entrent, et qui couvre la préparation changée de R1337 ; `Op::AgentOps` dans `reload` ; les opérations des agents et l'écriture par différence sous `twins::watching`.
2. **Mutations** : les listes « Preuves (mutations) » de chaque tâche, dont celles que S2, S3 et M1 n'ont pas jouées, les quatre de S4 sans mutation, le rouge pour sa raison de deux tests de S3 et de plusieurs tests vus seulement verts (R1277, R1353, R1374, R1397, R1458).
3. **Tests à écrire** : les quatre de R1398 (S7) ; un test de route de `limit` ramené à 1 024 (M2) ; la borne de dix secondes de `the_third_write_of_an_account_by_token_is_busy`.
4. **Mesures** : le pire repli de `write_diff` sur une page de 12 000 blocs (R1324) ; le coût de `Store::children` ; la ligne `MESURE` de la mémoire sous Linux, puis la décision (R1659) ; `diff_cost` qui sous-estime une page gardant des blocs supprimés (R1294) ; `format` de yrs pour un changement de marques seules (R1295, non mesuré).
5. **Avec le vrai client** : les limites de Claude Code sur une longue sortie (R1662 : déclarer `anthropic/maxResultSizeChars` sur `fetch`, baisser la borne d'une lecture, ou borner un refus) et les jetons des sorties (R1654) ; le cas d'Alex de bout en bout avec un vrai Claude Code et un navigateur ouvert (point 4 de « Fin du lot ») ; `claude mcp add` avec Alex, shell par shell (R1464, R1488, R1576).
6. **Défauts et douteux** : une écriture par jeton admise avant une révocation et validée après elle, hors des opérations (R1658 : relire le jeton dans toute transaction faite par jeton) ; le renommage d'`update_page` qui attend son tour d'écriture en tenant le verrou de la page (point 11 de X2) ; le focus sur `body` à l'arrivée sur « Réglages de l'espace » depuis le menu au clavier (R1621) ; les mentions `proven_by` « la spécification seule (M2 et X1 l'éprouvent) » de `spec/mcp/messages.json`, que X1 n'a pas jouées (R1487) ; l'e2e `recents.spec.ts`, récrit par S6 sans être joué ; l'écran des jetons qui ne dit pas le risque d'une page piégée et propose « Tous mes espaces » d'abord (R1500, à trancher par Lenny) ; un `from` ambigu de `fetch`, qui répond `stale_anchor` là où une opération répond `ambiguous_anchor` (R1505 : aligner le code, puis la spécification).
7. **`cargo deny check`** sur les crates neuves de M1 (`schemars` et ses dépendances, licences lues à la main, R1418).
8. **Essai de migration** sur des copies : une base du lot 4 passe `0006`, avec sa copie `avant-0006.bak` ; ses pages ont leur date, lue au journal ; ses versions, des trois raisons, sont toutes là ; la durée de `0006` et la taille du fichier avant et après (le coût disque de la reconstruction de `doc_versions`, supposé au README) ; une base du lot 3 passe `0005` puis `0006` dans la même ouverture.
9. **Au-delà du lot** : une tâche dédiée pour l'onglet figé sur une page de 10 000 blocs (LA-q, R1661 : deux à trois jours, risque moyen à élevé, dans le correctif de y-prosemirror) ; au lot 7, un plafond des acteurs sans connexion pour l'instance (R1657) et un débit pondéré par le temps d'écriture (R1660).

## La méthode suivie, et ce qu'elle a donné

La méthode de Lenny du 20 novembre (R935) : chaque tâche n'écrit et ne lance que les tests de son ajout et les suites qu'elle touche, a une seule relecture, sans re-relecture, et laisse au registre ce qu'elle ne corrige pas ; la passe complète vient une fois, à la fin du lot. Ce que ce lot en a appris :

- **Le test vu rouge d'abord a souvent été remplacé par le test vu rouge sous une mutation de sa règle** (S1, S5, S6, S7, C1 : R1277, R1353, R1374, R1397, R1458) : le code écrit d'abord, puis chaque test rougi par un interrupteur posé le temps d'un passage, une règle à la fois. C'est plus faible pour ce qu'un test ne nomme pas : les tests vus seulement verts se jugent aux mutations de la fin du lot.
- **Deux ateliers ne partagent pas un dossier de compilation** (R1334) : cargo range un crate sous un hachage de son chemin relatif à la racine de l'atelier, le même dans deux ateliers ; S4 a compilé contre le `mementum-acl` de S5. Tout ce qui avait passé dans ce créneau a été relancé dans un dossier à soi.
- **Les relectures ont trouvé ce que les tests de la tâche ne voyaient pas** : un rendu quadratique (S7), une fuite par le temps de réponse (S7, R1603), une écriture sans trace (S5, R1355), une règle du plan fausse dans le code (X1, R1629), la mémoire des pages ouvertes (X2). La passe de fin cherchera ce que les relectures de tâche n'ont pas vu.
- **Un vrai Claude Code a été lancé douze fois au plus** (S0 neuf fois, M2 une, X1 deux), toujours en mode non interactif, avec une configuration à part, jamais par `claude mcp add` (R1258) : ce que `claude mcp add` écrit ne se voit que par Alex.
