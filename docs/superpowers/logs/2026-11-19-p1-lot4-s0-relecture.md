# Lot 4, Task S0 : relecture indépendante (partielle, arrêtée par la pause du 2026-11-19)

Relecture du compte rendu [`2026-11-19-p1-lot4-s0-mesures.md`](2026-11-19-p1-lot4-s0-mesures.md) et du banc `spike/search-bench/`, à `e179419`. Le relecteur a été arrêté par la pause : ce qui suit est son rapport partiel. Rien n'en est encore porté au compte rendu, au banc ni au plan.

**Verdict provisoire : « prêt sous réserve ».** « Tantivy tient, la tâche ne rend pas `BLOCKED` » est porté par le banc. La matière donnée à Lenny pour L1 ne l'est pas tout à fait : FTS5 est mesuré dans une forme défavorable, et la forme hostile du plan n'est pas le pire cas. La règle de L1 (« Tantivy si S0 tient les budgets ») donne le même choix, mais l'arbitrage est plus serré que ce que dit le compte rendu.

## Ce qui a été relancé

Sur une copie du banc hors de l'atelier, avec deux ajouts du relecteur (une commande `fts-alt`, une option `--word-len` sur `hostile`), supprimés avec son dossier : ils sont à refaire.

- `r916` : même verdict (tout identique sous statistiques constantes, témoin changé dans 12 cas sur 15).
- `tokens`, `fts-check` : sorties identiques à `results/`.
- `fts-build` à l'échelle 1 : 498,7 / 549,3 / 114,4 Mo, les chiffres du compte rendu.
- L'index Tantivy complet n'a pas été reconstruit.

## Important

### I1. FTS5 est mesuré dans une forme qu'une variante simple corrige

- Où : `spike/search-bench/src/fts.rs:218-223` (`grouped`), compte rendu lignes 364-370 et 463-466.
- La requête du banc lit `bi` par `rowid` pour chaque ligne touchée, afin de connaître sa page. Le compte rendu écrit qu'une autre forme n'a pas été essayée et que « rien n'indique qu'elle gagnerait le facteur 10 à 30 ».
- Variante du relecteur : la page rangée dans le `rowid` (`ord << 14 | rang`), table sans contenu, pages permises en table d'entiers, même corpus, même SQLite 3.51.3, cache de 2 Mo, 12 passages. `fts3` y ajoute `prefix='2 3'`.

| Requête (p50, ms) | Forme du banc, bm25 | Variante sans note | Variante bm25 | Tantivy (p95 du compte rendu) |
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

- Ce que cela change : sans note, FTS5 manque les 100 ms d'un facteur 1,2 à 1,7 sur le seul pire cas (mot ou préfixe très courant, pour qui voit 10 000 pages ou plus). Il les tient pour un invité et pour tout le reste. « Manqué d'un ordre de grandeur » et « un canal trente fois plus large » sont à récrire (le canal : environ sept fois).
- Ce qui reste vrai : Tantivy est 1,5 à 7 fois plus rapide ; le `bm25` de FTS5 est interdit par R916 et coûteux ; une note sans statistique demande du C, à un coût non mesuré ; les écritures restent dans le tour d'écriture.
- Tailles mesurées : index FTS5 sans contenu 134,9 Mo ; avec `prefix='2 3'` 288,8 Mo, construit en 34 s.
- La comparaison de disque pèche dans l'autre sens : « cinq fois moins » compare 651 Mo (texte gardé et débuts de mot) à 114 Mo (ni l'un ni l'autre). À fonctions égales : environ 395 Mo (Tantivy sans texte gardé) contre 270 à 290 Mo (FTS5 avec préfixes courts).
- À faire : refaire le Step 5 dans cette forme, avec `--prefix` (l'option existe dans `fts-build`, aucun résultat ne l'utilise), et récrire le tableau et la recommandation.

### I2. La forme hostile du plan n'est pas le pire cas du champ des débuts de mot

- Où : plan, Step 2 de S0 et table des bornes (ligne 296) ; compte rendu lignes 193-196 et 417 ; `tv.rs:1397`.
- R920 borne par mot, pas par octet : des mots distincts de 10 caractères donnent 9 débuts pour 11 octets, contre 9 pour 41.
- Mesure, 300 blocs de 64 Kio (19,7 Mo reçus) :

| Mots de | `text_starts` | Index entier | Débit | Pointe de mémoire de travail |
|---|---|---|---|---|
| 40 octets (forme du plan) | 24,7 Mo, 1,25 fois | 3,40 fois | 1,07 Mo/s | 155 Mo |
| 10 octets | 84,2 Mo, 4,28 fois | 6,68 fois | 0,51 Mo/s | 455 Mo |

- « Tient (borné) : 1,17 à 1,25 fois » ne vaut que pour la forme demandée.
- À faire : ajouter la forme à mots de 10 caractères au banc et au plan (table des bornes, L-g), et dire 4,3 fois pour le champ et 6,7 fois pour l'index.

### I3. La pointe de mémoire pendant les fusions est du tas, pas des fichiers projetés

- Où : compte rendu lignes 198-206 (« la part privée n'a pas été relevée à part »).
- Mesure : 300 blocs, mots de 40 octets : pointe du tas 146 Mo pour 156 Mo de mémoire de travail. La passe à mots de 10 octets (455 Mo) a été interrompue par la pause : sa part de tas n'est pas établie.
- Ce que cela suggère, sans le prouver à cette échelle : les 802 Mo relevés à 10 000 blocs sont vraisemblablement de la mémoire réelle, donc un risque sur une petite machine, pas une question de comptabilité.
- À faire : relever le tas pour les formes hostiles, et regarder ce que borne la politique de fusion. Le tampon de l'écrivain ne borne pas cette pointe (tas de 167 Mo avec un tampon de 15 Mo sur le corpus ordinaire).

### I4. R922 proposé : le « rang stable » n'existe pas dans le produit

- Où : compte rendu lignes 482-490 ; `tv.rs:172` et `tv.rs:385`.
- Dans le banc, `blk` est le rang du bloc dans la page, stable parce que le corpus est figé. Dans le produit, insérer un bloc décale le rang des suivants, et R919 ne réindexe que les blocs dont l'empreinte a changé.
- À faire : départager par une valeur tirée de l'entrée seule (des bits de l'identifiant du bloc en colonne rapide, l'entrée de titre d'abord), et chiffrer ces colonnes.
- L'essai R916 ne mord pas sur ce point : un départage par `DocAddress` passerait aussi, les entrées permises ayant les mêmes adresses dans les quatre index (`small_index`, `tv.rs:1011`).

### I5. R922 dit que `title` n'a pas à être gardé, alors que R920 vérifie « le champ touché » sur le texte gardé

- Où : compte rendu ligne 489 ; plan, R920 (ligne 140) et paragraphe « Où » (ligne 217).
- Un préfixe de plus de 10 caractères trouvé par `title_starts` n'a plus de texte à vérifier. Le banc ne vérifie que `text` (`tv.rs:843`).
- À trancher : garder le titre dans l'index (taille à mesurer, il est recopié sur chaque bloc), ou ne vérifier le titre que sur l'entrée de titre, lu ailleurs.
- R920 dit aussi « dans l'ordre de l'index » pour les 2 000 entrées vérifiées : cet ordre dépend des fusions, donc de l'activité des autres, ce que R922 veut proscrire. À accorder.

### I6. Réserve mémoire (R924) : bien posée sur le fond, il lui manque trois choses

- Le fond tient : +4 Mo à l'ouverture, 185 à 190 Mo après 500 recherches pour 20 à 24 Mo privés (`results/tantivy-open-rest.txt`), et `idle_rss_under_150mb` (`crates/mementum-cli/tests/budgets.rs:77`) part d'un dossier vide, donc ne voit aucun index.
- Manque 1 : la mesure que X2 définit (« une fois le rattrapage fini », écrivain et lecteur tenus, index de 10 000 pages). Le banc mesure « écrivain lâché » (34 Mo de mémoire de travail, et 42 Mo privés que le compte rendu ne cite pas, `tantivy-build-50.txt:1`) ou « pas de rattrapage ».
- Manque 2 : Linux. La CI y lit le RSS, fichiers projetés compris. La « mémoire privée » du banc est `virtual_memory()` de sysinfo : la mémoire privée sous Windows, mais la taille virtuelle sous Linux. Le critère de R924 demande une autre lecture là-bas (`RssAnon`).
- Manque 3 : dire à Lenny que c'est surtout une question de comptabilité. SQLite passe par le même cache de fichiers du système sans qu'il soit compté au processus.
- « Tantivy n'offre pas d'autre lecture du disque » est vrai des répertoires fournis ; `Directory` est un trait public, et un répertoire à nous n'a pas été examiné.

## Mineur

- **m1.** Le Step 1 n'a aucune sortie brute dans `results/` (« bans ok, licenses ok », 40 s de compilation, +5,5 Mo de binaire, 65 crates). `tantivy-licences.txt` recoupé avec `deny.toml` : chaque ligne a une option admise. La CI lance `cargo deny check` entier ; S0 n'a passé que `licenses bans`.
- **m2.** La « preuve » de pagination de R902 (`tv.rs:821-823`) coupe en deux une même liste déjà dédoublonnée : elle passerait avec toute implémentation. Ce n'est pas deux recherches avec `skip`.
- **m3.** Un préfixe a une note constante (`Basic`, sans normes, `tv.rs:57`). Or R911 fait du dernier terme un préfixe : une recherche d'un seul mot range ses résultats hors titre par identifiant de page. Avec L6 et R916, l'argument « meilleur classement » de L1 est affaibli ; le compte rendu ne le dit pas.
- **m4.** L18 : la base « de 400 à 550 Mo » est une estimation qui ignore les versions de l'historique. La conclusion « ne tient vraisemblablement pas » est fragile dans les deux sens.
- **m5.** L'ensemble permis est dit « bâti une fois » (ligne 127) ; le plan le bâtit à chaque requête. Son coût pour 100 000 identifiants est à ajouter, ou un cache par `rev` à prévoir.
- **m6.** R925 : les pointes sont voisines (tas 167 contre 212 Mo), donc garder 15 Mo dans le serveur coûte trois fois le temps pour 45 Mo. Justification mince.
- **m7.** Question (d) : le plan demande si l'écart « se distingue du bruit d'un appel par le réseau local » ; la réponse est un calcul. La conclusion (le canal existe) est prudente.

## Ce qui tient, vérifié

- Tous les chiffres Tantivy recoupés sont dans `results/` : ouverture, repos, validations, tailles, formes hostiles, R916, R909, canal, pire p95 de 42,63 ms, R920.
- Le corpus est honnête pour les latences : « de » touche 695 569 entrées, « pr » 1 156 705 sur 1 290 901.
- R916 par statistiques constantes : l'API existe, `SnippetGenerator` lit bien `doc_freq`, et le témoin mord sur les requêtes à terme.
- Le filtre dans le collecteur (R921) est juste.
- R909 côté Tantivy tient, par construction autant que par l'essai.

## Passages du plan à changer (non exhaustif)

- « La requête et les droits », point 4 : « filtre de la requête » devient un filtre du collecteur (R921).
- Paragraphe « Où », ligne 217 : ajouter les colonnes rapides ; trancher `title` gardé ou non (I5) ; `space_id` ne sert plus de filtre.
- R920 : « dans l'ordre de l'index » (I5).
- Table des bornes, ligne 296, et Step 2 de S0 : la forme hostile à mots courts (I2).
- L18 et L19 : aux chiffres mesurés.
- X2 : dire quelle mémoire est jugée (I6).

## Non vérifié

- Les fourchettes des passages 2 et 3 de `tantivy-query`, ligne à ligne.
- Le détail de `fts-write`, R926 au-delà de la sortie `tokens`, la cohérence des Rulings avec R906 à R915.
- `cargo deny` lui-même, la durée de compilation, la taille du binaire, les 65 crates.
- La mémoire de l'index complet : après rattrapage, sous Linux, à froid, sous charge.
- La part de tas de la forme hostile à mots de 10 octets et à 10 000 blocs.
- FTS5 avec une note sans statistique, et son coût d'écriture avec index de préfixes.
- La variante FTS5 suppose un ordinal entier par page et 14 bits de rang par bloc : sa tenue dans le produit (rangs stables, suppression dans une table sans contenu) n'est pas étudiée.
