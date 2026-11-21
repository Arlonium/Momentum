# Le format MMD (Mementum Markdown)

> Spécification du format des pages de Mementum, telle que la phase 1 la sert (lot 5, novembre
> 2026). Licence Apache-2.0. Le serveur MCP la donne telle quelle comme ressource
> `mementum://spec/mmd`. Elle est tirée de la conception (§6.2) et du code qui lit et écrit le
> format (`crates/mementum-md`) ; en cas d'écart, le serveur juge : il refuse toute forme qu'elle
> ne décrit pas, et son message dit quoi écrire à la place.

## En bref

- MMD est le texte d'une page : CommonMark et GFM (tableaux, cases à cocher, barré), `$maths$`,
  et des balises de type XML pour les blocs propres à Mementum.
- **Une forme par contenu, et une lecture stricte** : le serveur n'accepte que la forme canonique
  décrite ici. Écrire ce que `fetch` rend redonne la même page.
- Un agent lit une page par `fetch` (avec `anchors: true` pour viser des blocs), la change par les
  opérations d'`update_page` (`str_replace` d'abord), et crée des pages par `create_pages`. Le
  MMD d'une opération (`insert`, `replace_block`, `replace_page`) est une suite de blocs entiers,
  écrite comme une page.
- **Le MMD ne porte pas les métadonnées de la page** : ni titre, ni identifiant, ni révision, ni
  propriétés (pas de frontmatter YAML en phase 1). `fetch` les donne à côté du texte ; le titre se
  change par l'opération `set_properties`.

## Une page

- Un document est une suite de blocs. Chaque ligne finit par `\n` (LF, jamais CRLF) ; un document
  vide est le texte vide. Une page sans bloc n'existe pas dans l'éditeur : une page vide se relit
  `<p/>`, un paragraphe vide.
- **Lignes vides** : une entre deux blocs frères ; aucune entre deux éléments voisins d'une même
  liste (puces, numérotés, cases) ni entre deux `<column>`. Une ligne vide n'a jamais d'espaces. Un
  marqueur sans contenu n'a pas d'espace finale (`##`, `-`, `- [ ]`, `1.`, `>`).
- **Listes** : les éléments consécutifs d'un même type forment une seule liste, sans ligne vide
  entre eux.

## Ancres, alias et révision

- **Lue avec ses ancres** (`anchors: true`), chaque bloc qui a un identifiant porte son **alias** :
  ` ^bk7qa` en fin de première ligne pour un bloc de texte, seul sur la ligne qui suit un bloc à
  clôture (code, équation, séparateur, image, tableau), attribut `id="bk7qa"` d'une balise,
  `anchor="bk7qa"` d'un `<page>` ou d'un `<database>` (leur `id` est déjà pris). Sans ancres, le
  texte reste propre et économe.
- **Un alias** est `b` suivi d'au moins quatre caractères de base32 en minuscules (`a` à `z`, `2` à
  `7`), le début du SHA-256 de l'identifiant du bloc. Il ne dépend que des identifiants des blocs,
  jamais de leur rang : un bloc inséré au-dessus ne le change pas. Il **s'allonge** autant qu'il
  faut pour être seul dans la page au moment de la lecture, si bien qu'une lecture plus tardive peut
  rendre un alias plus long pour le même bloc.
- Un alias se résout au bloc de la page dont le hachage commence ainsi. Aucun : `stale_anchor` (le
  bloc n'existe plus, ou a été recréé sous un autre identifiant : relis la page). Plusieurs :
  `ambiguous_anchor` (un bloc arrivé depuis partage son début : relis la page avec ses ancres, tu
  recevras un alias plus long). Une exception : un `from` ambigu de `fetch` répond aujourd'hui
  `stale_anchor`, avec un message qui dit que l'ancre « désigne plusieurs blocs » ; la conduite à
  tenir est la même (relire la page avec ses ancres).
- **L'identifiant entier d'un bloc** est accepté à la place de son alias, sauf s'il a lui-même la
  forme d'un alias : une ancre de cette forme ne se lit que comme un alias.
- `<block>` écrit toujours le vrai identifiant du bloc, même avec les ancres, et ne reçoit pas
  d'alias.
- **Les ancres d'un MMD écrit** : `replace_page` reconnaît un alias (ou l'identifiant) d'un bloc de
  la page lue, et garde ce bloc ; un alias ambigu y est refusé (`ambiguous_anchor`) ; un alias
  périmé y est refusé (`stale_anchor`), sauf en `rebase` pour un bloc qu'un autre a retiré depuis ta
  lecture : écrit tel que tu l'avais lu, et sans sous-blocs, il est jeté (le retrait de l'autre
  tient) ; changé, c'est un conflit (`revision_conflict`). Un identifiant qui n'est ni un alias ni
  un identifiant de la page est oublié. Un bloc écrit sans ancre (ou dont l'ancre a été oubliée)
  reprend l'identifiant du bloc courant identique, liste de frères par liste de frères, s'il y en a
  un, et sinon un identifiant neuf : une page récrite à l'identique garde ses identifiants. Les
  blocs qu'insèrent `insert` et `replace_block` reçoivent des identifiants neufs, quoi que porte
  leur MMD ; `replace_block` donne l'identifiant du bloc remplacé au premier bloc de premier niveau
  de son type dans le MMD neuf (les fils de commentaires gardent leur bloc).
- **`revision`** est une poignée opaque (`r_` suivi de 16 chiffres hexadécimaux), rendue à chaque
  lecture et à chaque écriture. On la redonne à `update_page` (`base_revision`) : si les blocs que
  l'appel touche ont changé depuis, il est refusé (`revision_conflict`) avec les blocs à jour.
- **Une longue page se lit par tranches** : 24 Kio de MMD par défaut, 48 Kio au plus, coupés entre
  deux blocs. Une tranche coupée dit `truncated` et l'alias `next` du bloc où reprendre ; on reprend
  avec `from` (cet alias) et `since` (la révision de la première tranche), et la tranche dit alors
  quels blocs déjà lus ont changé depuis. La révision à donner ensuite est celle de la **dernière**
  tranche. Marques d'une tranche :
  - `<unloaded id="bk7qa"/>` : un bloc trop gros a été rendu avec ceux de ses sous-blocs qui
    tiennent ; la lecture reprend à ce sous-bloc ;
  - `<unloaded id="bk7qa" bytes=N/>` (ou `<unloaded bytes=N/>`) : un bloc dont le texte seul ne
    tient pas dans la tranche demandée (`limit`, 24 Kio par défaut), qu'il ait des sous-blocs ou non ;
    la lecture reprend à son premier sous-bloc, ou au bloc suivant. Relis-le avec `from` (son alias)
    et `limit` 49 152 ; au-delà de 48 Kio environ, il ne se lit pas par `fetch`, et ne se change que
    par `replace_block` ou `delete_block` ;
  - `<unloaded blocks=N/>` : des blocs sans alias à eux (sans identifiant, ou d'un identifiant porté
    deux fois) qui ne tenaient pas ; ils ne se lisent pas par `fetch`.
- **Les tranches mises bout à bout ne sont pas le MMD de la page** : chaque bloc y est rendu avec ses
  sous-blocs à la racine de la tranche, la numérotation d'une liste y reprend à 1, et les marques
  `<unloaded>` n'existent pas dans la page. Pour changer une longue page, vise ses blocs ; ne la
  récris jamais par `replace_page` à partir de tranches recollées.

## Règles de base

- **Marques** : en Markdown (`**`, `*`, `~~`, `` ` ``) quand le texte marqué commence et finit par
  une lettre ou un chiffre et qu'il est entouré de texte sans marque ; sinon en balises
  (`<strong>`, `<em>`, `<s>`, `<code>`). `<u>` et `<span color>` sont toujours en balises. Un seul
  genre de délimiteur Markdown à la fois (voir « Contenu en ligne »).
- **Espaces en début et fin de contenu** : écrites `&#32;` (Markdown les supprimerait). Un
  paragraphe vide s'écrit `<p/>`.
- **Indentation** : les enfants d'un élément numéroté sont indentés de la largeur du marqueur
  (« 1. » = 3 espaces, « 10. » = 4) ; ceux de tout autre bloc, de 2 espaces.
- **Maths en ligne** `$…$` : LaTeX à accolades équilibrées ; une formule que `$…$` ne relit pas
  exactement s'écrit `<math latex="…"/>`.
- **Point d'exclamation** : un `!` juste avant un lien est échappé (`\!`), sinon il serait relu
  comme une image.
- **Texte littéral** : `` \ * _ ` [ ] ~ $ | ^ `` sont précédés de `\` ; `<` s'écrit `&lt;` et `&`
  s'écrit `&amp;`. Dans un attribut, `&`, `"` et `<` s'écrivent `&amp;`, `&quot;` et `&lt;`. `{`
  et `}` s'écrivent `\{` et `\}`, et les caractères de contrôle en références numériques (voir
  « Échappements »).
- **Début de ligne** : un contenu qui commence par `#`, `>`, `-`, `+`, `N.` ou `N)` est échappé
  (`\#`, `1\.`) pour ne pas être lu comme un bloc.
- **Blocs de code** : clôture de backticks plus longue d'un cran que la plus longue suite de
  backticks du code (3 au minimum).
- **Toggle** : le titre est du MMD en ligne écrit dans l'attribut `title`. **Callout** : le premier
  paragraphe est le contenu du callout, les suivants sont ses enfants.
- **Texte brut** (sans marques ni échappement Markdown) : le titre d'un `<page>` et le texte
  alternatif d'une image.

## Les blocs

Les types, leurs props, leurs défauts et leurs formes viennent du registre des blocs
(`spec/schema/blocks.json`). Tout ce que l'éditeur sait représenter a une forme MMD, et la
relecture est exacte. Une prop égale à son défaut ne s'écrit pas.

| Type | Forme MMD | Props portées par la forme | Autres props (nom MMD) |
|---|---|---|---|
| `paragraph` | une ligne ; vide : `<p/>` (props en attributs) | — | `bg`, `color`, `align` |
| `heading` | `#` × niveau, puis une espace (niveaux 1 à 4) | `level` | `bg`, `color`, `align`, `toggle` (`isToggleable`) |
| `bulletListItem` | `- ` | — | `bg`, `color`, `align` |
| `numberedListItem` | `N. ` (N = `start` sur le premier élément de la liste, s'il vaut autre chose que 1) | `start` | `bg`, `color`, `align` |
| `checkListItem` | `- [ ] ` ou `- [x] ` | `checked` | `bg`, `color`, `align` |
| `toggleListItem` | `<toggle title="…">` … `</toggle>` | contenu en ligne = `title` | `bg`, `color`, `align` (attributs) |
| `quote` | `> ` | — | `bg`, `color` |
| `codeBlock` | clôture de backticks + langue ; contenu en texte brut : ni marque, ni lien, ni mention, ni formule ; un saut de ligne reste un saut de ligne du texte ; toute langue est gardée | `language` | — |
| `divider` | `---` | — | — |
| `image` | `![légende](url)` | `url`, `caption` | `name`, `preview` (`showPreview`), `width` (`previewWidth`), `align`, `bg` |
| `video` | `<video src="…"/>` | `url` (`src`) | `caption`, `name`, `preview`, `width`, `align`, `bg` |
| `audio` | `<audio src="…"/>` | `url` (`src`) | `caption`, `name`, `preview`, `bg` |
| `file` | `<file src="…" name="…"/>` | `url` (`src`), `name` | `caption`, `bg` |
| `table` | tableau GFM ; la ligne `{…}` qui suit porte `header-rows` (omis s'il vaut 1), `header-cols` (omis s'il vaut 0), `widths` (`"120,,80"`, vide = auto : une largeur par colonne de la grille, une cellule fusionnée de la première ligne en couvrant `colspan`) et `color` | lignes | `header-rows`, `header-cols`, `widths`, `color` |
| cellule de tableau | `\| texte {…} \|` | contenu | `bg`, `color`, `align`, `colspan` (de 1 à 100), `rowspan` (de 1 au nombre de lignes restantes) |
| `callout` | `<callout icon="…" color="…">` : 1ᵉʳ paragraphe = contenu, puis enfants ; contenu vide et 1ᵉʳ enfant paragraphe : `<p/>` en tête | `icon`, `color` | — |
| `equation` | `$$`, le LaTeX ligne à ligne, puis `$$` ; une équation vide garde sa ligne : `$$`, une ligne vide, `$$` | `latex` | — |
| `bookmark` | `<bookmark url="…"/>` | `url` | — |
| `columnList` / `column` | `<columns>` / `<column>` : un `<columns>` contient deux `<column>` ou plus, et rien d'autre ; une `<column>` n'existe que dans un `<columns>`, contient au moins un bloc, et jamais directement un `<columns>` | — | `width` (omis s'il vaut 1) |
| `pageLink` | `<page id="…">titre</page>` | `pageId`, `title` | — |
| `databaseRef` | `<database id="…" view="…"/>` | `databaseId`, `view` | — |
| `syncedBlock` | `<synced ref="…">` … `</synced>` | `ref` | — |
| `toc` | `<toc/>` | — | — |
| `unknown` et tout type inconnu du serveur | `<block type="…" id="…" data="…"/>`, `data` = base64 du JSON `{"attrs":{…},"content":[…],"children":[…]}` du nœud d'origine | `originalType` (`type`), `data` | `foreign` (`foreign=true` : un nœud étranger à recréer tel quel) |

- **En-têtes de tableau** : la première ligne reste la ligne d'en-tête de GFM ; `header-rows` et
  `header-cols` disent combien de lignes et de colonnes sont des en-têtes. Un tableau tout en
  en-têtes s'écrit `header-rows` = nombre de lignes, sans `header-cols`.
- **`<block>`** est l'échappatoire sans perte d'un bloc d'une version plus récente ou d'une
  extension : laisse-le tel quel. Le retirer demande `allow_deleting_content`.

## Props, balises, enfants

- **Props hors forme** : sur une forme en ligne ou à clôture, liste `{…}` en fin de première ligne,
  avant l'ancre (`Texte {align="center" color="red"} ^bk7qa`, `` ``` {language="mon langage"} ``) ;
  pour un tableau, sur la ligne qui suit ses lignes ; sur une balise, en attributs. Clés triées par
  ordre alphabétique. Une chaîne est toujours entre guillemets (échappée comme un attribut), un
  nombre et un booléen sans guillemets, y compris dans les balises (`width=320`, `preview=false`,
  `<column width=0.5>`). Un flottant s'écrit sans « .0 » (`width=320`).
- **Valeur que la forme ne peut pas porter** : elle passe dans la liste `{…}` sous son nom MMD.
  Titre de niveau 5 : `# Titre {level=5}` ; `start` sur un élément qui n'est pas le premier de sa
  liste, ou valant 1, ou hors de 0 à 999 999 999 : `{start=…}` ; langue avec une espace :
  `` ``` {language="mon langage"} `` ; adresse d'image avec une espace : `![légende]() {url="…"}` ;
  LaTeX qui contient une ligne `$$` : `$$ {latex="…"}`, suivi d'une ligne vide, puis `$$`.
- **Prop inconnue du registre** : elle se lit (son nom tel quel, son type d'après son écriture :
  guillemets, chaîne ; nombre sans point, entier ; avec point, flottant ; `true` ou `false`), mais
  **une écriture qui en porte une est refusée**, sur un bloc comme sur une cellule : l'éditeur
  l'effacerait sans bruit à la première modification du bloc. Une page qui en porte une se lit,
  mais son texte ne se récrit pas tel quel par `replace_page` ; les opérations ciblées n'y touchent
  pas.
- **Balises** : attributs dans cet ordre : l'ancre `id`, `title` (toggle), les attributs portés par
  la forme (toujours écrits, même à leur défaut : `<callout icon="💡" color="default">`), les autres
  props triées, puis l'ancre `anchor` pour `<page>` et `<database>`. Pas d'espace avant `/>`.
- **Enfants de tout bloc** : les enfants d'un bloc à forme en ligne, à clôture ou autofermante
  (`<video/>`, `<page>…</page>`, `<p/>`, `<block/>`…) suivent sa dernière ligne (ancre et ligne
  `{…}` comprises), sans ligne vide, indentés de 2 espaces (de la largeur du marqueur pour un élément
  numéroté). Entre deux enfants, la séparation est la même qu'au premier niveau. Les balises
  `toggle`, `callout`, `columns`, `column` et `synced` contiennent leurs enfants entre leurs
  balises, sans ligne vide après l'ouvrante ni avant la fermante. Dans un callout, le premier enfant
  suit le contenu sans ligne vide.

## Contenu en ligne

- **Délimiteurs Markdown** pour un seul genre de marque (`**`, `*`, `***` pour gras et italique,
  `~~` ou `` ` ``) : deux genres imbriqués (`~~**a**~~`, `` **`a`** ``) ne se refermeraient pas
  devant une lettre, ils passent en balises.
- **Ordre des marques**, de l'intérieur vers l'extérieur : `<code>`, `<em>`, `<strong>`, `<s>`,
  `<u>`, puis `<span bg="…" color="…">`. Un délimiteur Markdown prend la place de sa balise, donc à
  l'intérieur de `<u>` et de `<span>` (`<span color="blue"><u>souligné coloré</u></span>`,
  `<u>**gras souligné**</u>`). Surlignage : `<span bg="…">` ; couleur et surlignage :
  `<span bg="…" color="…">`. Un `<span>` ne porte qu'un texte, d'une seule mise en forme
  (`<span color="red">a</span><span color="red"><em>b</em></span>`), jamais un lien, une mention,
  une formule, un `<br/>` ni un autre `<span>`.
- **Lien** : `[texte](href)` si le `href` ne contient ni blanc, ni caractère de contrôle, ni `(`,
  `)`, `<`, `>`, `\`, `&` ; sinon `<a href="…">texte</a>`. Un lien entoure ses textes marqués et ses
  `<span>` (`[**a**](x)`, `[<span bg="blue">c</span>](u)`). Deux liens voisins de même adresse n'en
  font qu'un : la lecture refuse `[a](x)[b](x)` et demande `[ab](x)`.
- **Formule en ligne** : `$…$` quand cette forme relit exactement la formule (`$\frac{a}{b}$`,
  `$\$5$`) ; sinon (formule vide, blanc en bord, `$` non échappé, `$$`, accolades déséquilibrées,
  saut de ligne, barre oblique finale…) `<math latex="…"/>`, l'attribut échappé comme les autres.
  Un `$` non échappé impose la balise **même entre accolades** (`<math latex="\text{aire
  $x^{2}$}"/>`). La lecture refuse la balise pour une formule que `$…$` porte.
- **Mention** : `<mention type="…" id="…">libellé</mention>`, où `type` est `page`, `user` ou
  `date`, `id` l'identifiant de la page ou du compte, ou la date `AAAA-MM-JJ`, et le libellé celui
  qui a été copié quand on l'a posée (le titre, le nom, la date écrite), échappé comme du texte.
  Une mention ne donne aucun droit sur ce qu'elle nomme.
- **Saut de ligne** dans un bloc : `<br/>` (hors d'un bloc de code, où c'est un saut de ligne du
  texte).
- **Bornes** : une couleur s'écrit en 100 octets au plus, une adresse de lien en 16 Kio au plus.

## Échappements

- Le jeu des règles de base (« Texte littéral », « Début de ligne »), plus `{` et `}` (écrits `\{`
  et `\}`), puisqu'une liste `{…}` porte des props.
- Un caractère de contrôle du texte (tabulation, saut de ligne…) s'écrit en référence numérique
  (`&#9;`, `&#10;`) ; dans un attribut, saut de ligne et retour chariot s'écrivent `&#10;` et
  `&#13;`. Le titre d'un `<page>` échappe `&`, `<`, `\n` et `\r`.

## Lecture stricte

Tout ce qui n'est pas la forme canonique est refusé, avec la ligne en cause et un message en
français qui dit quoi écrire : marqueurs `*` ou `+`, `N)`, titres soulignés, `#####`, code
indenté, clôture `~~~`, balise ou clôture non fermée, indentation incohérente, ligne vide manquante
ou en trop, liste `{…}` mal formée, accolade non échappée, fusions de cellules hors de la grille,
fins de ligne CRLF, imbrication de plus de 100 niveaux, colonnes hors de leur règle (`<column>` hors
de `<columns>`, `<columns>` d'une seule colonne ou qui contient autre chose, `<column>` vide,
`<columns>` directement dans une `<column>`). Après la lecture, l'arbre est récrit et doit redonner
l'entrée octet pour octet ; sinon, la première ligne qui diffère est signalée avec la forme
attendue (valeur par défaut écrite, échappement ou marque non canonique). L'import d'un Markdown
quelconque (Obsidian, Notion) est un autre convertisseur, prévu plus tard.

## Ce qu'une écriture d'agent refuse en plus

- **Les médias du web** : un bloc `image`, `video` ou `audio` dont l'adresse n'est ni celle d'un
  fichier de l'instance (`/api/v1/pages/<uuid>/files/<uuid>`), ni celle d'un média que la page
  porte déjà, est refusé (`external_media`), à la création comme dans une page. Un lien, un signet
  et un bloc `file` à adresse web passent.
- **Les contenus protégés** : une opération qui retirerait un bloc `<page>`, `<database>`,
  `<synced>` ou `<block>`, ou un bloc qui porte un fil de commentaires, est refusée
  (`protected_content`) sauf si elle porte `allow_deleting_content`.
- **Les bornes** : 12 000 blocs, lignes de tableau comprises, par page pour `create_pages` et
  `replace_page`, et, pour `update_page`, sur l'ensemble des blocs que les opérations d'un appel
  écrivent (au-delà, découpe-les en plusieurs appels) ; une écriture estimée à plus
  de 8 s est refusée (`too_long_to_write`, à découper en opérations plus petites) ; 100 niveaux
  d'imbrication ; 2 000 000 caractères par MMD, et 2 Mio de corps par appel.

## Lectures de pages venues de l'éditeur

Une page écrite par l'éditeur (ou par un client Yjs) peut porter ce que le MMD n'accepte pas en
écriture. Le serveur la lit toujours sous une forme que sa lecture accepte, sans rien écrire dans
le document :

- **Imbrication** : au-delà de 100 niveaux, les descendants d'un bloc du centième niveau sont lus à
  sa suite, au même niveau ; une liste de colonnes mal formée est défaite (ses blocs prennent sa
  place).
- **Tableaux** : un tableau aux lignes inégales, aux fusions qui sortent du tableau ou se
  recouvrent, est lu en une grille que la lecture accepte, sans perdre une cellule : la grille a
  la largeur de la ligne qui couvre le moins de colonnes, sans être plus étroite que celle qui a le
  plus de cellules ; un `colspan` ou un `rowspan` trop long est ramené ; une ligne trop courte
  reçoit des cellules vides à droite (au plus seize de plus que les cellules lues ; au-delà, le
  tableau se lit en une seule colonne, une cellule par ligne, sans fusion). `widths` porte une
  largeur par colonne de la grille de la première ligne, ni plus ni moins, 100 colonnes au plus ;
  `header-rows` et `header-cols` sont ramenés dans la grille.
- **Type hors registre** : lu `<block type="…" id="…" data="…"/>`, que l'on récrit tel quel.
