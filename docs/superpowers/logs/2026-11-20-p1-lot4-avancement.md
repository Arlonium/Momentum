# Phase 1, lot 4 « Recherche » : avancement

Écrit le 20 novembre 2026, par la Task D, quand le code des seize tâches est sur `main` (`6f5ae38`). Plan : [`2026-11-19-p1-lot4-recherche.md`](../plans/2026-11-19-p1-lot4-recherche.md) ; registre : [`2026-11-19-p1-lot4-ledger.md`](2026-11-19-p1-lot4-ledger.md) (R921 à R1174 ; R900 à R920 sont dans le plan) ; recette du propriétaire : [`2026-11-20-p1-lot4-recette.md`](../../recette/2026-11-20-p1-lot4-recette.md).

## Où en est le lot

- **Le code est fini sur `main`** : seize tâches (S0 à S7, C1 à C3, X1, X2, et D pour la documentation), de `97d3847` (le plan, le 19 novembre) à `6f5ae38` (la Task X2, le 20 novembre à 11 h 08). Chaque tâche a eu une relecture, une seule, et ses constats ont été corrigés une fois (méthode de R935, plus bas). Tout a été poussé au fil du travail (R934).
- **La passe complète de fin de lot est en cours** (un autre agent, sur ce poste, le 20 novembre) : toutes les suites sur un clone neuf, étape par étape comme la CI, les mesures, les mutations que chaque tâche a listées, la relecture finale en trois passes (sécurité, intégration, documentation contre le code) et l'essai de migration d'une base du lot 3. Elle ne corrige rien : elle remplit une liste de défauts au registre. Viennent ensuite la correction groupée, une passe de contrôle, puis la recette du propriétaire. **Rien de ce document ne dit que la passe est faite.**
- **Les dix-neuf décisions du plan** (L1 à L19) sont retenues par défaut depuis le 20 novembre (Lenny : « tu es autonome »), sauf L12 (pousser : sa réponse, R934) et L1 (Tantivy, retenu sur les mesures de S0, R933). Elles sont listées, avec les choix pris en route, en tête de la recette.
- **La migration `0005`** peut changer jusqu'à la clôture du lot. Elle a changé trois fois (`8600ad9`, `6f093e8`, `865338c`) : aucun binaire du lot n'ouvre une instance que quelqu'un regarde.

## Les tâches

| Tâche | Ce qui est livré | Commits (code ; clôture sur `main`) | Rulings |
|---|---|---|---|
| S0 | Tantivy mesuré contre FTS5, sur un banc jetable (`spike/search-bench/`) ; verdict : Tantivy tient | `2225a2b`, `ce60c21` ; `a4b636b`, `f63a9a2` | R921 à R933 |
| S1 | migration `0005` (`blocks_index`, `page_links`, `search_state`, `search_outbox`), voie `Lane::Index`, marqueurs durables, fonctions du store | `8600ad9`, `6f093e8` ; `064808e` | R936 à R939, R961 à R964, R975 à R978 |
| S2 | crate `mementum-search` : la projection d'une page, la lecture d'une requête, le pliage des mots ; tables de cas `spec/search/` | `70157c6`, `afefc30` ; `0983893` | R940 à R949, R960 |
| S3 | l'index plein texte Tantivy derrière `SearchIndex` : note sans statistique, groupement par page, filtre dans le collecteur, débuts de mot, jeton de validation | `6a1bf54`, `156a0a7`, `b2fdc59` ; `39ed4b8` | R965 à R973, R990 à R992 |
| S4a | `mementum-sync` : les pages écrites (`Hub::take_touched`) et la lecture seule d'une page gardée (`stored_page`) | `1bc4fb1`, `c4d3a47`, `878a04f` ; `63fd2a2` | R950 à R956 |
| S4b | l'indexeur, tâche de fond, jusqu'aux tables de la base | `6210ee5`, `91e6afa`, `0d7258a`, `83cf7eb` ; `cf42861` | R980 à R988, R995 à R999 |
| S6 | `GET /api/v1/pages/{id}/backlinks` | `865338c`, `96ed769`, `b68c06f`, `b520659`, `b69cb69`, `0a79925` ; `5529097` | R1015 à R1023 |
| S4c | l'index plein texte nourri par intention, validation, clôture ; la comparaison des jetons et la reconstruction au démarrage | `8657a64`, `974d0bc`, `1eaa426`, `addbd63`, `e1c74e2`, `e7dedce` ; `fbff3a1` | R1000 à R1014, R1025 à R1030 |
| S7 | `mementum reindex [--text-only]` | `20b6f24`, `7b8fefc`, `ac1485d` ; `3217438` | R1060 à R1072 |
| S5 | `POST /api/v1/spaces/{id}/search`, son débit et son plancher | `10d1201`, `90a2834`, `291176c`, `968235f` ; `030269b` | R1040 à R1059 |
| C1 | la fenêtre de recherche (« Rechercher », Ctrl+K) | `3fd2e73`, `9a5a205`, `0d12e47`, `ee7c3ea` ; `9cb9c5f` | R1080 à R1099 |
| C2 | l'arrivée sur le bloc trouvé (`#b=`) | `ebf8348`, `befb11d`, `6478121`, `9800fc6`, `2234e5c` ; `faaa19f` | R1100 à R1114, R1130 à R1133 |
| C3 | « Pages qui mènent ici » sous la page, branché sur C2 | `20739cc`, `97cad4f`, `dc01e63`, `9e45623`, `a331330`, `96e1460`, `f5c234d` ; `463667d` | R1115 à R1129, R1134 à R1137 |
| X1 | les droits de la recherche et des rétroliens, de bout en bout, dans Chromium | `1bea008`, `eef1526`, `272bc1f`, `7bd23d1` ; `f9d74d8` | R1140 à R1148 |
| X2 | les budgets : mesures, budgets de la recherche au §5.10, mesures bornées dans la CI | `30553a3`, `0a858a0`, `eb2466d`, `1d4ca0b`, `ca3d782`, `c5131ea` ; `6f5ae38` | R1160 à R1174 |
| D | cette documentation, la recette, la conception, le README, `docs/REPRISE.md` | atelier `lot4-d`, non poussé | R1190 à R1194 |

Il n'y a pas de commit de fusion : de `97d3847` à `6f5ae38`, `main` compte 104 commits, tous linéaires : chaque atelier a été reporté sur `main` sans fusion. La colonne « clôture » donne le commit qui porte au registre la relecture de la tâche (ou, pour S4a, X1 et X2, son registre et ses pièges).

### Ce que chaque tâche a appris, en une ligne

- **S0** : Tantivy 0.27.0 ouvre son index et son écrivain en 2,4 à 2,8 ms, ajoute 4 Mo à la mémoire au repos, et répond en 42,6 ms au pire p95 sur 1,29 million d'entrées (index au repos) ; il est 1,5 à 7 fois plus rapide qu'un FTS5 bien écrit, et tient par son API publique les trois exigences du plan (note sans statistique, groupement par page, charge utile d'une validation) (R921 à R933).
- **S1** : la clé de `blocks_index` reste deux identifiants en texte (1,77 fois le texte, R937) ; l'outbox n'a pas de `gen` : toute écriture d'une ligne lui retire son jeton (R939).
- **S2** : le découpage et le pliage des mots sont écrits une fois, et servent à l'index comme à la requête (R942) ; une page pleine (8 Mio) se projette en 16,5 ms, 118 ms en accents combinants, le pire trouvé (R946).
- **S3** : chaque note de mot posée sur une grille (R965), sans quoi l'ordre de deux notes égales dépendait de l'ordre d'addition du moteur ; le titre gardé ne pèse qu'un dixième du titre brut, les colonnes rapides 16 Mo par million d'entrées, et ne plus garder le texte rendrait 23 % de l'index (R972) ; lire une entrée non permise coûte 38 ns, plus du double de ce que S0 disait.
- **S4a** : l'acteur d'une page ne fait que la nommer dans un ensemble ; aucune liste de blocs, aucun encodage de plus (R953).
- **S4b** : une transaction de l'index de 256 Kio au plus (R981 : la frappe d'une autre page attend 11 ms au p95 pendant, contre 2 à 3 ms seule) ; 10 000 pages de dix blocs projetées en 26 s.
- **S6** : les sources d'un rétrolien lues dans l'espace de la cible, une entrée par source (R1015, R1016 : 4 576 pas de SQLite contre 76 pour trois cents liens), et triées par droits avant d'être comptées.
- **S4c** : la base et l'index tenus d'accord par un jeton par validation ; 10 000 pages de dix blocs indexées et validées en 33,7 s ; la base grossit de 26,9 Mo et `search/` pèse 7,4 Mo pour 5,9 Mo de texte ; le serveur démarre en 15 ms sur un index de 10 000 pages, jetons comparés en fond en 109 ms (R1009).
- **S7** : la commande prend le verrou de l'index avant d'ouvrir la base (R1060), et réutilise les fonctions de l'indexeur (R1061).
- **S5** : l'ensemble des pages permises se bâtit en 3,2 ms au p95 pour 100 000 pages : pas de cache (R1048) ; huit mots que 200 000 entrées portent coûtent 54 ms de calcul, au-delà du plancher (R1051) ; la route répond en 64 ms au p95, plancher compris.
- **C1** : une coquille non différée tient le champ le temps que le code de la fenêtre arrive (R1095) ; la sélection de l'éditeur est rendue avec le focus (R1088) ; rien ne bouge sous le pointeur (R1080 à R1082).
- **C2** : le fragment `#b=` gardé par le routeur, une visée suivie une fois, après la première synchro (R1100 à R1106) ; le texte propre du bloc amené au milieu (R1108, R1113).
- **C3** : la lecture des rétroliens jamais sur le chemin de l'ouverture (R1116) ; une page visée par 200 pages s'ouvre en 142 ms au p95 (R1122).
- **X1** : aucun défaut de droits trouvé dans un navigateur, à trois comptes : aucun compte ne trouve, ne lit en extrait, en titre, en fil d'Ariane ni en rétrolien ce qu'il ne lit pas.
- **X2** : les budgets d'avant tiennent avec l'index ; ceux de la recherche sont adoptés tels que mesurés (R1163, R1165, R1173) ; le plancher des rétroliens monte de 75 à 150 ms et leur parcours descend de 20 000 à 10 000 sources (R1167).

## Mesures clés

Sur le poste de développement (Windows 11, 24 cœurs, 32 Gio, disque rapide), en release, une mesure à la fois, souvent pendant qu'un autre agent compilait. **Rien n'a été mesuré sous Linux ni sur les quatre cœurs de la CI** (R1174) : la CI de la poussée de `c5131ea` lancera les mesures bornées (R1171).

| Mesure | Valeur | Source |
|---|---|---|
| Démarrage à froid, index à jour de 10 000 pages (et d'un million de blocs) | 24 ms (25 ms) | R1160 |
| Mémoire au repos, même dossier | 18,4 Mo de mémoire de travail, dont 10,7 Mo en propre (19,6 et 11,9 Mo) | R1160 |
| Mémoire en propre après 500 recherches | 17,7 Mo (28,4 Mo au million de blocs) | R1160 |
| Pointe en propre d'une récriture de l'index | 74 Mo (190 Mo au million de blocs ; 115 Mo avec un tampon de 15 Mo) | R1160, R1164 |
| Frappe vue par un autre pendant toute une première indexation de 10 000 pages | 6,0 ms au p95, 108 ms au pire (3 882 frappes) ; indexation en 47,9 s | R1161 |
| Frappe vue pendant qu'une page de 10 000 blocs est récrite et renommée en boucle | 5,8 ms au p95, 98 ms au pire | R1162 |
| Mot tapé trouvable | 2,12 à 2,20 s après la dernière frappe | R1173 |
| Recherche, route entière, plancher compris, 10 000 pages et 100 000 blocs | 62,8 ms au p95 (sous Windows, le plancher de 50 ms répond en 62 ms) ; 64,9 ms pendant que l'index est récrit | R1163 |
| La même, un million de blocs | mot rare 62,8 ms ; un mot que toutes les entrées portent 86,7 ms ; huit tels mots 274 ms (447 ms pendant que l'index est récrit) | R1163 |
| Calcul seul, huit mots, un million d'entrées qui les portent | 269 ms (membre qui voit tout), 222 ms (invitée qui voit dix pages) | R1166 |
| De la dernière touche au résultat affiché (build de production) | 224 ms au p95 ; 299 ms sous un processeur ralenti de moitié | R1165 |
| Rétroliens à la borne du parcours (10 000 sources non vues) | 23,9 ms au p95 ; 200 sources vues à blocs de 64 Kio : 56 à 62 ms | R1167 |
| Une page visée par 200 pages, ouverte | 140 ms au p95 (212 ms sous ×1,5) | X2 |
| RO-1 : 100 Mio de mots de dix lettres tous différents | 77 s d'indexation, fusions finies à 166 s, 276 Mio de tas, 979 Mio de mémoire de travail, index de 667 Mio | R1169 |
| `blocks_index`, un million de lignes de 80 octets | 150,5 Mio pour 77,2 Mio de texte (1,95 fois) | R1170 |

## Défauts trouvés par les relectures, et corrigés

Chaque relecture a conclu « prêt après corrections ». Aucune n'a trouvé de fuite de page. Les constats importants :

- **S1** : deux indexations de front laissaient un index faux noté à jour (I1 → R975, un verrou de l'indexeur pris avant de lire) ; le retard d'un espace se lisait en parcourant ses pages (I2 → R964).
- **S2** : le pliage retirait toute marque combinante, et confondait des mots du hindi, de l'arabe ou du thaï (I1 → R947) ; une mention collée à un mot les fondait en un seul (R948) ; une requête exigeait les lettres seules (« l », « d ») (R949) ; pas de version de la projection (R960).
- **S3** : un chemin mal joint pouvait faire vider `files/` et la base (I1 → R990) ; les candidats d'un préfixe long n'étaient pas bornés en mémoire (I3 → R991) ; rien ne rougissait si un préfixe parcourait le dictionnaire (I2 → R992).
- **S4b** : une panique ou une erreur durable était réessayée sans fin (I1 → R995) ; « à jour » dit pendant le démarrage (I2 → R996) ; un second arrêt n'attendait rien (I3 → R997) ; `MEMENTUM_INDEX_AFTER_MS` acceptait plus de quinze secondes (R999).
- **S6** : la durée d'une lecture disait combien de sources on ne voit pas (I1 → R1022, un plancher) ; le poids d'un lien était sous-estimé (M2 → R1023).
- **S4c** : un refus durable faisait un essai par seconde (I1 → R1027) ; la recherche ne rendait plus rien entre deux essais (I2 → R1028) ; des validations dos à dos (I3 → R1029) ; un arrêt brutal pendant le vidage empêchait de redémarrer (I4 → R1026) ; les fonctions de test étaient dans le binaire livré (M1 → R1014).
- **S7** : sans adresse publique, la commande retirait en silence des rétroliens (I1 → R1070) ; le remplissage de l'index sans fonction bornée ni test de R1001 (I3 → R1071).
- **S5** : la place de l'instance, rendue à la fin du calcul, laissait lire sa durée (I1 → R1054) ; `behind` lisait la base à chaque recherche (R1055).
- **C1** : le second clic d'un double clic agissait (R1094) ; une frappe entre Ctrl+K et l'arrivée du code partait dans la page (R1095) ; le focus d'avant mal rendu (R1096) ; une sélection relâchée hors du cadre fermait la fenêtre (R1097).
- **C2** : un bloc à longs sous-blocs n'était pas montré (I1 → R1113) ; une page ouverte à froid prenait le focus à qui tapait dans le titre (I2 → R1114) ; les liens de la table des matières (R1130).
- **C3** : un bloc visé parti laissait le focus sur `body` (R1134) ; le gel sous le pointeur ne voyait pas un pointeur laissé par une fenêtre qui se ferme (R1135) ; une ligne retirée quand la fenêtre du navigateur passait à l'arrière-plan (R1136).
- **X2** (par la mesure, sans relecture) : un compte pouvait pousser une lecture des rétroliens au ras du plancher de 75 ms, et lire au-delà le nombre de sources non vues (R1167).

## Incidents de la CI du 20 novembre

- **Une inclusion par chemin refusée sous Linux** (`0a79925`, `9e9f59f`, 08 h 43) : un `#[path]` dans un module en ligne de `src/routes/backlinks.rs` part d'un dossier qui n'existe pas ; Windows remonte ce dossier par `..` sans rien dire, Linux refuse ; rouge deux poussées de suite. Corrigé en déclarant le module au niveau du fichier ; piège écrit dans `CLAUDE.md` (Task S6).
- **L'e2e `page_link_opens_the_page_and_reads_back`** (lot 3), rouge trois fois sur la CI : il attend maintenant que le serveur ait reçu le retrait de la frappe du menu « / » (`0b161d4`, 09 h 13). Le registre le porte encore parmi les tests instables, sans noter ce correctif.
- **La ligne de l'arbre décalée par l'entrée « Rechercher »** : depuis que la barre latérale porte « Rechercher » sous le sélecteur d'espace, la ligne de la page mesurée par un budget du navigateur n'était plus rendue (l'arbre ne rend que les lignes proches de l'écran) ; le budget l'amène à l'écran avant le clic (`06c6a1a`, 09 h 41).
- **Des tests du lot 3 vus rouges une fois** (registre, « À reprendre à la fin du lot ») : `e2e/budgets.spec.ts` « open_1000_block_page_with_50_images_under_300ms » (19 et 20 novembre, sur des commits sans code produit), et `e2e/equations.spec.ts` « a_formula_nested_without_tables_never_crashes_the_tab__mathllap_deep_in_a_list » (délai d'une minute dépassé) ; un second test du 20 novembre n'est pas nommé. Pour la passe complète.

## Ce qui reste ouvert

**Pour la fin du lot** (registre, « À reprendre à la fin du lot », et ce que les tâches ont laissé) :

- les mutations des listes « Preuves » de chaque tâche, jamais jouées (R935) ; plusieurs tests de l'ajout n'ont jamais été vus rouges (dits par chaque tâche) ;
- la suite de bout en bout entière n'a jamais tourné avec un index après 200 ms de calme (R1091) ni avec la lecture des rétroliens à chaque ouverture de page (R1116) : premières pistes si un e2e de rafale ou de session perdue rougit ;
- Vitest entier, `cargo test --workspace`, les quatorze autres budgets du navigateur ensemble ;
- `cargo deny check advisories` rouge sur `libsqlite3-sys` 0.37.0 (RUSTSEC-2026-0459), qui n'est pas neuf : c'est l'avis que le lot 1 a déjà accepté (R44) ;
- `POST /api/v1/invitations/lookup` n'est pas dans `READING_POSTS` (R1092) ;
- le `404` d'une page non vue diffère en temps de celui d'une page inconnue (antérieur au lot, pour la passe de sécurité, R1022) ;
- Échap dans le texte de l'éditeur, sans menu, laisse le focus sur `body` (antérieur au lot, `test.fixme`, R1143) ;
- la CI de la première poussée de X2 (lignes `MESURE`, dont `RssAnon` sous Linux), et Vitest pour cinq fichiers du client dont un commentaire a changé (75 ms devenu 150 ms).

**Reporté au lot 7** :

- **RO-1** : un passage de l'indexeur borné en octets, avec un débit par compte de l'indexeur (L-g, R928, R972, R1169) ;
- l'indexeur unique et sériel : une page coûteuse à relire le tient pour toute l'instance (R997, R860) ;
- une borne de durée d'un calcul de recherche, par un collecteur qui regarde l'heure (R1045, R1172) ;
- `behind` qui lit toute l'outbox (100 ms pour 100 000 pages, une fois par seconde et par espace hors repos) (R1055, R1047) ;
- un verrou de toute l'instance, pris avant la base (R1013, R1069) ;
- une boucle de lectures des rétroliens d'un compte coûte la borne du parcours à chaque appel : un débit par compte (R1019) ;
- la place de l'index sur le disque, sans quota (L-g, comme R247) ;
- la relecture périodique de la session des connexions ouvertes et pendant l'attente du débit (R125, et S5) ;
- une forme plus courte de `blocks_index` (R1170).

**Non mesuré** : Linux et les quatre cœurs de la CI ; `RssAnon` ; ARM ; le chargement d'une page de 10 000 blocs pendant que l'indexeur la relit ; une vraie reconstruction depuis un index vidé (mesurée à la place : l'index récrit en entier) ; la durée de `mementum reindex` ; un coureur 1,5 à 1,7 fois plus lent (calculé) (R1174, S7).

## La méthode suivie, et ce qu'elle a donné

Le 20 novembre, Lenny a changé la méthode à partir de ce lot (R935) : une tâche n'écrit et ne lance que les tests de son ajout et les suites qu'elle touche ; une seule relecture, sans re-relecture ; ni suite complète, ni suite de bout en bout entière, ni essai au hasard large, ni mutations entre les tâches ; la CI de chaque poussée est lue, et un rouge hors de l'ajout se note pour la fin du lot ; à la fin, une passe complète unique, la liste des défauts, une correction groupée, une passe de contrôle.

Ce qu'elle a donné :

- **Le temps** : les quinze tâches de code ont été menées et relues en une journée (le 20 novembre, de la clôture de S0 à 11 h 08), là où une tâche du lot 3 pouvait demander plusieurs tours de relecture.
- **Les relectures uniques ont encore trouvé l'essentiel des défauts de leur tâche** (liste plus haut), dont plusieurs qui touchaient à la sûreté (un dossier vidé par erreur, une durée qui redisait ce qu'on ne voit pas, deux indexations de front). Aucune n'a demandé de second tour.
- **La CI lue à chaque poussée** a vu trois rouges, corrigés dans la journée ; deux tests du lot 3 instables sont notés, pas traités.
- **Le prix** : ce qui ne se voit qu'à l'échelle de toutes les suites n'est pas encore vu. Les mutations n'ont pas été jouées, plusieurs tests n'ont jamais été vus rouges, la suite de bout en bout entière n'a pas tourné avec les changements du lot, et rien n'a été mesuré sous Linux. C'est l'objet de la passe en cours ; ce document sera complété de ce qu'elle trouve.
