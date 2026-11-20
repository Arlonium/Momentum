# Lot 5, Task S0 : mesurer avant de bâtir (transport MCP, écriture par différence, contenu en ligne, restriction, version d'avant un agent)

Compte rendu de la Task S0 du plan [`2026-11-20-p1-lot5-agents.md`](../plans/2026-11-20-p1-lot5-agents.md).
Mesuré le 2026-11-20 sur le poste de développement (Windows 11, 24 cœurs, disque NVMe), en
`--release`, une commande lourde à la fois. Aucun code du produit : le banc est dans
[`spike/agents-bench/`](../../../spike/agents-bench/) (son propre atelier Cargo, qui dépend des
crates du produit par chemin), ses sorties brutes dans `spike/agents-bench/results/`. Rien n'a
été lancé sous Linux. Les Rulings de la tâche (R1250 à R1258) sont au
[registre du lot 5](2026-11-20-p1-lot5-ledger.md), sous la forme du projet ; ce compte rendu en
donne les chiffres.

## Verdict

1. **MCP (R1236 tient, les deux révisions restent : L17 confirmée).** Un vrai Claude Code
   (2.1.329) s'est branché neuf fois au `/mcp` du banc, sans état, réponses JSON, jeton en
   `Authorization`. Il parle la **2026-07-28** par défaut (sonde `server/discover`, puis
   `resources/list`, `tools/list`, `tools/call`, chaque requête avec sa version dans `_meta` et
   les en-têtes `MCP-Protocol-Version`, `Mcp-Method`, `Mcp-Name`) ; il n'ouvre ni session, ni
   flux `GET`, ni `subscriptions/listen` quand le serveur n'annonce pas `listChanged`. Devant un
   serveur qui ne parle que la 2025-11-25, il retombe de lui-même sur `initialize`. Un client
   ancien (`MCP_PROTOCOL_NEGOTIATION=legacy`, ce que parlent les Claude Code d'avant le
   runtime v2 et les clients sur le SDK 1.x) **échoue** devant un serveur qui ne sert que la
   2026-07-28 : la 2025-11-25 ne se retire pas. `${VAR}` est développé dans un en-tête d'une
   configuration (`${BANC_TOKEN}`, `${MEMENTUM_TOKEN}`).
2. **Écriture par différence (R1227) : pas de seuil de repli par part de blocs.** Sur une page
   de 10 000 paragraphes, la différence coûte 47 à 51 ms pour un bloc changé, inséré, retiré ou
   déplacé (contre 625 à 650 ms pour `rewrite_blocks`), 62 ms pour la moitié changée, 75 ms pour
   tous ; elle ne rejoint `rewrite_blocks` que quand tout l'ordre est renversé (640 contre
   654 ms) et reste deux fois plus rapide quand la page double par insertions (2,1 s contre
   4,3 s). **À une condition : deux transactions, les suppressions d'abord** (la forme de R367).
   En une seule, quand le contenu retiré a été écrit par le même client (l'acteur qui a déjà
   écrit la page), l'ordre renversé coûte 2,3 s, et une page de paragraphes à six plages
   entièrement changée 950 ms, plus que `rewrite_blocks` (700 ms) ; en deux, 650 et 534 ms. Un
   bloc intact n'est jamais touché : vérifié élément Yjs par élément Yjs, sur la page de 10 000
   paragraphes et sur chaque type de bloc de la fixture `all-blocks`.
3. **Contenu en ligne (R1227) : par préfixe et suffixe communs, dans un seul texte.** Sur
   9 scènes (gras, frontière d'une marque, lien, après une mention, marques seules, suppression
   à cheval sur deux marques, en tête, lien posé, lien retiré) et 198 combinaisons avec des
   frappes concurrentes du vrai Yjs (13.6.33, celui du client), l'écriture se lit comme voulu,
   **toutes** les frappes concurrentes hors du texte remplacé sont gardées (90 sur 90), et le
   vrai Yjs lit ce que lit yrs **dans 197 combinaisons sur 198**. La seule différence : un lien
   retiré, une frappe juste après lui, l'écriture appliquée avant la frappe ; Yjs nettoie alors
   de lui-même des marques devenues inutiles (`cleanupYTextAfterTransaction`, une transaction
   locale qu'un navigateur renvoie au serveur), et yrs, ce nettoyage reçu, lit comme lui. Le
   contenu récrit en entier (R1227 tel qu'écrit) perd toute frappe concurrente du bloc (72 sur
   72) ; il est 3 fois plus rapide sur une page entière (64 contre 177 ms pour 5 000 blocs
   changés) et reste loin sous `rewrite_blocks`.
4. **Restriction (R1217) : ne pas restreindre tout l'espace pour juger une page.** Le prototype
   direct de `narrowed` coûte 2,2 ms (p95 2,5) sur 10 000 pages et 30 ms (p95 31) sur
   100 000, presque autant que le calcul du compte (2,7 et 37 ms), quelle que soit la portée ;
   le `404` d'une page hors portée coûte donc 2 à 30 ms de plus que celui d'une page inconnue
   (rien). Remonter les parents visibles de la page jugée coûte 10 à 20 µs aux deux tailles.
   Avec le rang du parent visible gardé dans le calcul du compte, la restriction entière tombe à
   0,15 ms (p95 0,35) et 2 ms (p95 2,4).
5. **Version d'avant un agent (R1234) : hors de l'acteur dès S4.** Pour la page de 10 000
   paragraphes des budgets : encodage 7,7 ms, `check_state` 112 ms, écriture 29 ms (p95 39) ;
   les trois de suite 147 ms (p95 160). Pour 10 000 paragraphes à six plages : 19, 249 et
   38 ms, 306 ms de suite. Tenue par l'acteur, une frappe qui tombe pendant sort de son budget
   de 150 ms.

## Versions, poste, méthode

- yrs 0.28.1 du serveur (`vendor/yrs`, avec son correctif), le même paquet que celui de
  `mementum-crdt` ; Yjs 13.6.33 (le verrou de pnpm du client) ; Claude Code 2.1.329, runtime
  MCP « v2 » (son journal : `mcp runtime arm: v2 (source: default)`) ; Node 24.22.3 ;
  `mementum-acl` et `mementum-store` de `main` à `673d8b1`.
- Le prototype de `write_diff` (`src/diff.rs`) écrit avec les fonctions du produit
  (`write_nodes`, `write_inline`, `write_table`, `keep_ids`), incluses par chemin. Il ne sait
  écrire qu'une page « propre » (chaque élément Yjs d'une liste répond à un bloc de
  `read_blocks`) ; ce que `read_blocks` normalise reste à S2.
- Chaque mesure est répétée (n donné), p50, p95 et pire au plus proche rang. Avec n = 3 à 11, le
  p95 est le pire ou presque. Aucune compilation de la tâche ne tournait pendant les mesures ;
  d'autres sessions du poste ont pu tourner. Les cas mesurés deux fois (un bloc changé, la moitié,
  le même client) diffèrent de moins de 5 % d'un passage à l'autre.
- Chaque écriture par différence mesurée est vérifiée dans le même passage : la page se relit
  comme les blocs voulus (MMD sans identifiants), la page relue de son état de départ et de la
  mise à jour est la page vivante, et le nombre de blocs dont l'identité Yjs (élément, nœud de
  contenu, ses enfants) a changé est exactement celui des blocs que l'écriture dit avoir
  récrits.

Refaire : `spike/agents-bench/README.md`.

## Step 1 : le transport, dans les deux révisions

### Ce que dit la spécification (lu le 2026-11-20)

Lu : le journal des changements, le transport Streamable HTTP, le versionnage, `server/discover`,
les pages outils, ressources, cache et autorisation de la 2026-07-28, son `schema.ts` ; le
transport, le cycle de vie et les ressources de la 2025-11-25 ; la page MCP et la référence de la
ligne de commande de Claude Code. Ce que le plan ne disait pas, ou disait autrement :

- **Codes.** Une version non servie : `400`, `-32022` (`UnsupportedProtocolVersion`), `data`
  `{supported: [...], requested}`. Un en-tête absent, mal formé ou qui ne dit pas ce que dit le
  corps : `400`, `-32020` (`HeaderMismatch`) ; l'absence de `MCP-Protocol-Version` en est un.
  Un champ exigé de `_meta` qui manque (`io.modelcontextprotocol/clientCapabilities`, exigé à
  chaque requête, `{}` admis) : `400`, `-32602`. Une méthode inconnue : `404`, `-32601`. Un outil
  inconnu : `-32602` (une erreur du protocole ; un échec de l'outil est un résultat
  `isError: true`). Une ressource inconnue : `-32602` avec `data.uri` en 2026-07-28, `-32002`
  en 2025-11-25.
- **`ttlMs` et `cacheScope`** sont exigés sur `server/discover` aussi, pas seulement sur
  `tools/list`, `resources/list` et `resources/read` (R1236 les oubliait sur `server/discover`).
- **Un serveur des deux ères** lit l'ère au corps de la requête ; `initialize` ne passe pas par
  la vérification des en-têtes de la 2026-07-28 (un client 2025-11-25 n'envoie
  `MCP-Protocol-Version` qu'après lui). C'est ce que R1236 disait.
- **Un client des deux ères** tente d'abord une requête moderne : un `400` qui porte une erreur
  moderne (`-32022`, `-32021`, `-32020`) le fait réessayer selon `data.supported` ; un corps vide
  ou une autre erreur le fait retomber sur `initialize`.
- **Autorisation.** La page ne définit qu'OAuth 2.1 (un serveur protégé doit publier ses
  métadonnées, RFC 9728) et laisse libre toute autre authentification ; un jeton invalide ou
  échu reçoit `401`. Elle ne dit rien d'un jeton porteur sans OAuth.
- **Claude Code** développe `${VAR}` et `${VAR:-défaut}` dans `url` et `headers` (la page le dit
  pour `.mcp.json`) ; une variable sans valeur ni défaut reste écrite telle quelle. Avec un
  `headers.Authorization` configuré, un `401` à la connexion est un échec, sans repli sur OAuth.

### Ce que fait Claude Code

Neuf lancements de `claude -p` en mode non interactif, sur `127.0.0.1:8103`, configuration dans
un fichier du dossier de la tâche (`--mcp-config … --strict-mcp-config --tools ""
--allowedTools mcp__banc__echo --no-session-persistence --model haiku`), jeton factice
`mmt_banc_factice` lu dans une variable. Aucun `claude mcp add` ; `~/.claude.json` ne porte
aucune trace du banc après les essais. Requêtes reçues : `results/mcp/*.requetes.jsonl`.

| Essai | Banc | Ce que Claude Code envoie | Issue |
|---|---|---|---|
| 1 | les deux | `server/discover` (id `server-discover-probe-1`), `resources/list`, `tools/list`, `tools/call` ; chaque requête en 2026-07-28, avec `_meta` (version, `clientInfo`, `clientCapabilities`), `MCP-Protocol-Version: 2026-07-28`, `Mcp-Method`, `Mcp-Name: echo` sur `tools/call` ; `Accept: application/json, text/event-stream` | « bonjour » |
| 2 | 2025-11-25 seule (`server/discover` → `200`, `-32601`) | la sonde `server/discover`, puis `initialize` (`protocolVersion: 2025-11-25`), `notifications/initialized` (`202`), **`GET /mcp`** (`405`, accepté), `tools/list`, `resources/list`, `tools/call`, chacune avec `MCP-Protocol-Version: 2025-11-25` | « bonjour » |
| 3 | 2026-07-28 seule | comme l'essai 1 | « bonjour » |
| 4 | 2026-07-28 seule, client forcé en 2025-11-25 (`MCP_PROTOCOL_NEGOTIATION=legacy`) | `initialize` → `400`, `-32022`, `supported: ["2026-07-28"]` | **échec de la connexion** (« Connection failed (400) ») ; l'outil manque au modèle |
| 5 | les deux, client forcé en 2025-11-25 | comme l'essai 2, sans la sonde | « bonjour » |
| 6 | les deux, autre jeton attendu | `server/discover` → `401`, puis `initialize` → `401` ; aucune adresse `/.well-known/…` demandée | échec : « Server rejected the configured Authorization header (HTTP 401) … OAuth fallback is disabled when headers.Authorization is set » ; le corps du `401` n'atteint pas le modèle |
| 7 | les deux, `tools/call` → `401` (un jeton révoqué en cours de session) | la connexion passe ; `tools/call` reçoit `401` | le modèle lit le message de Claude Code, pas celui du serveur : « MCP server "banc" rejected the Authorization header in its config (update it, then run /mcp to reconnect) » |
| 8 | les deux, `tools/call` → `400`, `-32020` | `tools/call`, puis de lui-même `tools/list` et `tools/call` de nouveau | le modèle lit le message de l'erreur JSON-RPC tel quel |
| 9 | les deux, `${MEMENTUM_TOKEN}` dans l'en-tête de la configuration | comme l'essai 1 ; le jeton reçu fait 16 caractères | « bonjour » |

Ce qui ne s'est jamais vu : `Mcp-Session-Id`, `ping`, `subscriptions/listen`, un lot JSON-RPC,
une notification en 2026-07-28, un `GET` ou un `DELETE` en 2026-07-28. Le journal de Claude Code
le dit pour le flux : « no auto-opened subscriptions/listen on a modern connection; … (no-op if
the server advertises no listChanged capability) ». `resources/list` est demandé à **chaque
connexion**, dans les deux ères. Le `_meta` de `tools/call` porte aussi `progressToken` et
`claudecode/toolUseId`, que le serveur doit ignorer. Claude Code ne lit le corps d'aucun `401` au
modèle : il dit son propre message (essais 6 et 7).

### Verdict

- **R1236 tient** : sans état, un message par `POST`, réponse `application/json`, ni session ni
  flux ; aucun client essayé n'a exigé ce que R1236 écarte. Le SDK officiel n'est pas
  nécessaire.
- **Les deux révisions restent (L17 confirmée)** : Claude Code d'aujourd'hui n'utilise que la
  2026-07-28 et retombe seul sur la 2025-11-25 ; mais un client qui ne parle que la 2025-11-25
  (Claude Code forcé en `legacy`, donc les versions d'avant le runtime v2 et les clients bâtis
  sur le SDK 1.x) ne se branche pas sur un serveur qui ne sert que la 2026-07-28 (essai 4).
- **M1 corrige sa table de cas** : les codes ci-dessus, `ttlMs` et `cacheScope` sur
  `server/discover`, `GET` en `405` (le client 2025-11-25 l'accepte), les clés inconnues de
  `_meta` ignorées, `resources/list` servi dans les deux ères (R1251).
- **Le refus d'un jeton** se lit dans le journal de Claude Code, pas par le modèle : la recette
  (point 8) jugera le message de Claude Code ; le corps de `401 invalid_token` reste pour les
  autres clients (R1252).
- **La commande prête à coller** : `${MEMENTUM_TOKEN}` est développé à chaque lancement quand une
  configuration le porte (essai 9). Ce que `claude mcp add` écrit d'un en-tête entre
  apostrophes n'a pas été essayé (la tâche l'interdisait) : la fin du lot le vérifie avec le cas
  d'Alex (R1253).

## Step 2 : l'écriture par différence

Page de 10 000 paragraphes (celle des budgets, état de 3,19 Mo), écrite par un autre client que
celui qui la récrit (une page relue de son état), MMD sans ancres (`keep_ids` donne les
identifiants ; il est compté dans la durée). `rewrite_blocks` est mesuré sur la même page, trois
fois. Une transaction, contenu en ligne récrit en entier, sauf mention.

| Cas | Différence (n=7) | Octets | Unités Yjs | `rewrite_blocks` (n=3) | Octets |
|---|---|---|---|---|---|
| un bloc changé | p50 48,8 / p95 50,1 ms | 129 | 79 | p50 625 / p95 626 ms | 3 187 144 |
| un bloc changé, deux transactions | 49,5 / 49,9 ms | 131 | 79 | 638 / 648 ms | |
| un bloc changé, par préfixe et suffixe | 48,8 / 49,8 ms | 46 | 9 | 630 / 636 ms | |
| un bloc inséré | 48,9 / 50,8 ms | 263 | 20 | 640 / 646 ms | 3 187 397 |
| un bloc retiré | 48,7 / 50,1 ms | 15 | 0 | 645 / 647 ms | 3 186 815 |
| un bloc déplacé (du rang 100 au rang 9 000) | 47,1 / 51,0 ms | 330 | 75 | 648 / 653 ms | 3 187 134 |

Où passe le temps de la différence (un passage) : lecture des blocs 21 à 24 ms, `keep_ids` 13 à
15 ms, plan 8 à 10 ms, écriture sous 1 ms. Dans l'acteur, la lecture se partage avec celle que
l'opération fait déjà (`Hub::read_page`), et `keep_ids` ne sert qu'à un MMD sans ancres.

**L'échelle** (n=3 ; `rewrite_blocks` : 636 à 656 ms partout pour les blocs changés) :

| Part de blocs changés (texte) | Différence p50 | Octets | | Part de blocs insérés | Différence p50 | `rewrite_blocks` p50 |
|---|---|---|---|---|---|---|
| 1 % | 51,8 ms | 10 905 | | 0,1 % | 50,5 ms | 644 ms |
| 2 % | 51,2 ms | 21 791 | | 1 % | 52,2 ms | 651 ms |
| 5 % | 52,9 ms | 54 735 | | 5 % | 77,8 ms | 735 ms |
| 10 % | 52,5 ms | 109 657 | | 10 % | 116 ms | 881 ms |
| 20 % | 57,1 ms | 219 500 | | 25 % | 271 ms | 1 245 ms |
| 33 % | 59,2 ms | 365 923 | | 50 % | 703 ms | 2 076 ms |
| 50 % | 62,4 ms | 549 037 | | 100 % | 2 095 ms | 4 280 ms |
| 100 % | 74,5 ms | 1 098 260 | | | | |

Un bloc changé ne touche que son nœud de contenu ; une insertion reparcourt la liste des frères
(yrs), d'où la croissance, que `rewrite_blocks` paie aussi, en plus. **L'ordre renversé** (9 999
blocs recréés au même identifiant) : 640 ms (p95 647) contre 654 ms, 3 186 850 octets contre
3 187 166 ; en deux transactions, 666 ms.

**Le contenu retiré écrit par le même client** (la page écrite puis récrite par le même
`PageDoc`, comme un acteur qui a déjà écrit) : le ramasse-miettes de R367 joue.

| Page, cas | Une transaction | Deux transactions | Deux, par préfixe | `rewrite_blocks` |
|---|---|---|---|---|
| 10 000 simples, moitié changée | 63,7 ms | 66,0 ms | 182 ms | 633 ms |
| 10 000 simples, tout changé | 75,5 ms | 77,2 ms | 323 ms | 643 ms |
| 10 000 simples, ordre renversé | **2 333 ms** | 652 ms | 656 ms | 634 ms |
| 10 000 à six plages, moitié changée | 515 ms | 411 ms | 419 ms | 798 ms |
| 10 000 à six plages, tout changé | **950 ms** | 534 ms | 211 ms | 701 ms |

**La fixture `all-blocks`** (28 blocs de premier niveau, 35 en tout), avec et sans identifiants
dans les blocs écrits : un changement de chaque type de bloc qui en admet un (texte, cellule de
tableau, prop texte des blocs sans contenu : image, vidéo, son, fichier, équation, signet, lien
de page, base, bloc synchronisé, bloc inconnu), une insertion, un retrait, un déplacement, la
moitié changée, le texte d'un bloc d'une colonne, un bloc ajouté à une colonne, un sous-bloc
changé, ajouté, tous retirés, sorti au premier niveau. Chaque écriture se relit comme voulu, la
page relue est la page vivante, et seuls les blocs récrits changent d'identité Yjs (un changement
de props : aucun) ; 0,23 à 0,35 ms, 11 à 2 727 octets, contre 0,45 à 0,59 ms et 10 400 octets pour
`rewrite_blocks`. Sans identifiants, le sous-bloc sorti au premier niveau reçoit un identifiant
neuf (`keep_ids` ne suit pas un changement de parent : cas 3 de R1227, LA-a).

### Verdict (R1254)

- **Pas de seuil par part de blocs touchés.** Dans tous les cas mesurés, la différence en deux
  transactions coûte au plus `rewrite_blocks` plus la lecture et le plan (une cinquantaine de
  millisecondes pour 10 000 blocs) ; elle ne le rejoint que pour un ordre entièrement renversé.
  Un seuil ferait perdre toute frappe concurrente sans rien faire gagner.
- **Toujours deux transactions** : les suppressions (blocs retirés ou déplacés, contenus en
  ligne retirés), puis les écritures. Une transaction perd jusqu'à un facteur 3,6 dès que le
  contenu retiré est du même client.
- **Le repli sur `rewrite_blocks`** ne garde qu'un rôle : une page que la différence ne sait pas
  lire élément pour bloc (ce que `read_blocks` normalise : une seconde racine, un contenu hors de
  la première `blockGroup`, une liste de colonnes défaite, des blocs aplatis au-delà de
  `MAX_DEPTH`, un nœud étranger hors conteneur). `rewrite_blocks` retire alors ce que
  `read_blocks` ne montre pas, comme R1227 le demande, et la limite LA-a le dit.
- `write_cost` (et le `diff_cost` de S2) reste la borne d'une écriture : une insertion
  reparcourt la liste des frères, et 10 000 insertions coûtent 2 s.

## Step 3 : le contenu en ligne

`src/diff.rs`, mode `Affix` : le contenu courant et le contenu voulu, lus en unités (un caractère
et ses marques, ou un élément en ligne : mention, formule, saut de ligne) ; le préfixe et le
suffixe communs ; si le milieu changé ne porte aucun élément en ligne, des deux côtés, et tient
dans un seul `XmlText`, il est retiré puis récrit à sa place, plage par plage, avec ses marques
explicites (comme `write_inline`) ; sinon le contenu est récrit en entier. Une marque seule qui
change (gras posé, lien retiré) récrit les caractères qu'elle couvre.

La sonde (`probe/inline.mjs`, Node, le vrai Yjs 13.6.33 du client) part d'une page écrite par le
serveur (« Du **gras** au milieu, un [lien](…) et <mention> fin. », puis un second paragraphe
que rien ne change), fait les deux écritures du serveur (`affix`, `whole`) sur cette page, et
cinq frappes du vrai Yjs sur l'état de départ : au début du texte (`prefix`), à la fin, après la
mention (`suffix`), dans le second paragraphe (`untouched`), et contre les bornes de ce que
l'écriture retire (`before`, `after`). Chaque frappe est appliquée dans les deux ordres ; yrs
(`inline-apply`) et Yjs relisent chaque combinaison, plage par plage (le texte de `diff` et de
`toDelta`, jamais `get_string`).

| | Combinaisons | Yjs lit comme yrs | Écriture lue comme voulu | Frappes gardées |
|---|---|---|---|---|
| Par préfixe et suffixe | 99 | 98 (et la 99ᵉ après le nettoyage de Yjs) | 9 / 9, et intacte autour de chaque frappe | 90 / 90 |
| Récrit en entier | 99 | 99 | 9 / 9 | 18 / 90 (celles du second paragraphe seulement) |

Octets d'une écriture : 15 à 135 par préfixe et suffixe, 269 à 447 en entier. Les marques d'une
frappe sont celles de sa place (gras dans le gras, lien dans le lien), comme dans l'éditeur.

**L'écart** (scène « lien retiré », frappe « after », l'écriture appliquée avant la frappe) : yrs
lit la frappe sous le lien, Yjs sans lui. L'écriture laisse des marqueurs de format inutiles ; le
vrai Yjs les retire de lui-même à la réception d'une mise à jour distante
(`cleanupYTextAfterTransaction`, une transaction **locale**, que y-websocket renvoie au serveur),
yrs non. Donnée à yrs, cette transaction de Yjs le ramène à la lecture de Yjs (vérifié pour les
dix combinaisons de la scène où Yjs a nettoyé). C'est le chemin de deux personnes qui touchent
aux marques en même temps ; la page converge dès que le navigateur a renvoyé son nettoyage, et
chaque navigateur qui reçoit une telle écriture en renvoie un.

Coût sur la page de 10 000 paragraphes : un bloc, comme en entier (48,8 ms) ; la moitié des
blocs, 177 ms (p95 180) contre 64 ms en entier (écriture 102 ms contre 11 ms) ; tous, en deux
transactions et contenu du même client, 323 contre 77 ms.

### Verdict (R1255)

Le contenu en ligne s'écrit **par préfixe et suffixe communs** quand le milieu changé tient dans
un seul texte et ne porte aucun élément en ligne, **en entier** sinon. Il garde la frappe
concurrente d'un bloc changé hors du texte qu'il remplace ; il coûte jusqu'à trois fois
l'écriture en entier sur une page entièrement récrite, toujours sous `rewrite_blocks`. S2 prouve
la règle contre le vrai Yjs (`diffWrites.test.ts`), et son test d'une marque changée près d'une
frappe concurrente rejoue le nettoyage de Yjs avant de comparer. Un changement de marques seules
pourrait s'écrire par `format` (ce que fait y-prosemirror) plutôt que par suppression et
écriture : non mesuré, laissé à S2.

## Step 4 : la restriction

Un espace de 10 000 puis 100 000 pages (1 % de racines, chacune avec la ligne `members`, les
autres sous une page tirée parmi les 200 plus récentes, 0,2 % à la corbeille), vu en entier par un
membre. `SpaceAccess` gardant ses nœuds privés, les prototypes partent de `visible()`, comme le
ferait `narrowed` dans `mementum-acl`, et bâtissent la liste restreinte triée. n=200 (n=50 pour
les calculs du compte et des rangs).

| | 10 000 pages | 100 000 pages |
|---|---|---|
| calcul du compte (`SpaceAccess::compute`, gardé par révision) | p50 2,75 / p95 3,03 ms | 37,1 / 38,7 ms |
| restriction sans portée de pages (niveau, interdits ; une copie) | 0,02 / 0,12 ms | 0,88 / 1,04 ms |
| restriction à des pages, mémo dans une table de hachage (1 page : feuille, milieu, racine ; 50 pages) | 2,09 à 2,25 / 2,30 à 2,58 ms | 29,8 à 30,5 / 30,8 à 31,9 ms |
| la même, mémo dans un tableau, parents par dichotomie | 0,98 à 1,05 / 1,18 à 1,28 ms | 17,0 à 17,5 / 18,0 à 18,7 ms |
| rangs des parents visibles, calculés une fois avec le compte | 0,74 / 0,94 ms | 11,8 / 12,4 ms |
| la restriction, rangs des parents gardés | 0,11 à 0,17 / 0,17 à 0,35 ms | 1,89 à 2,07 / 2,08 à 2,41 ms |
| `404` d'une page vue hors portée, par la restriction entière | 2,05 à 2,20 / 2,25 à 2,50 ms | 29,8 à 30,8 / 30,9 à 32,1 ms |
| la même, en remontant les parents visibles de la page | 0,00 à 0,01 / 0,01 ms | 0,01 à 0,02 / 0,02 ms |
| `404` d'une page inconnue (une recherche dans le calcul gardé) | 0,00 ms | 0,00 ms |

Le coût ne dépend pas de la portée (1 page gardée ou 9 201) : il est celui du parcours de toutes
les pages vues.

### Verdict (R1256)

- **Une route d'une page** (`node_access`, `seen_node` : lire, écrire, la corbeille, les
  opérations) ne restreint pas tout l'espace : elle juge la page par la remontée de ses parents
  visibles jusqu'à une racine de la portée (10 à 20 µs), puis ramène le niveau sous celui du
  jeton et pose l'interdit. Le `404` d'une page hors portée coûte alors 10 à 20 µs de plus que
  celui d'une page inconnue, au lieu de 2 à 30 ms : l'écart de temps que R1217 ajoutait à D12
  du lot 4 tombe d'un facteur cent à mille.
- **Une route de tout l'espace** (l'arbre, l'ensemble des pages permises de la recherche,
  `workspace_info`) restreint tout : avec le rang du parent visible gardé dans `SpaceAccess`
  (4 octets par page, calculés avec le compte et gardés en cache avec lui), 0,35 ms au p95 sur
  10 000 pages, sous le seuil de 1 ms de R1217 : pas de cache de la restriction.
- Sans portée de pages, la restriction n'est qu'un plafond de niveau et un interdit : S5 peut les
  lire au passage plutôt que de recopier la liste.
- `SpaceAccess::compute` coûte 37 ms sur 100 000 pages ; les rangs lui ajouteraient 12 ms tels
  que mesurés ici (une dichotomie par page), moins s'ils viennent de la table que `compute` bâtit
  déjà. S5 mesure dans le serveur.

## Step 4 bis : la version d'avant un agent

Les trois gestes de R1234, de suite, comme l'acteur les ferait : `encode_state`,
`PageDoc::check_state`, `Store::put_version` (raison `before_restore`, la plus proche de
`before_agent` ; base SQLite du banc, `synchronous = FULL`, sur le NVMe du poste). n=11.

| Page | État | Encodage | `check_state` | Écriture | Les trois |
|---|---|---|---|---|---|
| 1 000 paragraphes | 316 Ko | 0,75 / 0,78 ms | 9,4 / 9,9 ms | 3,9 / 13,8 ms | 14,0 / 23,9 ms |
| 10 000 paragraphes (page des budgets) | 3,19 Mo | 7,7 / 8,0 ms | 112 / 114 ms | 29,1 / 38,8 ms | **147 / 160 ms** |
| 10 000 paragraphes à six plages | 4,97 Mo | 19,1 / 20,3 ms | 249 / 255 ms | 37,5 / 41,4 ms | **306 / 313 ms** |

R816 disait 81 ms pour la relecture, R367 et R1234 8 ms d'encodage et 28 ms d'écriture : la
relecture vaut ici 112 ms sur la même page (un autre jour, un poste chargé d'autres ateliers),
l'encodage et l'écriture sont retrouvés.

### Verdict (R1257)

La relecture et l'écriture d'une `before_agent` sortent de l'acteur **dès S4**, comme la
compaction (R826) : 147 à 306 ms de commande de l'acteur font attendre toute frappe qui tombe
pendant au-delà de son budget de 150 ms (une page de 10 000 blocs n'est pas un pire cas : la
page à six plages double le coût). L'acteur n'encode que l'état (8 à 19 ms) ; la relecture et
l'écriture se font sur un fil bloquant ; l'opération de l'agent ne s'applique qu'une fois la
version écrite, dans une seconde commande qui revérifie que ce qu'elle remplace est encore ce
que la version garde (sinon elle recoupe, ou rend `409 version_refused`). S4 tient la forme
exacte et la prouve par la mesure de la frappe qui tombe pendant
(`measure_before_agent_version_of_a_10000_block_page`).

## Ce qui n'a pas été mesuré

- **Linux** : rien. Les quatre cœurs et le disque de la CI ne sont jugés qu'à la poussée et à la
  fin du lot.
- **Un autre client MCP que Claude Code**, et un vrai Claude Code d'avant le runtime v2 : le
  second est approché par `MCP_PROTOCOL_NEGOTIATION=legacy` du même binaire.
- **`claude mcp add`** et ce qu'il écrit d'un en-tête entre apostrophes ou entre guillemets : la
  tâche interdisait d'écrire dans la configuration de Claude Code de l'utilisateur. La page de
  Claude Code dit `${VAR}` développé dans `headers` ; l'essai 9 le prouve pour un fichier donné
  par `--mcp-config`. Une variable absente (gardée telle quelle selon la page) n'a pas été
  essayée.
- **`resources/read`, `Mcp-Name` encodé en base64 par le client, une sortie de plus de
  25 000 jetons** : Claude Code n'en a pas eu besoin dans ces essais ; le banc les sert (essai de
  la sonde `selftest.jsonl`).
- **L'écriture par différence d'une page qui porte ce que `read_blocks` normalise** : le
  prototype retombe sur `rewrite_blocks` ; ce que fera le vrai `write_diff` est à S2.
- **Une frappe concurrente du vrai Yjs pendant une écriture par différence d'une page entière** :
  la sonde ne joue que des paragraphes ; S2 l'éprouve (octets du vrai Yjs, `diffWrites.test.ts`).
- **`format` pour un changement de marques seules**, et le coût du nettoyage que chaque navigateur
  renvoie après une telle écriture.
- **La restriction dans le serveur** (le cache réel, la lecture de la portée d'un jeton) : S5.
- **La frappe qui tombe pendant une `before_agent`** : S4 et X2.

## Ce que le plan a changé d'après les mesures

- R1227 récrit : pas de seuil par part de blocs, deux transactions, le repli limité aux pages que
  la différence ne lit pas élément pour bloc, le contenu en ligne par préfixe et suffixe (R1254,
  R1255) ; la limite LA-a et la Task S2 suivent.
- R1217 et « Comment un jeton traverse `access.rs` » : la page jugée par la remontée de ses
  parents, la restriction entière réservée aux routes de tout l'espace, les rangs des parents
  gardés, pas de cache (R1256).
- R1234, la Task S4, LA-e et le tableau des budgets : la `before_agent` hors de l'acteur dès S4,
  avec ses chiffres (R1257).
- R1236 et la Task M1 : les codes, `ttlMs` et `cacheScope` sur `server/discover`, ce que fait
  Claude Code ; les deux révisions restent (R1250, R1251) ; L17 et RO-A disent ce qu'a montré
  S0.
- R1215 : le corps d'un `401` n'atteint pas le modèle dans Claude Code (R1252).
- « Ce que la personne voit » et la Task C1 : ce que S0 sait de `${MEMENTUM_TOKEN}`, et ce qui
  reste à vérifier à la fin du lot (R1253).
- L'encadré d'état, « Ce qui reste à mesurer », « Supposé » : S0 faite.
