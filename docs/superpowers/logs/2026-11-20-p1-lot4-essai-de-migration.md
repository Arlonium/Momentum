# Phase 1, lot 4 « Recherche » : l'essai de migration (2026-11-20)

Point 3 de « Fin du lot » du [plan](../plans/2026-11-19-p1-lot4-recherche.md), et les cases d'exploitation de la [recette](../../recette/2026-11-20-p1-lot4-recette.md) (G1 à G4, G6, G7), joués avec de **vrais binaires**, en release, sur le poste de développement sous Windows : le serveur du lot 3 (`4f1d77e`, le dernier commit d'avant le lot 4) crée et remplit une instance par l'API ; le serveur du lot 4 (`main`, `ac8707f`) la reprend. Tout s'est fait dans un dossier jetable (jamais `data/instance`), sur les ports 8101 et 8102, une commande lourde à la fois.

**En bref : aucun défaut trouvé.** Le serveur du lot 4 écrit la copie d'avant migration, passe `0005`, démarre en une cinquantaine de millisecondes et indexe l'instance en fond en moins de 0,3 s ; tout ce qui existait est relu à l'identique (407 lectures par l'API, au nom de trois comptes, et le contenu de chaque table de la base) ; la recherche et les rétroliens rendent exactement ce que chaque compte doit voir (264 recherches et 66 lectures de rétroliens justes, à chacun de six démarrages du lot 4) ; l'index se refait seul quand `search/` manque ou ne correspond plus à la base ; `mementum reindex` refuse sans rien toucher pendant qu'un serveur tourne ; le binaire du lot 3 refuse proprement la base migrée, et la copie d'avant migration remise en place repasse `0005` et fait reconstruire l'index.

## Défauts

Aucun. Quatre remarques, qui ne sont pas des défauts :

- **M1. `reindex --text-only --public-url …` est refusé en anglais** (« error: the argument '--text-only' cannot be used with '--public-url <URL>' », code 2) : c'est le refus de clap, voulu par R1064 (« refusée avec `--text-only` (par clap) »). La recette (G3) dit « doit être refusé », sans dire la langue ; Lenny peut s'en étonner.
- **M2. La ligne du journal « base migrée ; la copie d'avant migration est à côté d'elle (kept : une copie identique, d'une tentative précédente, y était déjà) … kept=false »** : la parenthèse est la légende du champ `kept`, écrite même quand il est faux, et se lit comme « une copie identique y était déjà ». Héritée des Rulings R67 et R72 (lot 2), inchangée par le lot 4.
- **M3. La première indexation d'une instance migrée ne dit rien au journal** (R1001, voulu) : seul `indexing` des réponses de recherche la montre (vrai aux premières réponses, puis faux). Le README le dit (« Instance du lot 3 »).
- **M4. La copie `mementum.db.avant-0005.bak` n'est pas le fichier d'origine octet pour octet** (elle est écrite par SQLite : en-tête en mode rollback et non WAL, compteurs, version de SQLite, quelques octets de la page 1) ; son **contenu** l'est : schéma et dix-huit tables identiques, lus ligne à ligne. C'est ce que dit le README (« Copies avant migration ») ; la copie témoin remise en place est d'ailleurs reconnue comme la même (`kept=true`, étape 5b).

## L'instance du lot 3

Remplie en 2 s par un script (Node, `fetch`), serveur du lot 3 lancé avec `--signup open --version-every-secs 1`, puis arrêté proprement (Ctrl-C dans sa console : le journal dit « arrêt demandé », et aucun `-wal` ne reste).

- **Trois comptes** : Alice (premier compte, admin de l'instance) ; Bob, **membre** de l'espace d'Alice (invitation d'espace acceptée) ; Carol, **invitée** sur une seule page de cet espace, en lecture (invitation de page acceptée).
- **Quatre espaces** : A (celui d'Alice), A2 (« Atelier d'Alice », créé par elle, où Bob n'a aucun rôle), B (celui de Bob), C (celui de Carol).
- **Vingt-deux pages** : dix-sept dans A, dont une **privée** (p14 « Carnet secret »), une **sous-page** (p03, sous p02), une **à la corbeille** (p15 « Brouillon jeté »), une **partagée avec Carol** (p11) ; deux dans A2, deux dans B, une dans C.
- **Du texte par `PUT /mmd`**, relu tel qu'écrit pour les vingt-deux pages : titres, liste à puces et numérotée, citation, tableau, bloc de code, image. Un mot rare par page, avec accents, ligature et casse (« Bienvenüe », « ÉLÉPHANTEAU », « cendrillonnades », « piñata », « Œuvrière », « Kafkaïen », « brûléerie », « ÅNGSTRÖM », « Zéphyrine », « Quokkalune », « Hydrozoaire », « quadrilobée »…), et un mot commun à cinq pages de quatre espaces (« Transversalité » : p01 dans A, p14 privée dans A, p18 dans A2, p20 dans B, p22 dans C).
- **Des liens** : p06 « Index des liens » porte trois liens de page (`<page id>` vers p02, p04, p11), une mention de page (p05), un lien relatif (`/p/<p01>`) et un lien écrit avec l'adresse entière de l'instance (`http://127.0.0.1:8101/p/<p07>`) ; p13 mentionne p07 ; p14 (privée) mène à p02 ; p15 (corbeille) mène à p04 ; p18 (espace A2) mène à p02 et p20 (espace B) à p01, d'un autre espace.
- **Un fichier** (`pixel.png`, 70 octets) envoyé sur p12 et montré par un bloc image ; **un fil de commentaires** sur p12 (Bob), une réponse (Alice), une réaction (Bob) ; **un favori** (Alice, p02) ; **des versions** : une `periodic` de p02 (récrite 1,6 s après), et une `closing` par page écrite, à l'arrêt (vingt états de version lisibles par Alice).

## Déroulé

Chaque lecture « tout le contenu » est une relecture par l'API, au nom de chaque compte : `GET /api/v1/info`, `/me`, pour chaque espace où le compte a un rôle l'arbre, la corbeille, les favoris, les membres et les invitations, et pour chacune des vingt-deux pages la page, son MMD (empreinte SHA-256 des octets), ses commentaires, ses versions et l'empreinte de l'état de chacune, ses accès, plus le fichier (empreinte, type, `Content-Disposition`) : **407 lectures**, statut et corps compris (refus `403` et `404` comparés aussi). Deux relectures du lot 3 sur deux démarrages sont identiques entre elles (lire ne change rien) ; c'est la première qui sert de référence.

Chaque « recherche juste » compare l'ensemble des pages rendues à un modèle : les pages de l'espace cherché que le compte lit, hors corbeille, dont le titre ou un bloc porte tous les mots cherchés (le dernier aussi en début de mot), le texte plié comme le serveur le plie (NFC, accents latins retirés, minuscules) ; `more` doit être faux. Quarante-trois recherches pour chacun des six couples compte et espace (A, A2 pour Alice ; A, B pour Bob ; A, C pour Carol), dont des variantes de casse et d'accents, des débuts de mot (« elephant », « cendrillonnade »), des mots de titre (« glossaire », « secret », « carnet », « jete », « absolue »), le titre d'une page copié dans le lien qui y mène (« phenix » trouve p02 et p06, L14), deux mots d'un même bloc (trouvés) et de deux blocs différents (rien) ; plus six recherches dans un espace où le compte n'a aucun rôle (`404`) : **264**. Chaque « rétrolien juste » compare, pour chaque compte et chacune des vingt-deux pages, la réponse au modèle (sources de l'espace de la page, lues par le compte, hors corbeille ; `404` pour une page qu'il ne lit pas) : **66**.

| Étape | Attendu | Vu |
|---|---|---|
| 1. Ateliers et binaires | deux ateliers, deux binaires en release | `essai-lot3` à `4f1d77e`, `essai-lot4` à `ac8707f` (détaché, `main` étant pris par le dépôt principal) ; 58 s et 78 s de compilation |
| 2. Remplir avec le lot 3 | l'instance décrite plus haut | faite ; les vingt-deux MMD relus identiques à ce qui a été écrit ; arrêt propre |
| 3a. Lot 4 sur les données | dire la copie d'avant migration, migrer, démarrer, indexer en fond | « base migrée ; la copie d'avant migration est à côté d'elle … `mementum.db.avant-0005.bak` bytes=266240 kept=false », puis « serveur prêt » 16 ms après ; `_sqlx_migrations` porte 1 à 5 ; `search/` créé |
| 3b. Tout le contenu d'avant | identique | 407 lectures sur 407 identiques à celles du lot 3 (pages, MMD octet pour octet, arbres, corbeilles, favoris, membres, invitations, accès, commentaires, versions et leurs états, fichier) ; dans la base, schéma mis à part, les tables du lot 3 ont le même contenu ligne à ligne, hors `sessions` (les connexions de l'essai) et `instance_settings` (deux clés neuves, `search.projection` et `search.committed`) |
| 3c. Indexation et recherche | chaque mot trouvé par qui lit la page, par personne d'autre (privée, corbeille, autre espace) ; `indexing` finit faux | 264 recherches justes sur 264, 88 pages rendues en tout ; `indexing` faux pour les six couples dès la première réponse (+0,6 s) ; mesuré à part : vrai aux premières réponses, faux dès +0,26 s |
| 3d. Rétroliens | les sources vues, de l'espace de la page, hors corbeille | 66 justes sur 66 : p02 ← p06, p14 pour Alice, p06 seule pour Bob ; p04 ← p06 (p15, à la corbeille, absente) ; p07 ← p13 (le lien à l'adresse entière ne compte pas sans adresse publique) ; p11 ← p06 pour Alice et Bob, rien pour Carol ; les liens de A2 et de B vers A absents ; `404` pour toute page que le compte ne lit pas |
| 3e. La projection | une ligne `search_state` par page, indexée | `blocks_index` 39 lignes, `page_links` 10 (exactement les dix liens de même espace), `search_state` 22, toutes indexées, aucune échouée, `search_outbox` vide |
| 4a. `search/` supprimé, relance | l'index refait seul, la recherche retrouve tout | « l'index de recherche n'est pas celui de cette base (…) : il est reconstruit, en fond » ; 264 et 66 justes |
| 4b. `reindex` pendant que le serveur tourne | refus, rien de modifié | code 1 : « l'index de recherche est déjà ouvert par un autre processus (…) Rien n'a été modifié, ni la base ni l'index » ; `--text-only` de même ; les quinze fichiers du dossier des données (base, `-wal`, `-shm`, copie, fichier, index), lus en partage pendant que le serveur les tenait, ont le même contenu et la même date avant et après ; la recherche répond toujours |
| 4c. `reindex`, puis `--text-only`, serveur arrêté | les deux résumés de la recette, l'avertissement sans adresse publique | « réindexation finie (sans adresse publique) : 22 pages relues depuis leur document et projetées, 61 entrées écrites dans l'index de recherche (1 validations) », suivi de l'avertissement ; « index de recherche refait depuis les tables, sans relire aucun document : 61 entrées (…) » ; `--text-only --public-url` refusé (M1) |
| 4d. Relance après la réindexation | rien de reconstruit, tout se trouve | aucune ligne de reconstruction au journal ; `indexing` faux dès la première réponse ; 264 et 66 justes |
| 4e. `reindex --public-url http://127.0.0.1:8101`, serveur relancé avec la même adresse | le lien écrit avec l'adresse entière devient un rétrolien | « (adresse publique : http://127.0.0.1:8101) », sans avertissement ; p07 ← p06, p13 pour Alice et Bob ; 264 et 66 justes contre le modèle avec adresse publique |
| 4f. `reindex` sur une copie neuve de la base du lot 3, jamais servie par le lot 4 | copier, migrer, réindexer ; le serveur ensuite ne reconstruit rien | « base migrée ; la copie d'avant migration est … `avant-0005.bak` (266240 octets) », puis la réindexation (235 ms en tout) ; serveur relancé : à jour dès la première réponse |
| 5a. Lot 3 sur la base migrée | refus propre (base plus récente), rien d'abîmé | code 1 : « la base porte la migration 0005, que ce binaire ne connaît pas : une version plus récente de Mementum l'a appliquée. La base n'a pas été modifiée, et aucune copie n'a été écrite. (…) reprends la copie d'avant la migration 0005, `…\mementum.db.avant-0005.bak` (…) » ; les treize fichiers du dossier identiques, contenu et date ; le port ne répond pas |
| 5b. Copie témoin remise en place, lot 4 relancé | migrer de nouveau, reconstruire l'index (un `search/` d'une autre époque de la base) | « base migrée (…) kept=true » (la copie déjà là est reconnue, aucune autre écrite) ; « (…) il est reconstruit, en fond » ; 407 lectures identiques au lot 3 ; 264 et 66 justes (p07 n'a plus que p13 : la projection est celle de la base remise) |
| 5c. (plan) Copie prise serveur arrêté, **une seule** écriture indexée, copie remise en place | reconstruire ; le texte écrit entre-temps dans aucun extrait | une ligne ajoutée à p07 (« palimpsestique »), trouvable 2,3 s après ; copie remise : « (…) il est reconstruit, en fond » ; le mot n'est dans aucun extrait de 282 recherches (six couples, toutes les recherches de l'essai et quatre de plus) ; p07 trouvée avec son ancien texte ; 407 lectures identiques au lot 3 |

## Mesures

À cette taille (vingt-deux pages, 2 113 octets de MMD, 61 entrées d'index), sans en faire un budget :

| Mesure | Valeur |
|---|---|
| Migration (copie et `0005`) | « base migrée » 37 à 38 ms après le lancement du processus, « serveur prêt » à 49 ms (trois essais sur des copies neuves) ; le même binaire sur une base déjà migrée est prêt à 29 ou 30 ms : la migration coûte une vingtaine de millisecondes |
| Première indexation | `indexing` vrai aux réponses reçues à +139 et +203 ms, faux à partir de +262 à +266 ms après le lancement (trois essais) : finie en moins de 0,27 s |
| Reconstruction après `search/` supprimé, ou base remise en place | à jour dès la première réponse, environ 0,8 s après le lancement (lancement par PowerShell) |
| `mementum reindex` | 153 à 235 ms, processus compris (avec la migration pour la copie neuve) ; `--text-only` 142 ms |
| Une écriture rendue trouvable | 2,3 s (calme de 2 s par défaut) |
| Arrêt propre (Ctrl-C) | 0,75 à 1,1 s |
| Base | 266 240 octets avant (lot 3, après arrêt propre) ; 319 488 après migration, indexation et arrêt propre (+53 248, +20 %) ; la copie d'avant migration, 266 240 |
| `search/` | 20 245 octets en dix fichiers (dont deux verrous vides) |

## Ce qui n'a pas été fait

- Rien par l'application web (ni Vite ni navigateur) : tout est passé par l'API, que l'application utilise ; la recette G6 (« Une instance d'avant ce lot ») reste à jouer par Lenny s'il veut la voir dans l'écran.
- Pas de session ouverte sous le lot 3 réutilisée sous le lot 4 : chaque relecture s'est reconnectée.
- La mémoire du serveur n'a pas été relevée.
- Une seule taille d'instance, petite : les mesures à grande échelle sont celles des budgets du lot (README, « Index de recherche »).

## Outils de l'essai

Écrits dans le dossier de la tâche, puis supprimés avec lui : un script Node (remplir l'instance, la relire au nom de chaque compte, comparer deux relectures, chercher et lire les rétroliens contre le modèle, attendre la fin de l'indexation, une écriture puis l'absence de son texte), deux lecteurs de base par `node:sqlite` (toujours sur une copie), et trois scripts PowerShell : lancer le serveur dans une console cachée à lui (`Start-Process -WindowStyle Hidden`, sorties dans des fichiers), l'arrêter proprement en envoyant Ctrl-C à cette seule console (`AttachConsole` puis `GenerateConsoleCtrlEvent`, depuis un processus à part), et relever l'empreinte de chaque fichier du dossier des données en lecture partagée (pour comparer pendant que le serveur les tient).
