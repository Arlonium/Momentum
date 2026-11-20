# Lot 4, Task S0 : le moteur de recherche mesuré avant de bâtir (Tantivy contre FTS5)

Compte rendu de la Task S0 du plan [`2026-11-19-p1-lot4-recherche.md`](../plans/2026-11-19-p1-lot4-recherche.md).
Mesuré le 2026-11-19, sur le poste de développement (Windows 11, 24 cœurs, disque NVMe), en
`--release`, une commande lourde à la fois. Aucun code du produit : le banc est dans
[`spike/search-bench/`](../../../spike/search-bench/) (son propre atelier Cargo), ses sorties
brutes dans `spike/search-bench/results/`.

> **Corrigé le 2026-11-20 d'après la [relecture indépendante](2026-11-19-p1-lot4-s0-relecture.md)** (partielle : constats I1 à I6, m1 à m7), **sans rien mesurer de nouveau**. Ce que la relecture a mesuré l'a été sur une copie du banc, avec deux ajouts à elle (`fts-alt`, `--word-len`), supprimés avec son dossier : ces chiffres sont cités ici comme « mesuré par la relecture, sur une copie du banc, non rejoué », et n'ont pas de sortie dans `results/`. Les propositions de Rulings de la fin sont remplacées par le [registre du lot 4](2026-11-19-p1-lot4-ledger.md), R921 à R933. Le verdict ne change pas ; l'arbitrage avec FTS5 est plus serré que la première version ne le disait, et la forme hostile du plan n'était pas le pire cas.

## Verdict

**Tantivy tient. La tâche ne rend pas `BLOCKED`.** Les deux budgets du §5.10 qui dépendaient
du moteur tiennent à la mesure existante, les licences passent, et les trois exigences du
Step 3 (R916, R902, R909) se tiennent simplement, par l'API publique de Tantivy 0.27.0.

Quatre réserves, à lire avant de décider (détail plus bas) :

1. **Le filtre par pages permises ne se pose pas « dans la requête »** (un ensemble de termes
   exacts : 65 à 100 ms pour 10 000 pages, 540 à 580 ms pour 100 000). Il se pose **dans le
   collecteur**, par une colonne rapide qui porte l'identifiant de la page : 0 à 13 ms de plus.
   Le plan le dit désormais ainsi (R921).
2. **La mémoire de travail après usage dépasse 150 Mo**, par les fichiers de l'index projetés
   en mémoire (188 Mo après 500 recherches sur un index de 651 Mo), alors que la mémoire privée
   ne prend que 5 à 20 Mo et que la mesure du budget telle qu'elle existe (2 s après le
   lancement) ne bouge que de 4 Mo. Ce que « au repos » compte est retenu par défaut au
   registre (R924), et reste à Lenny. C'est surtout une question de comptabilité : SQLite passe
   par le même cache de fichiers du système, sans qu'il soit compté au processus (relecture, I6).
3. **Le disque** : l'index plein texte pèse 2,5 fois le texte indexé, `blocks_index` 1,9 fois.
   Les deux copies neuves font 4,4 fois le texte. La condition de la décision L18 (« l'ensemble
   sous le double de la base ») ne se juge pas avec ces chiffres : la base n'est qu'estimée, sans
   les versions de l'historique (relecture, m4 ; R931).
4. **Le temps de réponse comme canal** se mesure : 1,6 ms de plus pour 100 000 entrées non
   permises qui portent le mot (16 ns par entrée), contre un bruit de 0,03 à 0,2 ms dans le
   processus. Décision L19.

5. **La forme hostile mesurée n'est pas le pire cas** (relecture, I2 et I3) : des mots courts
   tous différents pèsent 4,3 fois les octets reçus dans le champ des débuts de mot, pas 1,25,
   et rien ne borne la mémoire des fusions (R928, risque ouvert RO-1).

**FTS5** est bien dans le SQLite du serveur et n'a rien à tenir cohérent. Dans la forme que
cette tâche a mesurée, il manque le budget proposé d'une recherche d'un ordre de grandeur ;
**cette forme lui était défavorable** : dans une variante simple (mesuré par la relecture, sur une copie du banc, non rejoué ;
Step 5), il ne le manque plus que d'un facteur 1,2 à 1,7, sur le seul pire cas, sans note, et
Tantivy reste 1,5 à 7 fois plus rapide. Sa note `bm25` lit les statistiques de toute la table,
ce que R916 interdit, et ses écritures passent par le tour d'écriture de SQLite.
**Recommandation pour L1 : Tantivy**, par un arbitrage plus serré que la première version de ce
compte rendu ne le disait.

## Version, corpus, méthode

- **Tantivy 0.27.0** (ce que cargo résout le 2026-11-19 ; `rust-version = 1.88`, sous le
  `1.96` du dépôt ; licence MIT), fonctions par défaut. Son API a été lue dans ses sources
  (`~/.cargo/registry/src/*/tantivy-0.27.0`), pas de mémoire.
- **SQLite 3.51.3**, par `libsqlite3-sys` 0.37.0, la version du `Cargo.lock` de la racine.
- **Corpus** fait par programme, toujours le même (`src/corpus.rs`) : un vocabulaire de 60 000
  mots (du français courant avec ses accents, puis des mots faits de syllabes), tirés selon une
  loi de Zipf ; 5 à 40 mots par bloc. Espace A : 10 000 grandes pages titrées « Projet … »
  (une de 10 000 blocs, les autres de 99) et 90 000 petites pages d'un bloc (pour le filtre de
  100 000 identifiants). Espace B, jamais permis : 1 000 pages de 100 blocs. En tout
  **1 290 901 entrées** (1 189 901 blocs, 101 000 titres), **262 Mo de texte**. C'est un peu plus
  que le « million de blocs » du plan.
- **La forme d'index mesurée** est celle du plan (une entrée par bloc, le titre recopié sur
  chaque bloc, une entrée par titre, les débuts de mot de 2 à 10 caractères dans deux champs),
  plus trois colonnes rapides (`page_hi`, `page_lo` : l'identifiant de la page en deux entiers ;
  `blk` : le rang du bloc dans sa page, pour départager ; il n'est stable que parce que le
  corpus du banc est figé : dans le produit, un bloc inséré décale le rang des suivants, et le
  départage retenu est une empreinte de la clé du bloc, relecture I4, R922), sans positions (aucune recherche d'expression), le
  texte gardé, le titre non gardé (R904 le lit dans `nodes`).
- **Écrivain** : un fil d'indexation, un fil de fusion, tampon de 15 Mo (le plus petit admis :
  `MEMORY_BUDGET_NUM_BYTES_MIN`). Recherche sur un seul fil (l'exécuteur par défaut).
- Chaque mesure est répétée : trois passages de la commande pour les latences et les
  validations, six lancements pour l'ouverture. Les fourchettes données sont celles des
  passages. Le poste était au repos ; les latences y varient de moins de 5 % d'un passage à
  l'autre. **Le cache de fichiers du système était chaud partout** : aucune mesure à froid.

Refaire : `spike/search-bench/README.md`.

## Step 1 : épingler

| Question | Résultat |
|---|---|
| Version | Tantivy 0.27.0 |
| Rust demandé | 1.88 (le dépôt demande 1.96) : tient |
| Licences | `cargo deny check licenses bans` (cargo-deny 0.22.0, le `deny.toml` du dépôt, sur un `Cargo.toml` d'essai qui ne dépend que de Tantivy) : **`bans ok, licenses ok`**. 145 crates toutes cibles confondues (`tantivy-licences.txt`), toutes sous une licence de la liste (MIT, Apache-2.0, BSD-2/3, Zlib, Unicode-3.0). Trois doublons signalés sans bloquer (`getrandom`, `r-efi`, `windows-sys`). |
| Crates neuves | 65 noms absents du `Cargo.lock` du serveur, dont `zstd-sys` (du C, par la fonction par défaut `columnar-zstd-compression`), `rayon`, `memmap2`, `fs4`, `lz4_flex`, `frostem` (la racinisation). Non mesuré : ce qu'on retire en coupant les fonctions par défaut. |
| Compilation | un binaire d'essai (créer un index, écrire, valider, chercher) : **40 s** de compilation complète en release sur 24 cœurs, contre 0,3 s sans Tantivy ; 1,5 s pour une recompilation du seul binaire. La CI a 4 cœurs : non mesuré là-bas. |
| Taille du binaire | **+5,5 Mo** (5,58 Mo contre 0,13 Mo pour le binaire d'essai, sans LTO) |

Le Step 1 n'a aucune sortie brute dans `results/`, hors `tantivy-licences.txt` (relecture,
m1) : la durée de compilation, la taille du binaire et le compte des 65 crates ne se recoupent
pas. La CI lance `cargo deny check` entier ; S0 n'a passé que `licenses bans`. C'est à refaire
sur l'atelier du serveur quand S3 y ajoute Tantivy.

## Step 2 : mesures, Tantivy

### Mémoire et démarrage (§5.10)

| Mesure | Résultat (trois passages) |
|---|---|
| Ouvrir l'index, le lecteur, relire la charge utile, créer l'écrivain | **2,4 à 2,8 ms** en tout (index 0,4 ; lecteur 1,3 ; charge utile 0,05 à 0,07 ; écrivain 0,7 à 1,0), six processus neufs |
| Première recherche d'un processus neuf | 10 à 11 ms |
| Mémoire, index ouvert et écrivain au repos, 2 s après | **+4 Mo** de mémoire de travail (6,7 → 10,6 à 10,8), +4,3 Mo de mémoire privée, 2,6 Mo de tas |
| Après une validation d'une entrée, sans aucune recherche, 2 s après | environ +2 Mo de mémoire de travail de plus (18,1 Mo, dont 6 Mo du vocabulaire du banc) |
| Après 500 recherches mêlées | mémoire de travail **185 à 190 Mo**, mémoire privée 20 à 24 Mo, tas 11 à 14 Mo (dont, chaque fois, 6 Mo du vocabulaire du banc) |
| 2 s après avoir indexé tout le corpus dans le processus, écrivain lâché | 34 Mo de mémoire de travail, 42 Mo de mémoire privée (`tantivy-build-50.txt`, non cité d'abord ; relecture, I6) |
| Pointe pendant l'indexation de tout le corpus | 366 Mo de mémoire de travail (tampon 15 Mo), 394 Mo (tampon 50 Mo) ; tas 167 et 212 Mo |
| Pointe pendant 58 validations à la suite (remplacements, renommages) | 171 à 190 Mo de mémoire de travail |

Lecture. Le serveur d'aujourd'hui pèse une quinzaine de Mo au repos (13,1 Mo au lot 1). La
mesure du budget (`idle_rss_under_150mb`, `mementum-cli` : la mémoire de travail 2 s après le
lancement) passera donc largement, index de 651 Mo ouvert compris. **Mais Tantivy lit ses
fichiers par projection en mémoire**, et les pages lues comptent dans la mémoire de travail
(sous Windows comme dans le RSS de Linux) tant que le système ne les reprend pas : un serveur
qui a répondu à des recherches, ou dont l'indexeur a fusionné des segments, montre une mémoire
de travail qui tend vers la taille de l'index. Ce n'est pas de la mémoire que le processus
tient (la mémoire privée reste à 20 Mo), le système la reprend sous pression, mais un
exploitant qui lit le RSS la verra. Tantivy 0.27 n'offre pas d'autre lecture du disque (sans la
fonction `mmap`, il ne reste que l'index entier en mémoire) : c'est vrai des répertoires que
Tantivy fournit ; `Directory` est un trait public, et un répertoire à nous n'a pas été examiné.
Sur un Pi 4 (non mesuré), c'est le cache de fichiers qui s'en ressent.

Trois manques de cette mesure (relecture, I6), repris par R924 et par la Task X2 :
- **la mesure que le plan définit n'est pas faite** : « une fois le rattrapage fini », écrivain
  et lecteur tenus, sur un index de 10 000 pages. Le banc mesure « écrivain lâché » ou « pas de
  rattrapage », dans un processus nu. Et `idle_rss_under_150mb` part d'un dossier vide : il ne
  voit aucun index ;
- **rien sous Linux**, où la CI lit le RSS, fichiers projetés compris. La « mémoire privée » du
  banc est `virtual_memory()` de sysinfo : la mémoire privée sous Windows, mais la taille
  virtuelle sous Linux, où il faut lire `RssAnon` ;
- c'est **surtout une question de comptabilité** : SQLite lit ses pages par le cache de
  fichiers du système, qui n'est pas compté au processus ; Tantivy les projette, et elles le
  sont.

### Indexation, validations

| Mesure | Résultat |
|---|---|
| Débit, tampon de 15 Mo | **3 585 entrées/s, 0,73 Mo/s** : 360 s pour le corpus, puis 14 s de fusions. Un segment est vidé toutes les 3 600 entrées environ (les débuts de mot font beaucoup de termes), et le fil de fusion suit. |
| Débit, tampon de 50 Mo | **11 286 entrées/s, 2,29 Mo/s** : 114 s, puis 6 s de fusions (un seul passage) |
| Une validation de 100 000 entrées pendant la construction | p50 2,85 s (15 Mo), 1,16 s (50 Mo) |
| Validation, 1 bloc remplacé | p50 **81 à 90 ms**, max 221 à 270 ms (20 validations par passage) |
| Validation, 100 blocs remplacés | écriture 4 à 5 ms, validation p50 78 à 100 ms, max 202 à 837 ms |
| Validation, 1 000 blocs remplacés | écriture 45 à 47 ms, validation p50 222 à 261 ms, max 338 à 783 ms |
| Validation vide (le jeton seul) | p50 4 à 6 ms |
| Renommage d'une page de 99 blocs (toutes ses entrées récrites, validation comprise) | p50 79 à 82 ms, max 156 à 169 ms |
| Renommage d'une page de 10 000 blocs | **2,8 à 3,6 s** |

Une validation a un plancher d'environ 80 ms (fichiers vidés sur le disque, suppressions
appliquées aux segments), hors du tour d'écriture de SQLite. Les maxima sont bruités (ils
tombent quand une fusion tourne). Conséquences pour le plan : valider par lots, jamais par
bloc (le plan le fait déjà) ; le renommage en boucle d'une page de 10 000 blocs occupe
l'indexeur 3 s toutes les 15 s (R906), sur le fil bloquant ; le budget proposé « trouvable en
moins de 5 s » (L10) tient pour une page ordinaire (2 s de calme, puis 0,1 à 0,3 s).

### Latence d'une recherche

Tout compris : bâtir la requête, chercher, grouper par page, trier, relire le texte gardé de
20 pages. Filtre dans le collecteur, note à statistiques constantes. **Non compris : bâtir
l'ensemble des pages permises** (une table de 10 000 ou 100 000 identifiants, bâtie une fois
par le banc ; le plan la bâtit à chaque requête : son coût est à ajouter à ces chiffres, S5 le
mesure, R927 ; relecture, m5).
p50 et p95 sur 100 recherches, fourchette des trois passages, en ms.

| Requête | Entrées vues | 10 pages permises | 10 000 | 100 000 |
|---|---|---|---|---|
| 1 terme rare | 130 | p95 0,11 à 0,12 | 0,16 à 0,17 | 0,14 à 0,16 |
| 1 terme moyen (rang 300) | 9 567 | p95 0,58 à 0,66 | 1,74 à 1,84 | 1,44 à 1,80 |
| 1 terme très courant (« de ») | 695 569 | p50 14,1 ; p95 14,7 à 15,4 | p50 21,0 ; p95 22,1 à 23,0 | p50 32,9 ; p95 34,0 à 34,5 |
| mot du titre de toutes les grandes pages (« projet ») | 1 110 901 | p50 18,2 ; p95 18,9 à 19,9 | p50 27,5 ; p95 28,5 à 29,6 | p50 27,6 ; p95 28,8 à 29,5 |
| mot du titre d'une page de 10 000 blocs | 10 001 | p95 0,29 à 0,33 | 0,36 à 0,48 | 0,36 à 0,65 |
| 3 termes courants | 2 435 | p95 6,1 à 6,2 | 6,7 à 6,9 | 6,5 à 6,7 |
| 3 termes dont le préfixe « pr » | 50 503 | p95 18,1 à 18,4 | 20,9 à 21,3 | 21,3 à 21,8 |
| 8 termes (les huit mots les plus courants) | 821 | p95 28,5 à 29,6 | 28,6 à 28,9 | 28,2 à 28,8 |
| 8 termes dont le préfixe « pro » | 2 738 | p95 34,3 à 34,9 | 34,6 à 35,1 | 34,6 à 35,4 |
| préfixe seul « pr » | 1 156 705 | p50 23,9 ; p95 24,9 à 25,6 | p50 33,0 ; p95 34,8 à 34,9 | p50 40,2 ; **p95 41,6 à 42,6** |
| préfixe seul de 10 caractères | 130 | p95 0,11 à 0,13 | 0,15 à 0,25 | 0,15 à 0,17 |

**Pire p95 : 42,6 ms**, sous les 100 ms proposés (L10), pour un membre qui voit tout comme
pour un invité qui voit dix pages. Le pire maximum relevé est 47 ms. Le pic du tas d'une
recherche : 1,4 Mo pour 10 000 pages trouvées, 10,6 Mo pour 60 879. Ces chiffres sont ceux
d'une recherche seule, index au repos : non mesuré, la latence pendant une fusion ou sous
quatre recherches de front (X2).

R920, le préfixe de plus de 10 caractères : relire et vérifier le texte gardé de 2 000 entrées
de pages différentes coûte **17 à 18 ms** (max 29 ms). La borne `MAX_VERIFIED_ENTRIES = 2 000`
tient dans le budget.

### Taille sur le disque

Corpus ordinaire, 262 Mo de texte reçu (un seul passage par forme ; les tailles ne sont pas
bruitées) :

| Quoi | Taille | Rapporté au texte |
|---|---|---|
| Index Tantivy, forme du plan | **651 Mo** (5 segments après fusions) | 2,48 |
| dont textes gardés (pour l'extrait) | 256 Mo | 0,98 |
| dont champs des débuts de mot (`text_starts` 215 Mo, `title_starts` 32 Mo) | **247 Mo** | 0,94 |
| dont `entry` (le terme `<node>/<bloc>`, 69 caractères) | 58 Mo | 0,22 |
| dont `text` (dictionnaire et listes) | 51 Mo | 0,20 |
| dont `title` recopié sur chaque bloc | 9 Mo | 0,03 |
| dont colonnes rapides (`page_hi`, `page_lo`, `blk`) | 22 Mo | 0,08 |
| **Sans les champs des débuts de mot** | **404 Mo** | 1,54 |
| `blocks_index`, la table du plan (`WITHOUT ROWID`, clé de deux UUID en texte) | **499 Mo** pour 260 Mo de texte | 1,92 |
| Index FTS5 seul (sans préfixes), pour comparaison | 114 Mo | 0,44 |

Les deux copies neuves du texte (`blocks_index` et l'index plein texte) font **1 150 Mo pour
262 Mo de texte**. La base d'aujourd'hui pour ce corpus n'a pas été mesurée (S4c le fait) ;
d'après les 3,2 Mo d'une page de 10 000 blocs, elle serait de l'ordre de 400 à 550 Mo : les
copies neuves la doublent à la triplent, et l'ensemble passe à trois ou quatre fois la base.
C'est une estimation, **qui ignore les versions de l'historique** (cent versions d'une page de
10 000 blocs : 320 Mo) : dire que la condition de L18 (« sous le double de la base
d'aujourd'hui ») ne tient pas est fragile dans les deux sens (relecture, m4). R931 retient les
trois copies aux tailles mesurées, sans cette condition, et rouvre le choix en S4c. Ce qui se gagne, d'après
le tableau : ne pas garder le texte dans l'index (−256 Mo, le repli que L18 prévoit) ; la clé
de `blocks_index` (deux UUID en texte de 36 caractères, insérés dans un ordre quelconque : la
table pèse le double de son texte).

Forme hostile (des blocs de 64 Kio de mots de 40 octets tous différents), 1 000 blocs,
65,5 Mo reçus :

| Quoi | Taille | Rapporté aux octets reçus |
|---|---|---|
| Index Tantivy entier | 222 Mo | 3,39 |
| champ `text_starts` (borné à 10 caractères, R920) | 82 Mo | **1,25** |
| champ `text` (le dictionnaire de mots tous différents) | 74 Mo | 1,14 |
| textes gardés | 66 Mo | 1,00 |
| FTS5, table et index | 140 Mo | 2,13 |

Ces 1 000 blocs sont indexés en 60 s (1,08 Mo/s), 99 s fusions comprises, pointe de mémoire
de travail 214 Mo ; chez FTS5, en 6 s, un bloc par transaction (la pire : 37 ms).
La borne de R920 fait son office **pour cette forme** : le champ des débuts pèse 1,25 fois les
octets reçus (sans borne, un mot de 40 octets donnerait 39 débuts, soit de l'ordre de vingt
fois).

**Cette forme, celle que le plan demandait, n'est pas le pire cas** (relecture, I2). R920 borne
par mot, pas par octet : des mots tous différents de 10 caractères donnent 9 débuts pour 11
octets reçus, contre 9 pour 41. Sur 300 blocs de 64 Kio, 19,7 Mo reçus (mesuré par la relecture, sur une copie du banc, non rejoué) :

| Mots de | `text_starts` | Index entier | Débit | Pointe de mémoire de travail |
|---|---|---|---|---|
| 40 octets (forme du plan) | 24,7 Mo, 1,25 fois | 3,40 fois | 1,07 Mo/s | 155 Mo |
| 10 octets | 84,2 Mo, **4,28 fois** | **6,68 fois** | 0,51 Mo/s | **455 Mo** |

Le pire cas à dire est donc : le champ 4,3 fois les octets reçus, l'index 6,7 fois (R928).

**À 10 000 blocs, la forme exacte du plan** (655 Mo reçus, un seul passage) : dossier de
**2 165 Mo, 3,30 fois les octets reçus** ; `text_starts` 768 Mo (**1,17 fois**), `text` 738 Mo
(1,13), textes gardés 658 Mo (1,00). Indexés en 617 s (1,06 Mo/s), **1 221 s fusions
comprises**, et une **pointe de mémoire de travail de 802 Mo** pendant les fusions (la part
privée n'a pas été relevée à part pour cette forme ; sur 300 blocs de la même forme, la
relecture a relevé 146 Mo de tas pour 156 Mo de mémoire de travail : la pointe est du tas, pas
des fichiers projetés, ce qui rend vraisemblable, sans le prouver à 10 000 blocs, que les
802 Mo sont de la mémoire réelle, donc un risque sur une petite machine ; la part de tas de la
forme à mots de 10 octets n'est pas établie, la passe ayant été interrompue ; I3). Le rapport de taille ne dépend donc pas
de l'échelle ; la pointe de mémoire et la durée des fusions, si : un compte qui écrit 640 Mo
de mots tous différents (5 000 pages pleines, à la borne de 8 Mio par page) occupe le fil de
fusion vingt minutes et fait monter la mémoire de travail du serveur à 800 Mo. Rien du lot 4
ne le borne, et le tampon de l'écrivain non plus (tas de 167 Mo avec un tampon de 15 Mo sur le
corpus ordinaire) : c'est le risque ouvert RO-1 du registre (R928), que S3 mesure (le tas, une
politique de fusion bornée) et que X2 relève dans le serveur ; à défaut, limite L-g et lot 7.

## Step 3 : les trois exigences

### R916, une note sans statistique d'ensemble : **tient**

Comment : `Searcher::search_with_statistics_provider(&query, &collector, &provider)`, API
publique de Tantivy, avec un `Bm25StatisticsProvider` écrit par nous qui rend **des
constantes** (nombre d'entrées, nombre de mots par champ, fréquence d'un terme) : une quinzaine
de lignes. La note garde la forme BM25 (fréquence du terme dans l'entrée, longueur de
l'entrée, champ touché), sans rien lire de l'index hors de l'entrée.

Essai (`r916`) : un index de 40 entrées permises dosées (`dosea` et `doseb` en proportions
inverses), et trois index qui n'y ajoutent que du hors-filtre : 3 000 entrées d'un autre
espace, 3 000 entrées de pages non permises du même espace, 3 000 entrées supprimées et pas
encore fusionnées (660 encore dans les segments). Cinq requêtes (`dosea`, `doseb`, les deux,
le préfixe `dos`, un terme et un préfixe).

- Statistiques constantes : **mêmes pages, même ordre, mêmes notes au bit près dans les 15
  cas**.
- Témoin, la BM25 de l'index telle quelle (`Searcher::search`) : **l'ordre change dans 12 cas
  sur 15**, et les notes dans les 15. L'essai voit donc ce qu'il doit voir.

Le préfixe est servi par un terme exact du champ des débuts de mot (`TermQuery`, une seule
recherche dans le dictionnaire, aucun parcours) : c'est la construction même de la requête,
bâtie par programme.

Deux réserves de la relecture sur ce que cet essai porte :
- **il ne prouve rien du départage de deux notes égales** (I4) : les entrées permises ont les
  mêmes adresses dans les quatre index de l'essai (`small_index`), si bien qu'un départage par
  adresse d'entrée, que R916 interdit, serait passé aussi. S3 le prouve (R922) ;
- **un préfixe a une note constante** (m3) : les champs des débuts de mot sont indexés sans
  fréquence ni longueur (`Basic`). Or le plan fait du dernier terme un préfixe (R911) : une
  recherche d'un seul mot range ses résultats hors titre par identifiant de page. Avec la note à
  statistiques constantes et sans racinisation, l'argument « meilleur classement » de Tantivy
  pour L1 est affaibli. R930 y répond en partie (le dernier terme est aussi cherché comme mot
  entier, noté), le reste est la limite L-q.

Trois choses à ne pas faire, trouvées en lisant les sources, à écrire en S3 :
- **`SnippetGenerator` lit la fréquence des termes dans l'index** pour choisir son fragment
  (`snippet/mod.rs`) : l'extrait est à bâtir par programme, sur le texte de l'entrée ;
- **ne jamais départager deux notes égales par l'adresse d'une entrée dans l'index**
  (`DocAddress` : segment et rang), qui dépend des autres entrées et des fusions ; le banc
  départage par l'identifiant de la page, et le bloc rendu par le rang du bloc gardé en colonne
  (dans le produit : une empreinte de la clé du bloc, R922) ;
- `Searcher::search` (sans fournisseur) et le fournisseur par défaut ne doivent paraître nulle
  part dans le chemin de la recherche : un test de sources, comme `only_the_depot_forgets_files`.

### R902, le groupement par page avant la pagination : **tient**

Un collecteur écrit par nous (`Collector` et `SegmentCollector`, une centaine de lignes) lit
pour chaque entrée trouvée l'identifiant de sa page dans deux colonnes rapides, écarte les
pages non permises, et garde la meilleure entrée de chaque page ; `skip` et `limit` comptent
ensuite des pages.

| Cas | Entrées vues | Pages rendues | Latence | Tas |
|---|---|---|---|---|
| un mot touche 1 110 901 entrées de 10 000 pages permises (et 1 000 non permises) | 1 110 901 | 10 000, toutes distinctes ; la page de 10 000 blocs y paraît une fois | p50 27,5 ms, p95 28,5 à 29,6 ms | 1,4 Mo |
| un mot touche 10 001 entrées d'une seule page | 10 001 | 1 | p50 0,36 ms | 0,1 Mo |

Deux réponses qui se suivent (pages 1 à 20, puis 21 à 40) ne partagent aucune page : **ce
n'est pas une preuve** (relecture, m2). Le banc coupe en deux une même liste déjà
dédoublonnée, ce qui passerait avec toute implémentation ; ce ne sont pas deux recherches avec
`skip`. La preuve est un test de S3, puis de S5 par la route. Le budget
proposé tient avec une marge de trois : **le repli (ne plus recopier le titre sur les blocs)
n'a pas été mesuré, il n'est pas nécessaire.** Ce que R902 demande au schéma de l'index, et
que le plan ne liste pas : l'identifiant de la page en colonne rapide (22 Mo ici).

### R909, la charge utile d'une validation : **tient**

`IndexWriter::prepare_commit()`, `PreparedCommit::set_payload(&str)`, `commit()` ; relue par
`Index::load_metas()?.payload` en 0,05 à 0,07 ms (0,3 ms dans un processus qui vient d'ouvrir).
Un index neuf a `None`.

- Une validation préparée, jeton posé, puis abandonnée (`abort`) : la charge utile et les
  entrées lisibles restent celles de la validation d'avant.
- **25 arrêts brutaux** d'un processus qui valide en boucle (200 entrées et un jeton par
  validation), tué à un instant quelconque : à chaque fois la charge utile relue est celle de
  la dernière validation dont les entrées sont lisibles, jamais l'une sans l'autre (24 fois la
  dernière validation que le processus avait dite, 1 fois une validation faite et pas encore
  dite) ; aucun fichier abîmé (`validate_checksum`), et l'écrivain se reprend aussitôt (le
  verrou d'un processus tué est rendu).

Non essayé : une coupure de courant (un processus tué laisse le système écrire ce qu'il avait
en tampon).

## Step 4 : quatre questions

**(a) Le filtre par un ensemble de pages.** Posé dans la requête (`TermSetQuery` sur le champ
`node`, en clause exigée) : 0,5 ms pour 10 pages, mais **65 à 68 ms pour 10 000** (97 à 103 ms
sur le mot du titre) et **541 à 584 ms pour 100 000**, plus 4,5 et 16,6 Mo de tas : ne tient
pas. Posé dans le collecteur (une table des pages permises, consultée pour chaque entrée
vue) : c'est ce que mesure le tableau des latences, 0 à 13 ms de plus que pour dix pages. Il
reste appliqué dans la recherche, avant le groupement, donc avant `skip` et `limit` : le
nombre de pages rendues et leur ordre ne dépendent que des pages permises. Un terme d'espace
exigé en plus ne change rien d'utile (+3 ms sur un million d'entrées) : le filtre des pages
suffit.

**(b) Le tokeniseur.** `SimpleTokenizer`, `RemoveLongFilter`, `LowerCaser`,
`AsciiFoldingFilter` : « élève » est trouvé par « eleve », « ÉLÈVE » par « élève », « cœur »
par « coeur », « straße » par « strasse ». À savoir :
- **des accents combinants (texte en NFD, comme les noms de fichiers venus de macOS) coupent le
  mot** : « élève » en NFD donne `e`, `le`, `ve`. La projection doit normaliser en NFC avant
  d'indexer (`unicode-normalization` est déjà dans le `Cargo.lock`), et la requête de même ;
- `RemoveLongFilter::limit(n)` écarte un mot de `n` octets : pour garder les mots de 40
  octets, la limite est 41. Un mot plus long n'est pas indexé du tout (pas tronqué) ;
- **le chinois, le japonais et le coréen ne sont pas découpés** : une suite sans espace est un
  seul mot (« 中文搜索引擎测试 » n'est pas trouvé par « 搜索 »), et au-delà de 13 caractères
  (40 octets) elle n'est pas indexée. Le préfixe trouve le début d'une suite, rien d'autre.
  Une limite à écrire ;
- `snake_case` donne `snake` et `case` (trouvé par l'un ou l'autre) ; `camelCase` reste un
  mot ; « l'élève » donne `l` et `eleve`, « aujourd'hui » `aujourd` et `hui`, « e-mail » `e`
  et `mail`, « v2.0.1 » `v2`, `0`, `1` ; une adresse est coupée à chaque signe.

**(c) La racinisation française** (`Stemmer(Language::French)`, la fonction `stemmer`). Sur
vingt paires françaises choisies à la main, elle en réunit dix que rien ne réunit sans elle
(cheval et chevaux, page et pages, national et nationaux, recherche et rechercher…), et en
laisse dix séparées (travail et travaux, nouveau et nouvelle, petit et petite, créer et
créée, les verbes irréguliers). Sur trente paires anglaises, elle en réunit **six à tort**
(`import` et `important`, `pages` et `pager`, `change` et `changer`, `server` et `serve`,
`marine` et `marin`, `relation` et `relative`) et abîme les mots (`document` devient `docu`,
`commit` devient `comm`). L'échantillon est petit et choisi à la main : c'est une indication,
pas une mesure de rappel. Le préfixe couvre déjà les pluriels en « s » du dernier mot.
**Avis pour L6 : pas de racinisation au lot 4.** Elle changerait aussi les débuts de mot (un
champ de plus, ou des débuts de racines).

**(d) Le temps de réponse comme canal.** La même requête d'un mot, le même ensemble permis,
3 000 mesures de chaque, tour à tour, trois passages :

| Ensemble permis | Mot porté par 10 entrées permises | Le même, plus 100 000 entrées non permises | Écart |
|---|---|---|---|
| 10 pages | p50 0,114 à 0,116 ms (écart type 0,03) | p50 1,711 à 1,723 ms (écart type 0,15 à 0,19) | **1,60 ms** |
| 10 000 pages | p50 0,115 à 0,116 ms | p50 1,712 à 1,716 ms | **1,60 ms** |

Soit **16 ns par entrée non permise** qui porte le mot (cohérent avec le tableau des
latences : 14 ms pour 700 000 entrées). L'écart est dix fois l'écart type dans le processus :
il se lit en une mesure. **À travers un réseau, non mesuré** : le plan demandait si l'écart
« se distingue du bruit d'un appel par le réseau local », et ce qui suit est un calcul, pas la
réponse mesurée (relecture, m7). Par le calcul : avec un bruit
d'un appel de l'ordre de 0,2 à 1 ms sur un réseau local, 1,6 ms se distingue en quelques
appels ; un mot porté par 10 000 entrées cachées (0,16 ms) demande de l'ordre d'une centaine
d'appels moyennés, un mot porté par 1 000 (16 µs) plusieurs milliers. Le débit de R910 ralentit
la mesure sans la fermer. La limite L-b est donc réelle pour les mots courants chez les
autres, pas pour un mot rare. Un index par espace ne fermerait que la part des autres
espaces, pas celle des pages non vues du même espace ; un temps de réponse plancher la ferme
sous le plancher (42 ms couvriraient ici 2,6 millions d'entrées). Décision L19 : R932 retient
le plancher, par défaut.

## Step 5 : la même mesure avec FTS5

FTS5 est dans le SQLite du serveur, vérifié **par une requête** passée par sqlx 0.9 avec les
fonctions du `Cargo.toml` de la racine (`fts-check`) : SQLite 3.51.3, options de compilation
`ENABLE_FTS5` (et `ENABLE_FTS3`), une table `fts5(…, tokenize='unicode61 remove_diacritics 2')`
créée, « eleve » y trouve « élève ».

Forme mesurée : la table `blocks_index` avec un `rowid`, les titres en lignes à part, une
table `fts5` à contenu externe dessus. **Une table à contenu externe demande un `rowid`
entier** : la `blocks_index` du plan (`WITHOUT ROWID`, clé `(node_id, block_id)`) ne peut pas
servir telle quelle. Requête : la meilleure ligne de chaque page permise (`JOIN` sur une table
temporaire des pages permises, `GROUP BY node_id`, note `bm25`), 20 pages, leur extrait.

| Question | Tantivy | FTS5 |
|---|---|---|
| Taille de l'index (hors `blocks_index`) | 651 Mo (404 sans les débuts de mot, 148 sans les débuts ni le texte gardé) | **114 Mo**, sans index de préfixes ni texte gardé : ces deux chiffres ne se comparent pas (plus bas) |
| Construction | 114 s (tampon 50 Mo) à 360 s (15 Mo) | **17 s** (74 000 entrées/s), par tranches de 20 000 lignes de 250 ms (max 478 ms) chacune, **dans le tour d'écriture** |
| 1 terme rare | p95 0,1 à 0,2 ms | p95 0,1 à 0,7 ms |
| 1 terme moyen (8 600 lignes), 10 / 10 000 / 100 000 pages | p95 0,6 / 1,8 / 1,8 ms | p50 30 / 49 / 72 ms |
| 1 terme très courant (640 000 lignes) | p95 15 / 23 / 34 ms | **p50 440 / 1 152 / 1 347 ms** |
| 3 termes courants | p95 7 ms | p50 16 / 37 / 43 ms |
| 8 termes | p95 29 ms | p50 126 à 129 ms |
| préfixe « pr » seul | p95 26 / 35 / 43 ms | **p50 595 / 1 152 / 1 357 ms** |
| Le canal : 100 000 lignes non permises de plus | +1,6 ms | **+53 ms** (+30 ms avec un cache de 1 Gio) ; +10,5 à +12,7 ms dans la variante de la relecture |
| Une écriture d'index dans le tour d'écriture de SQLite | aucune pour l'index plein texte (seul `blocks_index` y passe) | voir plus bas |
| Forme hostile, rapport aux octets reçus | 3,39 | 2,13 |
| Mémoire | fichiers projetés (réserve 2) | le cache de SQLite, borné et privé |

Les latences de FTS5 sont celles d'un seul passage de 30 recherches par ligne (elles ne se
jouent pas à 5 % près) ; avec un cache de 1 Gio (`cache_size`), le terme moyen descend à 16 ms
mais le terme très courant reste à 754 ms et le préfixe à 805 ms. Le coût est dans la lecture
de chaque ligne touchée pour connaître sa page (une recherche dans la table par ligne) et dans
`bm25`.

**Cette forme est défavorable à FTS5, et une variante simple la corrige** (relecture, I1 ; la
première version de ce compte rendu disait d'une autre forme que « rien n'indique qu'elle
gagnerait le facteur 10 à 30 qui manque » : c'était faux). Variante du relecteur : la page
rangée dans le `rowid` (`ord << 14 | rang`), table sans contenu, pages permises en table
d'entiers, même corpus, même SQLite 3.51.3, cache de 2 Mo, 12 passages ; `fts3` y ajoute
`prefix='2 3'`. Tout ce tableau est mesuré par la relecture, sur une copie du banc, non rejoué (la commande `fts-alt` n'est plus dans le
banc) :

| Requête (p50, ms) | Forme du banc, bm25 | Variante sans note | Variante bm25 | Tantivy (p95, plus haut) |
|---|---|---|---|---|
| terme moyen, 10 000 pages | 50 | 2,5 | 16 | 1,8 |
| « de », 10 pages | 441 | 37,5 | 52 | 15 |
| « de », 10 000 pages | 1 153 | 143 | 478 | 23 |
| « de », 100 000 pages | 1 335 | 162 | 529 | 34 |
| 3 termes dont « pr », 10 000 pages (`fts3`) | 334 | 25 | 75 | 21 |
| 8 termes, 10 000 pages | 130 | 50 | 133 | 29 |
| « pr » seul, 10 000 pages (`fts3`) | 1 147 | 116 | 393 | 35 |
| « pr » seul, 100 000 pages (`fts3`) | 1 347 | 131 | 440 | 43 |
| canal, +100 000 lignes non permises, 10 000 pages | +53 | +10,5 | +12,7 | +1,6 |

Ce que cela change. **Sans note**, FTS5 ne manque les 100 ms que d'un facteur 1,2 à 1,7, sur le
seul pire cas (un mot ou un préfixe très courant, pour qui voit 10 000 pages ou plus) ; il les
tient pour un invité et pour tout le reste. Son canal par le temps est **environ sept fois**
celui de Tantivy, pas trente. Ce qui reste vrai : Tantivy est 1,5 à 7 fois plus rapide ; le
`bm25` de FTS5 est interdit par R916 et coûteux (la colonne « variante bm25 ») ; une note sans
statistique demande du C, à un coût non mesuré ; les écritures restent dans le tour d'écriture.

**Le disque, à fonctions égales** (relecture, I1). « Cinq fois moins » comparait 651 Mo (texte
gardé et débuts de mot) à 114 Mo (ni l'un ni l'autre). Mesuré par la relecture : l'index FTS5
sans contenu pèse 134,9 Mo, et 288,8 Mo avec `prefix='2 3'`, construit en 34 s. À fonctions
égales : environ 395 Mo pour Tantivy sans texte gardé, contre 270 à 290 Mo pour FTS5 avec ses
préfixes courts. Tantivy pèse donc de l'ordre de 1,4 fois FTS5, pas cinq.

Non étudié : la tenue de cette variante dans le produit (elle suppose un ordinal entier par
page et 14 bits de rang par bloc, donc des rangs stables, et la suppression dans une table
sans contenu) ; FTS5 avec une note sans statistique ; son coût d'écriture avec un index de
préfixes.

**Durée d'une transaction d'index dans le tour d'écriture** (`BEGIN IMMEDIATE` … `COMMIT`,
WAL, `synchronous=FULL`, base de 664 Mo), avec l'index FTS5 puis sans (`blocks_index` seul,
ce que Tantivy laisse dans le tour), trois passages :

| Blocs récrits dans la transaction | Avec FTS5 | `blocks_index` seul |
|---|---|---|
| 1 | p50 1,7 à 2,7 ms, max 5 à 28 ms | p50 1,1 à 2,8 ms, max 1,5 à 3,6 ms |
| 100 | p50 3,3 à 4,0 ms, **max 29 à 41 ms** | p50 1,2 à 2,5 ms, max 3 à 17 ms |
| 1 000 (de l'ordre de `MAX_INDEX_TX_BYTES`, 256 Kio) | **p50 19 à 51 ms, max 34 à 94 ms** | p50 3,0 à 4,4 ms, max 5 à 16 ms |
| 10 000 | p50 313 à 482 ms | p50 22 à 26 ms |

C'est bruité (les maxima tombent quand FTS5 fusionne ses segments), et ce disque est rapide :
la CI et un disque lent feraient pire. FTS5 mettrait donc de 20 à 90 ms de plus devant la
frappe des autres pour une transaction d'index de mille blocs, là où `blocks_index` seul en
met 3 à 4 ; il faudrait baisser `MAX_INDEX_TX_BYTES` d'un ordre de grandeur.

**Les trois exigences chez FTS5** :
- **R916 : ne tient pas simplement.** La note `bm25` de FTS5 lit le nombre de lignes, la
  longueur moyenne et le nombre de lignes qui portent le terme, toute la table comprise :
  l'ordre des pages permises change dans **8 cas sur 8** de l'essai dosé (`fts-r916`). Une
  note sans statistique demande une fonction auxiliaire écrite contre l'API C de FTS5
  (`fts5_api`, que ni sqlx ni rusqlite n'enveloppent : du code `unsafe`) : non essayée. Un
  préfixe, sans index de préfixes, parcourt la plage des termes ; avec `prefix='2 3 … 10'`, il
  devient une recherche exacte, à une taille non mesurée.
- **R902** : le groupement est un `GROUP BY` qui lit toutes les lignes touchées ; c'est le
  coût du tableau.
- **R909 : sans objet**, et c'est l'avantage de FTS5 : l'index est dans la transaction de
  `blocks_index`, il n'y a ni intention, ni validation, ni clôture, ni reconstruction à part.

## Step 6 : verdict par exigence

| Exigence | Verdict | Chiffre |
|---|---|---|
| Licences contre `deny.toml` | **tient** | `bans ok, licenses ok` (cargo-deny 0.22.0), 145 crates |
| Version de Rust | **tient** | 1.88 demandé, 1.96 au dépôt |
| Démarrage à froid < 1 s (§5.10) | **tient** | +2,4 à 2,8 ms pour ouvrir l'index et l'écrivain (le serveur démarre en quelques dizaines de millisecondes) |
| Mémoire au repos < 150 Mo (§5.10), à la mesure existante | **tient** | +4 Mo, 2 s après l'ouverture |
| Mémoire après usage | **à trancher** (réserve 2 ; retenu par défaut : R924) | mémoire privée +5 à 20 Mo ; mémoire de travail 185 à 190 Mo après 500 recherches, par les fichiers projetés |
| R916, note sans statistique d'ensemble | **tient** | 15 cas sur 15 identiques au bit près ; témoin BM25 : 12 sur 15 changent |
| R916 et R920, préfixe par terme exact, borné à 10 caractères | **tient** | p95 ≤ 42,6 ms ; vérification de 2 000 entrées : 17 à 18 ms |
| R902, groupement par page avant `skip` et `limit` | **tient** | 1,1 million d'entrées de 10 000 pages : p95 29,6 ms, 1,4 Mo |
| R909, charge utile d'une validation | **tient** | 25 arrêts brutaux, 0 incohérence |
| Filtre par l'ensemble des pages permises, dans la requête | **ne tient pas** par `TermSetQuery` (65 à 580 ms) ; **tient dans le collecteur** (0 à 13 ms de plus) | réserve 1 |
| Budget proposé d'une recherche < 100 ms au p95 (L10) | **tient** | pire p95 42,6 ms, recherche seule, index au repos |
| Taille sur le disque | mesurée ; la condition de L18 ne se juge pas (base estimée, sans l'historique) : R931 | index 2,48 fois le texte, `blocks_index` 1,92 fois |
| Forme hostile, champ des débuts de mot | **borné pour la forme du plan seulement** | mots de 40 octets : 1,17 à 1,25 fois les octets reçus, l'index entier 3,3 fois ; mots de 10 octets (mesuré par la relecture, sur une copie du banc, non rejoué) : 4,3 fois et 6,7 fois |
| Mémoire pendant les fusions d'une forme hostile | **rien ne la borne** (risque ouvert RO-1, R928) | 802 Mo de mémoire de travail à 10 000 blocs de mots de 40 octets ; 455 Mo pour 300 blocs de mots de 10 octets (relecture) ; vraisemblablement du tas |
| Canal par le temps de réponse (L19) | **mesuré dans le processus : le canal existe** ; à travers un réseau, un calcul | 16 ns par entrée non permise |
| FTS5 compilé dans le SQLite du serveur | **oui** | par une requête de sqlx |

## Ce qui n'a pas été mesuré, et pourquoi

- **À travers un réseau** (question d) : le banc est un processus ; l'écart est donné, sa
  lecture par le réseau est un calcul.
- **À froid** : le cache de fichiers du système était chaud ; pas de moyen simple de le vider
  sous Windows sans toucher au poste. La première recherche d'un processus neuf (10 à 11 ms)
  n'est pas une lecture à froid du disque.
- **Sous charge** : latence d'une recherche pendant une fusion, pendant l'indexation, ou sous
  quatre recherches de front ; effet de l'indexation sur la frappe des autres. C'est X2, avec
  le serveur.
- **Dans le serveur** : la mesure `idle_rss_under_150mb` elle-même, avec l'index ouvert par
  `mementum serve`. Les deltas viennent d'un processus nu.
- **ARM, Pi 4, le coureur de la CI** (4 cœurs, disque lent) : rien. La durée de compilation
  sur la CI n'est pas mesurée.
- **`cargo deny` sur l'atelier du serveur** avec Tantivy ajouté : fait sur un `Cargo.toml`
  d'essai, comme le dit le Step 1 ; cargo-deny n'était pas installé sur le poste, il l'a été
  dans le dossier de la tâche.
- **Le repli de R902** (pas de titre sur les blocs) et **Tantivy sans ses fonctions par
  défaut** : pas nécessaires au verdict.
- **Une coupure de courant** pendant une validation (R909) : seuls des processus tués.
- **FTS5 avec des index de préfixes et une forme sans jointure** : pas par cette tâche ; la
  relecture les a mesurés sur sa copie du banc (Step 5), sans sortie gardée. **Une fonction de
  note sans statistique** pour FTS5, et son coût d'écriture avec index de préfixes : par
  personne.
- **La mémoire que le plan définit** (rattrapage fini, écrivain et lecteur tenus, index de
  10 000 pages), **toute lecture sous Linux**, la part de tas des formes hostiles à 10 000
  blocs et à mots de 10 octets : X2 et S3.
- **Le départage de deux notes égales** et **la pagination par deux recherches avec `skip`** :
  l'essai R916 et la « preuve » de R902 ne les éprouvent pas ; tests de S3.
- **Le Step 1 n'a pas de sortie brute** : compilation, taille du binaire, compte des crates.
- **Non vérifié par la relecture**, qui a été arrêtée : les fourchettes des passages 2 et 3 de
  `tantivy-query` ligne à ligne, le détail de `fts-write`, la cohérence des propositions avec
  R906 à R915.
- **La taille de la base d'aujourd'hui** pour ce corpus (L18) : estimée, pas mesurée (S4c).
- **La racinisation** : vingt et trente paires choisies à la main, pas un corpus.
- **Construire l'ensemble des pages permises** (une table de 100 000 identifiants) : hors des
  latences données.

## Recommandation pour la décision L1

**Tantivy**, comme le dit la conception.

Ce qu'il coûte, chiffré :
- une dépendance lourde : 65 crates neuves (dont du C, `zstd-sys`), +40 s de compilation en
  release sur 24 cœurs, +5,5 Mo de binaire ;
- un second stockage : 651 Mo pour 262 Mo de texte dans la forme du plan (404 Mo sans les
  débuts de mot), et sa cohérence (R909, qui tient) ;
- une mémoire de travail qui suit les pages de l'index lues (réserve 2), et une pointe de 370
  à 390 Mo pendant l'indexation de toute une instance ;
- une indexation lente au plus petit tampon : 3 600 entrées par seconde, six minutes pour un
  million de blocs (deux minutes avec 50 Mo) ;
- un plancher de 80 ms par validation.

Ce que FTS5 coûterait à sa place, corrigé d'après la relecture (I1) : dans sa meilleure forme
mesurée, sans note, le budget d'une recherche manqué d'un facteur 1,2 à 1,7 sur le seul pire
cas (un mot ou un préfixe très courant, pour qui voit 10 000 pages ou plus), tenu ailleurs ;
un canal par le temps environ sept fois plus large ; 20 à 90 ms de plus dans le tour
d'écriture par transaction d'index ; une note à écrire en C contre l'API de FTS5 pour tenir
R916 (son `bm25` est interdit, et coûte : 393 à 529 ms sur les mêmes requêtes) ; et une
`blocks_index` à refaire avec un `rowid`. Ce qu'il offrirait : de l'ordre de 1,4 fois moins de
disque à fonctions égales (pas cinq), aucune cohérence à tenir, aucune dépendance, une mémoire
bornée et privée. Ce que Tantivy garde pour lui : 1,5 à 7 fois plus rapide, R916 tenu par son
API publique, les écritures hors du tour d'écriture. Ce qu'il ne peut plus avancer : un
« meilleur classement » (m3). **FTS5 est un repli sérieux**, pas seulement pour une petite
instance : si Tantivy devait être écarté (sa mémoire projetée, RO-1), il resterait à écrire sa
note en C et à mesurer ses écritures avec index de préfixes.

## Ce que le plan a changé d'après les mesures

La première version de ce compte rendu proposait ici six Rulings (R921 à R926) et quatre
décisions à reprendre. Ils sont remplacés, le 2026-11-20, par le
[registre du lot 4](2026-11-19-p1-lot4-ledger.md) et par le
[plan](../plans/2026-11-19-p1-lot4-recherche.md), corrigés d'après la relecture :

| Proposition d'ici | Ce qu'elle est devenue |
|---|---|
| R921, le filtre dans le collecteur | R921, tel quel ; l'index n'a plus de champ `space_id` |
| R922, colonnes rapides et « rang stable » | R922 : le départage par une empreinte de la clé du bloc, pas par son rang (I4) |
| R922, « `title` n'a pas à être gardé » | R929 : le titre est gardé, pour vérifier un préfixe long (I5) ; l'ordre des entrées vérifiées n'est plus celui de l'index |
| R923, la note à statistiques constantes, l'extrait par programme | R923, tel quel ; R930 pour la note d'un préfixe seul (m3) |
| R924, la mémoire jugée | R924 : quelle lecture sous Linux et sous Windows, mesurée à X2 (I6) |
| R925, tampon de 15 Mo dans le serveur, 50 Mo pour `reindex` | R925 : 50 Mo partout, repli à 15 Mo (m6) |
| R926, NFC, limite 41, CJK | R926, tel quel (limite L-p) |
| L18, à reprendre avec les chiffres | R931 : les trois copies gardées, repli mesuré en S3, choix rouvert en S4c (m4) |
| L6, L10, L19 | tableau des décisions en tête du plan ; L19 : un temps de réponse plancher (R932) |
| (rien) | R927 : l'ensemble permis bâti par requête (m5) ; R928 : la forme hostile à mots courts, risque ouvert RO-1 (I2, I3) |

Deux remarques de méthode restent vraies : « avec et sans le champ des débuts de mot » a été lu
dans le détail par champ d'un seul index (`space_usage`), pas par deux constructions ; « un
million de blocs » est ici 1,19 million. Et si FTS5 était retenu malgré tout : `blocks_index`
ne peut pas rester `WITHOUT ROWID`, `MAX_INDEX_TX_BYTES` baisse d'un ordre de grandeur, et R916
demande une fonction de note en C.
