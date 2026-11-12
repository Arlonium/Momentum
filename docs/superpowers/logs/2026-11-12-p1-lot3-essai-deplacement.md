# Essai borné de la Task 13 : un bloc déplacé pendant qu'un autre écrit (R39, R138)

Compte rendu de l'essai que le plan du lot 3 prévoyait à l'étape 1 de la Task 13, élargi après la Task 12. Il a été mené le 2026-11-12 dans un atelier jetable, sans rien écrire dans le dépôt ; ses prototypes ne sont pas repris. Ce document garde ce qu'il a mesuré et son verdict : c'est le point de départ du découpage de la Task 13, en fin du [plan](../plans/2026-11-12-p1-lot3-collaboration-blocs-riches.md), et du plan qui traitera ce qui reste.

Tout ce qui suit côté client est mesuré dans jsdom (Vitest, vrai BlockNote 0.57 avec `withCollaboration`, deux ou trois éditeurs, identifiants de client fixés). **Aucun vrai navigateur n'a tourné pendant l'essai.** Le report côté serveur est mesuré par un test Rust jetable, sur des octets produits par un vrai BlockNote.

## Verdict

**La promesse de R138 telle qu'écrite (les deux ordres d'arrivée) n'est pas faisable dans ce lot.** Deux pas le sont, et sont utiles seuls :

1. **faire écrire au client un déplacement comme « le bloc déplacé est supprimé et recréé, les autres ne sont pas touchés »** (la « forme à clés ») ;
2. **reporter côté serveur, à l'arrivée du déplacement, ce que son auteur n'avait pas vu** : cela règle l'ordre « le déplacement arrive après l'édition ».

L'ordre inverse (une frappe faite sans avoir vu le déplacement, qui arrive après lui) demande une lecture que yrs 0.28.1 n'offre pas ; il reste une limite, décrite plus bas.

## 1. Ce qu'un geste écrit aujourd'hui dans Yjs

y-prosemirror 1.3.7 ne regarde que l'état final : après chaque transaction, `_prosemirrorChanged` passe tout le document à `updateYFragment`, qui saute le préfixe et le suffixe identiques, puis traite les enfants du milieu **par position** (même nom de nœud : l'élément Yjs est récrit en place, attributs dont `id`, et texte par diff ; noms différents : l'élément est supprimé et recréé).

| Geste | Ce qu'il écrit aujourd'hui | Une frappe concurrente |
|---|---|---|
| Glisser X sous Z, même type, sans enfants | Aucun élément créé ni supprimé. Par conteneur de la plage : une entrée `id` écrasée et un diff de texte. | Dans n'importe quel bloc de la plage, elle finit chez le voisin (« Bloc Y de Tom » ou « Bloc  de TomY » selon les identifiants de client). |
| Un titre dans la plage | Les éléments de contenu qui changent de type sont supprimés et recréés. | Perdue dans les blocs recréés ; dans un bloc resté de même type, chez le voisin. |
| Bloc avec enfants | Conteneurs récrits en place ; le groupe d'enfants est supprimé et recréé avec les mêmes `id`. | Perdue dans un enfant. |
| Tab | Nouveau `blockGroup` et nouveau conteneur de même `id`, original supprimé. | Perdue dans le bloc ; juste ailleurs. |
| Maj+Tab | Nouveau conteneur de même `id`, groupe de l'ancien parent supprimé. | Perdue. |
| Clavier (`moveBlocksDown`) | Récriture en place. | Chez le voisin. |
| Deux déplacements, sans frappe | — | « Bloc YY », « Bloc ZX » ; un `id` disparaît, un autre est en double. |

Mesuré en plus :

- **Le dédoublonnage du serveur aggrave le cas à deux déplacements** (lu dans `crates/mementum-crdt/src/guard.rs`, pas mesuré par l'acteur) : les doublons d'aujourd'hui naissent d'entrées `id`, pas d'éléments insérés ; le serveur ne les voit pas, puis supprime le « second » à la prochaine insertion d'élément, qui peut porter le vrai texte.
- **BlockNote renomme les doublons chez lui** : deux conteneurs de même `id` dans Yjs reçoivent chacun un `id` neuf, au hasard, dans ProseMirror seulement ; l'écart part dans Yjs à sa prochaine transaction locale.
- **Annulation** : Mona glisse X, Tom (qui a reçu le déplacement) écrit dans X, Mona fait Ctrl+Z : aujourd'hui le texte de Tom finit dans Z ; pour Tab, il est déjà perdu.
- **Curseur** : le curseur de qui tape dans un bloc de la plage reste dans le même élément, qui montre maintenant le voisin : ses frappes suivantes y vont.
- **Grand glisser** : le premier bloc d'une page de 10 000 blocs glissé tout en bas récrit les 10 000 blocs : une mise à jour de 630 Ko, 1,9 s dans `_prosemirrorChanged`.
- **Coût par frappe, déjà là** : sur 10 000 blocs, `_prosemirrorChanged` prend environ 30 ms pour une frappe au milieu ; le dispatch entier, environ 60 ms à la fin de la page.
- **Origine d'une frappe** : Yjs avance sur les éléments supprimés avant d'insérer. Une frappe à la fin d'un texte dont la queue a été effacée a pour origine un élément **supprimé**.

## 2. Les pistes

### 2.1 Le client à clés (prototypé, deux variantes)

Au milieu, après le préfixe et le suffixe identiques, les enfants sont appariés par `id` de bloc. La plus longue sous-suite croissante reste en place ; le reste est supprimé puis recréé à sa place. Sans clés sûres (un enfant sans `id`, deux `id` égaux, du texte), on retombe sur le code d'origine. À longueur égale (échange de deux voisins), on recrée de préférence le bloc qui porte la sélection locale.

Cœur du correctif de `updateYFragment` (y-prosemirror 1.3.7), **donné pour mémoire, à réécrire en partant des tests** :

```js
y.transact(() => {
  if ((yChildCnt - left - right > 0 || pChildCnt - left - right > 0) &&
      updateKeyedChildren(y, yDomFragment, yChildren, pChildren, left, right, meta)) return
  // … code d'origine, par position
```

```js
const updateKeyedChildren = (y, yParent, yChildren, pChildren, left, right, meta) => {
  const yMid = yChildren.slice(left, yChildren.length - right)
  const pMid = pChildren.slice(left, pChildren.length - right)
  const pIndex = new Map()
  for (let i = 0; i < pMid.length; i++) {
    const id = pMid[i] instanceof Array ? undefined : pMid[i].attrs.id
    if (typeof id !== 'string' || id === '' || pIndex.has(id)) return false   // repli
    pIndex.set(id, i)
  }
  const seen = new Set(); const target = []
  for (const yc of yMid) {
    const id = yc instanceof Y.XmlElement ? yc.getAttribute('id') : undefined
    if (typeof id !== 'string' || id === '' || seen.has(id)) return false     // repli
    seen.add(id)
    const j = pIndex.get(id)
    target.push(j !== undefined && matchNodeName(yc, pMid[j]) ? j : -1)
  }
  const keep = keptInPlace(target, light)       // sous-suite croissante la plus longue
  const keptOfP = new Array(pMid.length).fill(null)
  for (let i = yMid.length - 1; i >= 0; i--) {
    if (keep[i]) keptOfP[target[i]] = yMid[i]
    else { meta.mapping.delete(yMid[i]); yParent.delete(left + i, 1) }
  }
  let at = left
  for (let j = 0; j < pMid.length; j++) {
    const yc = keptOfP[j]
    if (yc !== null) {
      if (!mappedIdentity(meta.mapping.get(yc), pMid[j])) updateYFragment(y, yc, pMid[j], meta)
      at++; continue
    }
    const ins = []
    for (; j < pMid.length && keptOfP[j] === null; j++) ins.push(createTypeFromTextOrElementNode(pMid[j], meta))
    j--; yParent.insert(at, ins); at += ins.length
  }
  return true
}
```

Seconde variante, sans toucher à la dépendance : envelopper `binding._prosemirrorChanged` ; dans la même transaction Yjs, les blocs déplacés sont supprimés et un élément vide de même nom et de même `id` est posé à leur place, puis le `updateYFragment` d'origine le remplit. Sur 12 gestes, les comptes de structures sont identiques à ceux du correctif. L'essai préfère le correctif par `pnpm patch` : création en une fois, pas d'élément vide transitoire, actif pour tout éditeur, et l'installation échoue bruyamment s'il ne s'applique plus.

**Ce que deviennent les faits** (frappe concurrente dans… ; « à l'arrivée » : report des seuls textes déjà au serveur quand le déplacement arrive, §2.2) :

| Geste | Aujourd'hui : bloc déplacé | Aujourd'hui : autre bloc de la plage | Clés seules : bloc déplacé | Clés seules : autre bloc | Clés + report à l'arrivée |
|---|---|---|---|---|---|
| Glisser, clavier | chez un voisin | chez un voisin | **perdu** | juste | gardé si le serveur l'avait avant le déplacement, sinon perdu |
| Titre dans la plage | perdu | perdu ou chez un voisin | perdu | juste | idem |
| Bloc avec enfants | chez un voisin (enfants : perdu) | chez un voisin | perdu (tout le sous-arbre) | juste | idem, enfants compris |
| Tab, Maj+Tab | perdu | juste | perdu | juste | idem |

Deux déplacements concurrents, avec les clés : blocs différents, juste, sans doublon ni texte mêlé ; même bloc, deux exemplaires de même `id`, que le dédoublonnage du serveur voit enfin (la mise à jour insère des éléments).

**Mesure au hasard** (40 graines, 60 pas, 8 blocs, 3 onglets en étoile ; chaque frappe est un jeton qui nomme son bloc ; le faux serveur de l'essai dédoublonne à chaque mise à jour : les chiffres ne valent que par comparaison) :

| Scénario | Mode | Jetons | À leur place | Chez un voisin | Perdus | En double | Blocs dont l'`id` a disparu (sur 320) |
|---|---|---|---|---|---|---|---|
| 1 déplace, 2 tapent | position | 1 561 | 1 033 | 528 | 0 | 0 | 0 |
| 1 déplace, 2 tapent | clés | 1 561 | 1 346 | 0 | 215 | 0 | 0 |
| 2 déplacent, 1 tape | position | 790 | 234 | 238 | 148 | 170 | 187 |
| 2 déplacent, 1 tape | clés | 770 | 560 | 17 | 192 | 1 | 19 |

Convergence, et ProseMirror égal à Yjs : 40 sur 40 partout, onglets mêlés compris (un onglet à clés et un onglet par position). C'est le mode de **qui déplace** qui décide. Les 17 et 19 de la dernière ligne viennent du renommage local des doublons par BlockNote, pendant lequel la plage retombe par position.

**Ce que cela change ailleurs** :

- frappe, Entrée (début, milieu, fin), retour arrière, insertion, suppression, changement de type : mêmes comptes de structures qu'avant ;
- un bloc remplacé est supprimé et recréé, au lieu de renommé : **un élément ne change plus jamais d'`id`** ;
- coût : sur 10 000 blocs, pas d'écart mesurable par frappe (36 à 46 ms par position, 37 à 46 ms par clés, dispatch entier) ; le grand glisser passe de 1,9 s et 630 Ko à 24 ms et 246 octets ;
- colonnes : comme les blocs ; tableaux : pas d'`id`, donc repli, rien de changé (par construction, pas testé) ;
- `id` absent ou en double : repli, mêmes écritures qu'aujourd'hui.

**Ce que cela casse** :

1. **frappe dans le bloc déplacé** : « chez un voisin » devient « perdue » (c'est la forme que R39 décrit depuis le lot 1) ; le report à l'arrivée en récupère une part ;
2. **Ctrl+Z d'un déplacement** : tout ce que d'autres ont écrit dans le bloc depuis le déplacement est retiré, même plusieurs minutes après ; c'est déjà vrai aujourd'hui pour Tab, cela s'étend au glisser simple et au clavier ;
3. **curseur de qui tape dans le bloc qu'un autre déplace** : il saute à la fin du document (la position relative pointe un élément ramassé). Un petit plugin `appendTransaction` le remet dans le bloc de même `id` : prototypé, le curseur suit et la frappe suivante tombe dans le bon bloc.

### 2.2 Le report côté serveur (prototypé sur le cas simple)

Les trois lectures que le plan demandait passent sur yrs 0.28.1, par l'API publique :

1. `Update::decode_v1(u).delete_set()` avant d'appliquer ;
2. les caractères vivants d'un `XmlText` avec leurs marques par `diff()`, leur identifiant par `text.sticky_index(txn, i, Assoc::After).id()` ;
3. `StickyIndex::from_id(id, Assoc::After).get_offset(txn)`, qui vaut aussi pour un caractère supprimé d'un texte vivant.

La règle « ce que le jeu de suppressions ne couvre pas est ce que l'auteur n'avait pas vu » est **vraie** avec la forme à clés, y compris quand l'original est supprimé en entier et quand la mise à jour est une étape 2 qui porte tout le jeu de suppressions du client. Les descendants non vus ne sont pas couverts. À l'intégration ils sont supprimés avec leur parent ; arrivés après, ils naissent supprimés. Dans les deux ordres, il faut donc les saisir **avant** d'appliquer.

Résultats du prototype, témoin égal au serveur :

- glisser à clés avec trois frappes (début, milieu, fin) : « Début Bl[milieu]oc X de Tom » ;
- frappe touche par touche, gras (marques gardées), Tab, bloc avec enfants (frappe dans l'enfant et dans le parent) ;
- formes « retour en ligne » sur une page avec historique ;
- la même mise à jour reçue deux fois : reportée une fois ;
- auteur du déplacement qui a aussi tapé hors ligne dans le bloc : le prototype **s'abstient** (la copie n'est pas le texte vu), sans doublon.

**Il manque une quatrième lecture pour les frappes tardives.** Une frappe faite sans avoir vu le déplacement, arrivée après lui, a souvent pour origine un élément **supprimé** de l'original. Sans ces éléments dans un enregistrement du déplacement, « de Tom » tapé à la fin de X n'est pas reconnu et se perd (le test passe sur une page neuve, pas sur une page qui a un passé). L'API publique de yrs n'énumère pas les éléments supprimés d'un texte. Le contournement essayé (tout le jeu de suppressions de la page, chaque identifiant résolu par `get_offset`) est juste mais inutilisable : 27 ms sur 1 000 blocs récrits trois fois, **6,9 s sur 10 000** (en release). Restent un correctif de yrs ou un décodage de l'état complet par déplacement : ni l'un ni l'autre n'est essayé.

Autres modes d'échec du report :

- annuler un déplacement après un report retire le texte reporté, sauf règle « un déplacement ne supprime jamais de texte », qui ressusciterait ce qu'un auteur hors ligne avait effacé avant de déplacer ;
- une suppression concurrente reparaît dans la copie ;
- non couverts : contenu en ligne à plusieurs textes (mention, saut de ligne), tableaux, sous-bloc ajouté sous l'original, bloc déplacé deux fois.

### 2.3 Le serveur seul, sans changer le client (jugé, écarté)

Reconnaître la permutation est facile après coup. Savoir ce que l'auteur avait vu ne l'est pas : le diff garde le préfixe et le suffixe communs, donc « non couvert » ne veut plus dire « non vu ». L'étape 1 ne borne que le client revenu. Le report devrait supprimer du texte tapé là où il a atterri, sur une supposition. Il dépendrait en plus des heuristiques internes de y-prosemirror, et ne démêle pas deux déplacements concurrents.

### 2.4 Les déplacements demandés au serveur, piste (a) de R39 (jugé, écarté)

Il faudrait intercepter tous les gestes (glisser, clavier, Tab, Maj+Tab, couper-coller, colonnes), renoncer au hors-ligne ou tenir une file d'intentions, et sortir les déplacements de l'annulation locale. Cela ne règle que l'ordre « édition puis déplacement » ; les frappes tardives demandent quand même l'enregistrement. Dominée par « clés + report ».

### 2.5 Deux déplacements du même bloc

On veut un seul exemplaire, à une place déterminée. Avec les clés, le dédoublonnage existant suffit pour l'unicité : le premier dans l'ordre du document reste. Ce qui a été tapé dans l'exemplaire retiré est perdu (mesuré). Tant que BlockNote renomme les doublons chez lui, un onglet qui écrit pendant la fenêtre du doublon peut faire perdre l'`id` d'origine, donc les fils de commentaires du bloc.

### 2.6 Hors d'atteinte, quelle que soit la piste

- un changement de type ou de prop du bloc pendant son déplacement ;
- les lignes et colonnes de tableau déplacées (par position) ;
- un onglet resté sur l'ancien code, qui écrit encore par position ;
- sans la quatrième lecture : les frappes tardives, c'est-à-dire ce qu'une personne hors ligne a tapé dans le bloc pendant qu'un autre le déplaçait en ligne, et les une ou deux touches de la latence.

## 3. Ce qui reste à planifier à part, après le lot 3

1. Les frappes tardives, après avoir tranché la quatrième lecture (correctif de yrs, ou enregistrement des éléments de l'original au moment du déplacement).
2. La règle d'annulation d'un déplacement (Ctrl+Z ne doit pas retirer ce que d'autres ont écrit depuis).
3. Le report hors du cas simple : contenu en ligne à plusieurs textes, sous-bloc ajouté sous l'original, bloc déplacé deux fois.

## 4. Ce dont l'essai n'est pas sûr

- **Le vrai navigateur** : le dépôt réel (poignée de BlockNote, `drop` de ProseMirror, dépôt en colonnes), la sélection après dépôt dont dépend le départage d'un échange de voisins, les budgets. Il faut lancer les e2e sur un build avec le correctif.
- **La course entre le renommage des doublons par BlockNote et le correctif du serveur** : vue dans jsdom, durée réelle inconnue ; à mesurer en e2e avec un doublon provoqué.
- **Le report** : jamais passé par l'acteur ni par de vrais clients Yjs (témoin yrs) ; pas de persistance, pas de fusion à trois voies, pas de chaîne de déplacements.
- **Le coût du report** hors lecture des supprimés n'est mesuré que pour un bloc en tête de page (0,35 ms sur 10 000 blocs).
- **Tableaux et colonnes** : seul le déplacement dans une colonne est mesuré.

## 5. La limite à écrire pour les utilisateurs, une fois les deux pas livrés

« Ce qui est tapé dans un bloc, ou dans ses sous-blocs, pendant qu'une autre personne le déplace (glisser, clavier, Tab, Maj+Tab) peut être perdu quand la frappe arrive au serveur après le déplacement, ainsi qu'un sous-bloc ajouté sous lui au même moment. Le texte des autres blocs n'est pas touché. Annuler un déplacement remet le bloc comme avant : ce que d'autres y ont écrit depuis est retiré. Deux personnes qui déplacent le même bloc en même temps le retrouvent à une seule des deux places. Les lignes et colonnes de tableau déplacées ne sont pas protégées. »
