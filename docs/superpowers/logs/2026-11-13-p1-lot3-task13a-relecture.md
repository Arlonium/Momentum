# Task 13a (la forme à clés, R220) : relecture indépendante, constats et décisions

Relecture faite le 2026-11-13 dans l'atelier de la tâche (branche locale `worktree-agent-acd5330e6b63d3097`, HEAD `65089b2`), par un relecteur qui n'a pas écrit le code. Chaque constat est prouvé par une sonde jetable, sauf mention « lu ». Ce fichier garde son rendu et les décisions du contrôleur ; il se lit avec le [rapport de l'implémenteur](2026-11-13-p1-lot3-task13a-rapport.md). À fondre au registre à la clôture de la partie A.

Les sondes du relecteur (un banc d'essais au hasard, des sondes dirigées, la correction proposée pour `followBlock`) sont gardées sur le poste, hors du dépôt : `.superpowers/sdd/2026-11-12-p1-lot3-collaboration-blocs-riches/13a-relecture/`.

## Verdict

**Reprise sur `main` après corrections. Ni telle quelle, ni à abandonner.**

- **Aucune divergence** : sur 5 050 graines de 120 ou 150 pas, les documents Yjs sont égaux après échange complet dans tous les passages.
- **Aucune exception venue du correctif ou de `followBlock`.**
- **La forme à clés fait ce que R220 promet, et mieux que `main` sur chaque forme de page.** Régime visé (page plate, chacun déplace ses blocs, tous tapent, annulent, rétablissent ; 200 graines de 120 pas) :

| | Jetons tapés | En double | Dans un autre bloc | Blocs jumeaux | ProseMirror ≠ Yjs |
|---|---|---|---|---|---|
| `main` (code d'origine) | 6 153 | 495 | 1 978 | 38 | 11 |
| correctif sans `followBlock` | 6 105 | 49 | 473 | 28 | 2 |
| **livré (correctif et `followBlock`)** | 6 094 | **0** | **0** | **0** | **0** |

- **Le serveur l'accepte** : `cargo test -p mementum-sync --test moves -- --ignored` rend 1 vert et 3 rouges, sur la base de la branche comme sur les sources de `main` du 2026-11-13 (avec R198, R207, R208) ; les trois échecs sont tous « le texte de Tom n'y est pas » (ce que la 13b doit lever), aucun refus de la garde.

## Constats importants

**I1 — Une image cliquée, puis déplacée par un autre : Suppr retire un bloc voisin (régression).** `followBlock` ne suit une sélection de nœud que si le nœud porte un `id` ; un clic sélectionne le nœud `image`, qui n'en a pas. Mona clique une image ; Tom la déplace ; Mona presse Suppr sans recliquer : le bloc de texte qui a pris la place de l'image disparaît chez tous, l'image reste. Sur le code d'origine, la sélection reste sur l'image, et Suppr retire l'image. Les tests « un clic sur une image » de la branche passent par `selectBlock`, qui sélectionne le conteneur : ils ne prouvent pas ce cas.

**I2 — Après une Entrée au milieu d'un bloc à enfants, `followBlock` envoie le curseur d'un autre dans la mauvaise moitié (régression partielle).** Sous R223, le code d'origine donne l'ancien élément à l'`id` neuf ; le critère « l'`id` du bloc a changé » se trompe alors. Tom tape à la fin d'une puce qui a des sous-puces ; Mona presse Entrée au milieu ; la suite de Tom part à la fin de la première moitié.

| Curseur de Tom, Entrée de Mona | Sans `followBlock` | Livré | Correction B |
|---|---|---|---|
| x@6, Entrée@3 | `c X!` (juste) | `Blo!` (faux) | `c X!` |
| x@5, Entrée@3 | `c !X` (juste) | `Blo!` (faux) | `c !X` |
| x@1, Entrée@3 | `!c X` (faux) | `B!lo` (juste) | `B!lo` |
| x@3, Entrée@6 | bloc neuf (faux) | `Blo!c X` (juste) | `Blo!c X` |
| x@6, Entrée@6 | bloc neuf | `Bloc X!` | `Bloc X!` |

Au hasard (page imbriquée, 300 graines) : 6 dérives du curseur avec le livré, 0 avec les corrections A et B.

**Correction éprouvée pour I1 et I2** (« variante C », 49 lignes, gardée avec les sondes) : pour un nœud sans `id`, le retrouver à la même place dans le bloc de même `id`, sinon rendre un curseur de texte ; pour un curseur dont l'élément est vivant et dont l'`id` a changé, ne suivre que si la place tient dans le bloc de même `id` sans être rabattue. Avec elle : les vingt tests de `followBlock.test.ts` verts ; dans un navigateur, `block-move`, `collab`, `offline`, `presence` : 17 verts, 2 `fixme` ; la sonde de l'image donne la sélection restée sur l'image.

**I3 — Un déplacement emporte des blocs entiers que d'autres ont mis sous le bloc, pas seulement « ce qu'ils y ont écrit ».**
- Annulation (régression pour le glisser et le clavier) : Mona glisse X ; Tom rentre W sous X (Tab) et y tape ; Mona fait Ctrl+Z : W disparaît en entier (`y | z | x [w « Bloc W de Tom »]` devient `x | y | z`). Sur le code d'origine, W survit. C'était déjà vrai sur `main` pour Tab ; R220 l'étend.
- En même temps, sans annulation : Mona déplace X pendant que Tom rentre Y sous X : Y disparaît, dans les deux ordres d'arrivée. `main` n'est pas mieux : il perd un autre bloc et double Y.
- Annuler après qu'un autre a redéplacé le bloc : le bloc est en double. `main` fait pire.

**I4 — R227 : un glisser de plusieurs blocs recrée les blocs croisés, pas ceux du geste.** A, B, C déposés sous E écrivent la suppression et la création de D et E. Tom tapait dans D et E, que personne n'a déplacés : ses deux frappes sont perdues. « Le texte des autres blocs n'est pas touché » est faux dès qu'on déplace plus de blocs qu'on n'en croise ; même chose au clavier avec plusieurs blocs sélectionnés. Ce chemin n'a aucun test : une mutation qui ne supprime qu'un seul voisin laisse tout vert.

**I5 — Blocs jumeaux : l'`id` neuf que BlockNote leur donne est écrit à la première transaction venue.** Le rapport de l'implémenteur disait « sans l'écrire ». Deux glissers du même bloc laissent deux blocs du même `id` ; `UniqueID` de BlockNote les renomme chez chaque client, et la première transaction locale, même vide (chaque changement de présence reçu en lance une), écrit ces `id` neufs : le serveur finit avec deux « Bloc X » sous deux `id` neufs. L'`id` d'origine est perdu, donc les fils de commentaires du bloc se détachent ; une fois renommés, les jumeaux ne sont plus des doublons pour le dédoublonnage du serveur. Ce n'est pas une régression (`main` abîme davantage), mais **l'attente « les fils gardent leur bloc » de la Task 13b ne tiendra pas sans mesure côté client**.

## Constats mineurs

- **m1** — Quand le bloc recréé n'a pas ce que je venais de taper, mon curseur garde son décalage et tombe plus loin dans le texte (1 cas sur 1 437 contrôles). Inhérent.
- **m2** — Une sélection sur plusieurs blocs dont un bout est déplacé devient une autre plage.
- **m3** — `restoreRelativeSelection`, branche texte, peut lever `RangeError: Position … out of range` (colonnes, après une page vidée). Antérieur : `main` en lève aussi. Une garde comme celle de R224 coûterait deux lignes.
- **m4** — La table du binding grossit : de 18 à 5 419 entrées après 300 déplacements d'un bloc à cinq enfants ; 4 521 avec le code d'origine. Antérieur en nature.
- **m5** — Si les mises à jour d'un même client étaient livrées à l'envers (ce que R131 et TCP excluent), les suppressions passeraient avant les insertions, la racine se viderait et y-prosemirror la supprimerait. `main` se vide aussi en désordre. Un déplacement dépend désormais de cette garantie d'ordre.
- **m6** — Une copie non corrigée de y-prosemirror reste, orpheline, dans `.pnpm` de l'atelier (sans effet : l'application ne la résout pas).

Défauts antérieurs vus au passage, présents sur `main`, hors de la tâche : une colonne vidée emporte toute la liste de colonnes ; des déplacements croisés entre niveaux vident une page ; un bloc reste sans `id` dans Yjs après l'annulation d'un renommage ; deux Entrées simultanées dans le même bloc doublent un texte. Les trois pages vidées tracées sur le livré viennent toutes d'un déplacement entre niveaux ou de la cascade des colonnes, jamais d'un réordonnancement à clés.

## Avis du relecteur sur les écarts déclarés

- **R223** (la branche à clés ne joue que si un bloc a changé d'ordre) : nécessaire, mesure refaite : sans la règle, exactement les quatre gestes annoncés rougissent. La condition « deux milieux non vides » est redondante. Coût réel : « même `id` = même bloc » devient faux hors réordonnancement, d'où I2.
- **R224** (garde de `restoreRelativeSelection`) : juste, mais elle n'évite que l'exception, pas la sélection reportée sur un voisin (I1).
- **R225** (les deux fichiers corrigés, par ajouts seuls) : bon choix ; empreintes inchangées du début à la fin.
- **R227** (départage par la sélection) : logique juste ; la conséquence I4 est à trancher.
- **R228** (`followBlock` élargi) : nécessaire (sans lui : 473 jetons mal placés, et un dépôt qui vide le bloc tenu). Deux défauts, I1 et I2. Pour le reste : cellule de tableau juste, lecture seule sans écriture, aucune exception.

## Ce que les essais ont couvert

Banc : deux ou trois éditeurs BlockNote et un serveur sans éditeur ; frappe à la sélection courante ; glisser en deux temps (prise, puis dépôt) ; clavier, Tab, Maj+Tab, Entrée, retour arrière, suppression, changement de type, annuler, rétablir ; livraisons retardées. Contrôles : égalité des documents ; ProseMirror égal à Yjs après chaque geste et chaque réception ; exceptions ; curseur resté derrière son dernier jeton ; jetons et `id` en double. Tous gestes, 300 graines par ligne (150 à trois personnes, 200 pour les onglets mêlés) :

| Page | Code | Présents / tapés | Mal placés | Dérives du curseur | ProseMirror ≠ Yjs | Pages vidées |
|---|---|---|---|---|---|---|
| plate | `main` | 5 629 / 8 532 | 3 093 | 60 | 24 | 0 |
| plate | livré | 6 106 / 8 549 | 1 646 | 3 | 2 | 0 |
| imbriquée | `main` | 5 070 / 8 521 | 2 240 | 22 | 44 | 13 |
| imbriquée | livré | 5 767 / 8 547 | 1 699 | 6 | 7 | 0 |
| types mêlés | `main` | 3 077 / 8 239 | 934 | 5 | 22 | 9 |
| types mêlés | livré | 6 115 / 8 283 | 1 131 | 4 | 3 | 0 |
| colonnes | `main` | 5 266 / 8 634 | 1 948 | 21 | 21 | 12 |
| colonnes | livré | 5 797 / 8 629 | 1 625 | 2 | 10 | 7 |
| imbriquée, trois personnes | livré | 3 167 / 5 427 | 1 085 | 5 | 51 | 8 (une graine) |
| imbriquée, un onglet corrigé et un d'origine | — | 3 489 / 5 667 | 1 270 | 19 | 3 | 1 |
| plate, un onglet corrigé et un d'origine | — | 3 574 / 5 667 | 1 387 | 36 | 15 | 0 |

(« Mal placés » compte ici tous les gestes, dont les déplacements entre niveaux, Tab et les changements de type, que la forme à clés ne change pas ; le régime visé par R220 est le premier tableau.)

Coût, 5 000 frères, dans jsdom, même passage : une frappe au milieu, 7,7 ms avec le livré contre 16,1 ms avec le code d'origine ; le premier bloc glissé tout en bas, 18 ms et 186 octets contre 357 ms et 135 Ko ; une place au clavier, 156 octets contre 38.

Mutations du correctif : sans la règle R223, 4 rouges ; sans le départage par la sélection, 1 rouge ; sélection inversée, 14 ; insertion décalée, 109. **Restent vertes** : la suppression d'un seul voisin (I4 : le glisser de plusieurs blocs n'a pas de test) et la comparaison du nom de nœud (`matchNodeName`).

**Non vérifié** : la composition (IME) ; l'aperçu d'une version (`PreviewEditor`) ; les budgets du navigateur, la suite e2e entière et l'installation à partir de rien (non refaits par le relecteur) ; couper-coller, opérations de tableau, « Transformer en » par le menu, dépôt en colonne à la souris ; côté serveur, seules les quatre fixtures historiques sont rejouées (les 136 fichiers de scènes ne sont lus par aucun test Rust) ; une transaction qui mêle un réordonnancement et autre chose (aucune perte trouvée, pas d'essai dirigé).

## Décisions du contrôleur (2026-11-13)

1. **La Task 13a entre sur `main` après un tour de correction**, re-relu. R223, R224, R225 et R228 sont acceptés.
2. **I1 et I2** : la correction éprouvée par le relecteur est reprise, avec ses tests (les six cas du tableau ; un vrai clic sur une image dans un navigateur).
3. **I4, R227 amendé** : quand la sélection dit quels blocs le geste déplace (le groupe d'un seul tenant autour du bloc où le dépôt laisse le curseur, ou les blocs sélectionnés au clavier) et que les autres blocs gardent leur ordre, **ce sont les blocs du geste qui sont recréés**, même s'ils sont plus nombreux que les blocs croisés ; sinon, la règle d'aujourd'hui (le moins de blocs recréés). Pourquoi : « ce qui est tapé dans un bloc qu'un autre déplace peut se perdre » se comprend ; « dans un bloc qu'un autre croise » ne se comprend pas. Si cela ne se fait pas simplement et sûrement (les 21 gestes ordinaires inchangés, le hasard sans divergence), R227 reste tel quel et la limite est écrite telle que mesurée.
4. **I3** : accepté pour ce lot et écrit tel quel dans la limite (un déplacement, son annulation, ou un déplacement fait pendant qu'un autre rentre un bloc dessous, peut emporter ces sous-blocs). L'annulation d'un déplacement va au plan d'après le lot 3, avec les frappes tardives (R222). La Task 13b reporte les sous-blocs dans l'ordre qu'elle traite.
5. **I5** : la limite écrite dit vrai (deux déplacements simultanés du même bloc le laissent en double, sous des `id` neufs, et ses fils de commentaires se détachent). La Task 13b ne promet « les fils gardent leur bloc » que si elle apporte la mesure côté client qu'il faut ; à décider dans son découpage.
6. **m3** : la garde de deux lignes est ajoutée au correctif. Les autres mineurs vont à la liste de la relecture finale.
7. **Tests à ajouter avant la reprise** : le glisser de plusieurs blocs (la mutation « un seul voisin » doit rougir) ; un essai au hasard commité, avec `followBlock` monté et la frappe au curseur, borné pour tenir dans la suite ; le clic sur une image.
