//! Serveur HTTP de Mementum : comptes et sessions, API REST des pages, MMD,
//! commentaires, historique, fichiers, favoris, duplication des pages, WebSocket
//! de synchronisation et canal de l'espace.
//!
//! | Route | Session | Réponse |
//! |---|---|---|
//! | `GET /health` | non | `ok` |
//! | `GET /api/v1/info` | non | [`Info`] : versions, `signup` (`open` ou `invite`), `setup` (aucun compte n'existe encore) |
//! | `POST /api/v1/auth/signup` | non | `201`, [`Me`], cookie de session (celle que portait déjà le navigateur est fermée, avec les connexions de synchro de son compte, comme à la connexion, Ruling R90 ; une inscription refusée ne ferme rien) ; le premier compte est toujours permis ; `403 signup_closed` ; `404 invalid_invitation` ; `409 email_taken` ; `422 invalid_email`, `invalid_name`, `invalid_password` |
//! | `POST /api/v1/auth/login` | non | [`Me`], cookie de session (celle que portait déjà le navigateur est fermée, avec les connexions de synchro de son compte, Ruling R90) ; `401 invalid_credentials` (aussi quand le mot de passe du compte change pendant sa vérification : aucune session n'est ouverte, Ruling R120) ; `429 too_many_attempts` (avec `Retry-After`) |
//! | `POST /api/v1/auth/logout` | non | `204`, session supprimée, cookie effacé, connexions de synchro du compte fermées (Ruling R73) ; sans session : `204` aussi |
//! | `POST /api/v1/auth/password` | oui | `200`, `PasswordChanged` (`tokensRevoked` : les jetons d'API vivants que le changement a révoqués, Ruling R1221) : toutes les sessions du compte sont supprimées, tous ses jetons d'API révoqués, et ses connexions de synchro fermées ; la demande repart avec une session neuve (cookie de la réponse) ; `401 invalid_credentials` (mot de passe actuel faux, ou remplacé pendant sa vérification : rien n'est changé, Ruling R120) ; `422 invalid_password` ; `429 too_many_attempts` (avec `Retry-After` ; frein à part de celui de la connexion, Ruling R76) |
//! | `GET /api/v1/me` | oui, ou un jeton | [`Me`] : le compte et ses espaces ; par un jeton d'API, `AgentMe` : le compte **sans son adresse**, les espaces de la portée du jeton (avec le rôle restreint au jeton), et le jeton (lot 5, Ruling R1247) |
//! | `GET /api/v1/tokens` | oui, jamais par jeton | `TokenInfo[]` : les jetons d'API du compte qui se lisent encore (vivants, ou morts depuis moins de quatre-vingt-dix jours, Ruling R1270), les vivants d'abord ; ni leur texte ni leur hachage, leurs quatre derniers caractères ; le titre d'une page de leur portée seulement si le compte la voit encore, le nom de leur espace seulement s'il y a un rôle (Ruling R1368) |
//! | `POST /api/v1/tokens`, corps `CreateToken` | oui, jamais par jeton, et le mot de passe du compte (décision L2) | `201`, `TokenCreated` : le jeton (`mmt_…`), montré cette fois seulement, et sa ligne de la liste ; refus dans l'ordre : `422 invalid_name`, `invalid_level` (ni `read` ni `edit`, Ruling R1218 ; ou `read` avec l'option corbeille, R1520), `invalid_scope` (une portée mal formée), `invalid_expiry` ; `429 too_many_attempts` (dix mots de passe faux en quinze minutes, fenêtres à part de la connexion et du changement de mot de passe, Ruling R1366) ; `401 invalid_credentials` (mot de passe faux, ou remplacé pendant sa vérification : rien n'est créé, Ruling R120) ; `422 invalid_scope` (un espace sans rôle, une page qu'on ne voit pas ou à la corbeille, un identifiant inconnu ou mal écrit : une seule réponse, Ruling R1367) ; `409 token_limit` (vingt jetons vivants, Ruling R1220) |
//! | `DELETE /api/v1/tokens/{id}` | oui, jamais par jeton ; le jeton est le sien | `204` : le jeton ne vaut plus dès l'appel suivant (la révocation va au bout si le client coupe, Ruling R1376) ; déjà révoqué : `204`, rien n'est écrit ; un jeton d'un autre compte, inconnu, mal nommé, ou mort depuis quatre-vingt-dix jours : `404`, la même réponse |
//! | `GET /api/v1/tokens/{id}/calls?before=&outcome=` | oui, jamais par jeton ; le jeton est le sien (mort compris, tant qu'il est gardé) | `TokenCalls` : son journal d'activité, cinquante lignes au plus, de la plus récente à la plus ancienne (`before` : le rang de la dernière lue ; `outcome` : `ok`, `refused` ou `error`), avec les refus du débit comptés et les lignes purgées ; le titre d'une page seulement si le compte la voit encore ; `400 invalid_request` pour une adresse qui ne se lit pas, avant tout `404` (le jeton n'est pas encore cherché, Ruling R1378) ; puis le `404` de la révocation |
//! | `POST /api/v1/spaces` | oui | `201`, `SpaceInfo` : l'espace créé, dont le compte est `owner` ; `422 invalid_name` |
//! | `PATCH /api/v1/spaces/{id}` | `admin` | `SpaceInfo` ; `422 invalid_name` |
//! | `GET /api/v1/spaces/{id}/members` | `member` | `Member[]` : invités compris, dans l'ordre d'arrivée ; un invité : `403` ; par un jeton d'API, `MemberName[]` : noms et rôles, jamais les adresses (Ruling R1247) |
//! | `PATCH /api/v1/spaces/{id}/members/{userId}` | `admin` (entre `admin` et `member`) ou `owner` | `Member` ; `404` si la personne n'a pas de rôle dans l'espace ; `409 last_owner` ; `409 roles_changed` (les rôles ont bougé à chaque reprise) |
//! | `DELETE /api/v1/spaces/{id}/members/{userId}` | `admin` (un admin ne retire pas un `owner`, un `owner` le peut), ou soi-même, quel que soit son rôle | `204` ; `404` comme ci-dessus ; `409 last_owner` ; `409 roles_changed` |
//! | `POST /api/v1/spaces/{id}/invitations` | `admin` | `201`, `InvitationCreated` (le jeton, montré cette fois seulement ; rôle `admin` ou `member`) ; `422 invalid_email` |
//! | `GET /api/v1/spaces/{id}/invitations` | `admin` | `PendingInvitation[]` : celles qui attendent, sans jeton (une invitation de page, seulement si l'on peut partager la page, avec le titre de sa page, `pageTitle`) |
//! | `DELETE /api/v1/invitations/{id}` | invitation d'espace : `admin` de son espace ; invitation de page : `full` sur la page, `admin` ou non | `204` ; `404` sous `admin` ou sans le droit de partager la page, comme pour une invitation inconnue, échue, acceptée, ou d'un espace où l'on n'est rien |
//! | `POST /api/v1/invitations/lookup` | non | `InvitationInfo` ; `404 invalid_invitation` (aussi pour une invitation dont l'auteur n'a plus le droit de la faire, Ruling R69) |
//! | `POST /api/v1/invitations/accept` | oui | `Accepted` (rien de ce qu'avait le compte ne baisse) ; `changed: false` si elle ne lui donne rien (un rôle au moins égal ; pour une page, un niveau effectif au moins égal) : rien n'est écrit, et elle reste valable (Ruling R257) ; `404 invalid_invitation` |
//! | `GET /api/v1/spaces/{id}/tree` | un rôle | [`Tree`] : les pages de l'espace que la personne voit, hors corbeille, avec son niveau et sa section, et la version de l'espace (`lsn`), lue avant les droits et les pages : l'arbre est au moins aussi récent qu'elle (Ruling R141) |
//! | `GET /api/v1/spaces/{id}/trash` | un rôle | [`PageNode`]`[]` : les pages mises elles-mêmes à la corbeille, là où son niveau permet de restaurer (`edit`), la plus récente d'abord ; si une page parente y est aussi, la restauration répond `409 parent_trashed` |
//! | `GET /api/v1/spaces/{id}/favorites` | un rôle | `Favorites` : les pages favorites du compte dans l'espace, celles qu'il voit, hors corbeille, dans l'ordre où il les a ajoutées ; une page qu'il ne voit plus, ou à la corbeille, n'y est pas, et y revient à sa place avec l'accès, ou restaurée (Ruling R254) |
//! | `POST /api/v1/spaces/{id}/search`, corps `SearchRequest` (`q`, `after`, et les filtres du lot 5 : `createdBy`, `editedAfter`, `editedBefore`, qui ne font que réduire les pages que le compte voit, avant l'index, Ruling R1248 ; `422 invalid_filter`) | un rôle, invité compris | `SearchResults` : les pages de l'espace que le compte voit, hors corbeille, qui portent tous les mots cherchés dans leur titre ou dans un même bloc, le dernier mot aussi comme début de mot ; la mieux notée d'abord, vingt par réponse, cent en tout (`after`), sans total ; chacune avec son titre de l'instant, les ancêtres que le compte voit, le bloc trouvé, un extrait et les places des mots (lot 4, Task S5, `routes/search.rs`) ; `indexing` : l'index n'est pas à jour pour une page qu'il voit. Un `POST` qui n'écrit rien : la recherche n'est dans aucune adresse ni aucun journal (Ruling R917). Le corps (4 Kio au plus, `413` au-delà) est jugé après les droits : `404` sans rôle, quel que soit le corps ; puis `400` ou `422 invalid_json` ; `422 invalid_query` au-delà de 200 caractères, de 8 mots, ou de la centième page ; `429 search_busy` (`Retry-After: 1`) pour une recherche en attente qu'une plus récente du compte a remplacée (une admise et une en attente par compte, quatre calculs de front pour l'instance, Ruling R910). La réponse d'une recherche calculée ne part jamais avant 50 ms (`SEARCH_FLOOR`, Ruling R932) ; une recherche sans aucun mot répond vide tout de suite |
//! | `POST /api/v1/spaces/{id}/pages/batch`, corps `CreatePages` | racine : membre (et « Modification » pour un jeton), dans les deux sections ; sous une page : `edit` sur elle | `201`, `PageNode[]` : vingt pages au plus, avec leur contenu, toutes ou aucune, en une transaction dans la voie des copies, sur le débit des duplications du compte (lot 5, Rulings R1243, R590 : droit de créer et jeton relus dans la transaction) ; `422 too_many_pages`, `invalid_parent`, `invalid_mmd` (avec `index` et `line`), `external_media` (par un jeton, Ruling R1235), `too_many_blocks`, `too_long_to_write`, `invalid_request` (`section` avec une page parente), `title_too_long`, `subtree_too_large` (plus de 16 Mio de documents bâtis) ; `409 page_moved` (la destination a bougé trois fois pendant l'attente du tour d'écriture) ; `429 duplication_busy` ; un MMD sans fin de ligne finale est lu comme s'il en avait une (Ruling R1521) (`routes/agent.rs`) |
//! | `GET /api/v1/pages/{id}/document?anchors=block&from=&since=&limit=` | `read` (à la corbeille aussi) | `PageDocument` : métadonnées, révision et une tranche du MMD (24 Kio par défaut, 48 Kio au plus), reprise par `from=<next>` ; avec `since`, ce qui a changé depuis dans les blocs déjà servis (lot 5, Rulings R1223, R1225, R1226) ; `422 stale_anchor` (un `from` qui ne désigne aucun bloc), `422 ambiguous_anchor` (plusieurs, Ruling R1522) ; `400 invalid_request` (`routes/agent.rs`) |
//! | `POST /api/v1/pages/{id}/operations`, corps `PageOperations` | `edit` | `UpdateResult` : les opérations d'`update_page`, toutes ou aucune, écrites par l'acteur de la page, par différence (lot 5, Rulings R1228 à R1235) ; refus détaillés en tête de `routes/agent.rs` (`operation_failed`, `revision_conflict`, `revision_required`, `page_in_use`, `version_refused`, `external_media`, `page_busy`…) |
//! | `POST /api/v1/pages` | racine : `member` (un invité : `403`) ; sous-page : `edit` sur le parent | `201`, [`PageNode`] : le nœud, ses accès et son document, écrits ensemble ; `422 title_too_long` et le `404` d'un `spaceId` mal formé passent avant les droits ; `404 not_found` pour un `spaceId` mal formé, inconnu ou d'un espace où l'on n'a aucun rôle ; `409 trashed` sous un parent à la corbeille ; `422 invalid_parent`, `invalid_after`, `title_too_long` ; `422 invalid_json` sans `spaceId` à la racine, ou avec `section` sous un parent |
//! | `GET /api/v1/pages/{id}` | `read` | [`PageNode`] |
//! | `PATCH /api/v1/pages/{id}` | titre, place sous le même parent : `edit` ; autre parent : `full`, et `edit` sur le nouveau parent (racine : `member`) | [`PageNode`] ; seule route qui lit son corps entre le `404` et le `403` : `422 invalid_json` et `title_too_long` passent avant tout refus de niveau, `422 invalid_parent` avant celui du déplacement, `invalid_after` après lui ; `409 trashed` pour une page à la corbeille, ou sous un nouveau parent à la corbeille ; `409 cycle` ; `409 page_moved` si la page a changé de place, ou ce qu'on peut en faire, à chacune des quatre tentatives : rien n'est écrit (Ruling R91). À la racine, la page garde son accès général et qui la déplace y a `full` (Rulings R82 et R94) |
//! | `POST /api/v1/pages/{id}/trash` | `edit` | [`PageNode`] : la page et son sous-arbre sont à la corbeille ; `409 trashed` si elle y est déjà, elle-même ou par une page parente |
//! | `POST /api/v1/pages/{id}/restore` | `edit` | [`PageNode`] : la page et son sous-arbre reviennent (sans effet hors corbeille) ; `409 parent_trashed` si une page parente y est |
//! | `POST /api/v1/pages/{id}/duplicate` | `read`, hors corbeille ; et un endroit où poser la copie : `CreateChild` sur la page parente, sinon être membre de l'espace | `201`, [`PageNode`] : la racine de la copie de la page et de ses sous-pages que le compte voit, hors corbeille (Ruling R252). Contenus récrits dans des documents neufs, **sans aucun identifiant de bloc repris** ; fichiers que les blocs montrent attachés de nouveau à la copie, adresses récrites ; titre suivi de « (copie) » ; ni commentaires, ni versions, ni favoris, ni accès nommés. La copie va sous la page parente, juste après la page, sans accès propre, si le compte voit la page parente et peut y créer ; sinon, pour un membre, à la racine, en dernier, dans son « Privé » (`(user:<lui>, full)`) ; sinon `403 forbidden`. Puis `409 trashed` ; `422 invalid_request` pour un dossier ; `429 duplication_busy`, avec `Retry-After`, si le compte duplique déjà (une à la fois, Ruling R605) ou a bâti plus de 64 Mio de documents dans la dernière minute (Ruling R606) : refusé tout de suite, jamais mis en attente ; `422 subtree_too_large` au-delà de 100 pages, ou de 16 Mio de documents (Rulings R592, R608). Les droits sont relus juste avant d'écrire, une fois les documents bâtis (Ruling R89), puis dans la transaction qui écrit (Ruling R590) : si la page ou la destination bouge pendant qu'elle attend son tour, la route redécide, trois fois au plus, puis `409 page_moved` |
//! | `DELETE /api/v1/pages/{id}` | `full` | `204` : la page et son sous-arbre sont supprimés pour de bon, avec leurs attaches de fichiers ; puis, avant la réponse, le ménage retire du disque les contenus que plus aucune page n'attache, et eux seuls (Rulings R245, R557 ; une erreur du ménage est journalisée, sans changer la réponse) ; `409 not_trashed` si ni elle ni une page parente n'est à la corbeille (Ruling R65) |
//! | `PUT`, `DELETE /api/v1/pages/{id}/favorite` | `read` (à la corbeille aussi) | `204` : la page devient, ou n'est plus, un favori du compte ; déjà dans cet état : `204`, rien n'est écrit ; un favori n'avance ni `spaces.rev` ni `spaces.lsn`, et rien n'est annoncé sur le canal de l'espace (Ruling R254) |
//! | `GET /api/v1/pages/{id}/backlinks` | `read` (à la corbeille aussi) | `Backlinks` : les pages qui mènent à la page (un lien de page, une mention de page, un lien du texte vers son adresse), telles que l'indexeur les a projetées (lot 4, Task S6) : seulement celles que le compte voit, de l'espace de la page, hors corbeille, triées par droits **avant** d'être comptées, dans l'ordre de leurs identifiants, deux cents au plus ; pour chacune, le premier bloc qui y mène dans l'ordre des identifiants de bloc (`null` pour une clé de remplacement) et son texte coupé par la base à 240 caractères ; rien ne dit combien de sources ont été lues ni s'il en reste. Les sources se lisent par lots de 500, jusqu'à 10 000 lues : au-delà, des pages vues peuvent manquer (limite L-j) ; les liens d'un autre espace ne sont même pas lus (R1015). La réponse d'une lecture qui passe ne part jamais avant 150 ms après le début du traitement, que le parcours ait été long ou court (Ruling R1022 : sans ce plancher, sa durée comptait les sources non vues) ; les refus ne l'attendent pas. Jamais `403`, jamais `409 trashed` |
//! | `GET /api/v1/pages/{id}/mmd?anchors=block` | `read` | `text/markdown; charset=utf-8`, avec `ETag` : la révision des blocs rendus, lue dans la même commande de l'acteur (lot 5, Ruling R1223) ; `?anchors=block` : chaque bloc porte son alias (`^bk7qa`, Ruling R1224) |
//! | `PUT /api/v1/pages/{id}/mmd`, en-tête `If-Match` facultatif | `edit`, par une session (fermé aux jetons, R1222) | `204` avec `ETag` (la révision de l'état écrit) : le contenu est remplacé **par différence** (lot 5, R1227 : seuls les blocs qui diffèrent sont écrits, une frappe concurrente dans un bloc qu'il ne touche pas est gardée) ; `409 revision_conflict` (avec `revision`, la courante) si la révision nommée par `If-Match` n'est plus celle de la page, jugé dans la commande qui écrit, rien d'écrit ; `400 invalid_request` pour un `If-Match` qui ne se lit pas ; et un bloc écrit sans ancre garde l'identifiant du bloc courant qui lui correspond (même liste de frères, même contenu, ou même place et même type ; Ruling R201) ; `422 invalid_mmd` (avec `line`) ; `422 too_many_blocks` au-delà de 12 000 blocs, tous niveaux et lignes de tableau comprises, avant le verrou et le chargement de la page (Ruling R259) ; `422 too_long_to_write` si l'écriture est estimée au-delà de 8 s d'acteur (`mementum_crdt::write_cost` : une liste trop longue, frères, lignes ou cellules d'un tableau, morceaux du contenu en ligne d'un bloc, ou trop d'éléments), au même moment (Ruling R368) ; `422 invalid_blocks` : prop hors registre sur un bloc ou une cellule (Ruling R41) ; une structure que BlockNote refuserait (colonnes, imbrication) est refusée dès la lecture, en `422 invalid_mmd` ; `429 page_busy` (avec `Retry-After: 1`) si la file des écritures de la page compte déjà 8 requêtes, ou 2 du même compte (restaurations comprises), une fois le MMD lu et validé (un MMD refusé n'y tient jamais de place ; Rulings R435, R494, R535) : un agent qui envoie trois `PUT /mmd` de front sur une même page peut recevoir `429` pour le troisième, dès que les deux premiers sont encore en file ou en cours d'écriture, et écrit donc une page une requête à la fois, ou réessaie après `Retry-After` ; la lecture du MMD et ces deux estimations se font hors des fils du serveur, quatre de front, les autres à leur tour, les comptes tour à tour (Rulings R383, R436) ; une écriture du serveur à la fois par page, avec la restauration : sous le verrou de la page, une fois la page chargée, chaque requête relit sa session (`401` si elle a pris fin pendant l'attente, Ruling R209) et ses droits (Ruling R89) après la précédente, et va au bout même si le client coupe (Ruling R200) |
//! | `GET /api/v1/pages/{id}/access` | `full` (à la corbeille aussi) | `PageAccess` : l'accès général effectif et son plancher (celui d'une page parente), les comptes nommés sur la page ou ses ancêtres, les invitations de la page (avec son titre, `pageTitle`) |
//! | `PUT /api/v1/pages/{id}/access/general` | `full`, et le rôle `member` ou plus (un invité : `403`, R85) | `PageAccess` (`204` seulement si un autre changement ôte `full` à l'appelant entre le commit et la relecture) ; `level: null` retire la ligne `members` de la page ; qui restreint l'accès général (ligne retirée, ou baissée) y garde `full` : sa ligne `(user:<lui>, full)` est posée dans la même transaction s'il ne le garde pas autrement, et il figure dans la réponse (R122) ; le plancher est l'accès qu'une page parente donne (R93) : au plancher, la ligne propre part : ce `PUT` n'en laisse pas de redondante ; `409 inherited_access` sous lui, ou pour `null` sur une page sans ligne propre qui a un plancher (sans plancher, `null` sans ligne propre ne change rien) |
//! | `PUT /api/v1/pages/{id}/access/users/{userId}` | `full` (un invité aussi, R86) | `PageAccess` (`204` si l'appelant n'a plus `full`) ; `404` si le compte n'a aucun rôle dans l'espace ; `409 last_full_access` |
//! | `DELETE /api/v1/pages/{id}/access/users/{userId}` | `full` | `PageAccess` (ou `204`) ; `404` si la page n'a pas de ligne propre pour ce compte ; `409 last_full_access` ; un invité sans plus aucun accès sort de l'espace, avec ses invitations en attente |
//! | `POST /api/v1/pages/{id}/invitations` | `full` | `201`, `InvitationCreated`, la même réponse que l'adresse ait un compte ou non ; `422 invalid_email` |
//! | `GET /api/v1/pages/{id}/comments` | `read` (à la corbeille aussi) | `Thread[]` : les fils de la page, résolus compris, du plus ancien au plus récent, chacun avec qui l'a ouvert (`openedBy`, qui le reste quand son premier message est supprimé) ; chaque message avec son auteur (identifiant et nom, jamais l'adresse) et ses réactions (Ruling R134) |
//! | `POST /api/v1/pages/{id}/comments` | `comment` | `201`, `Thread` : un fil neuf et son premier message ; `422 invalid_body` ; `422 invalid_block` si `blockId` ne nomme aucun bloc du document vivant de la page : une seule attente de l'acteur, qui le dit, puis la session relue (`401` si elle a pris fin pendant l'attente) et les droits revérifiés, avant toute réponse sur le bloc (Rulings R89, R384) ; sans `blockId`, la page n'est pas chargée ; `409 thread_limit` si la page porte déjà 2 000 fils, résolus compris (Ruling R204), ou si la personne y en a déjà ouvert 200, sa part (Ruling R351) ; `409 page_comment_limit` si la page porte déjà 10 000 messages, ou si le message ferait passer leur texte au-delà de 4 Mio (Ruling R350), ou la part de la personne au-delà de 2 000 messages ou 1 Mio de texte sur la page (Ruling R358) |
//! | `POST /api/v1/pages/{id}/comments/{threadId}/replies` | `comment` | `201`, `Comment` ; `422 invalid_body` ; `409 message_limit` si le fil porte déjà 500 messages (Ruling R204) ; `409 page_comment_limit` comme à l'ouverture d'un fil (Rulings R350, R358) |
//! | `PATCH /api/v1/pages/{id}/comments/{threadId}` | l'avoir ouvert avec `comment`, ou `edit` | `Thread` : résolu (`{"resolved": true}`) ou rouvert ; déjà dans cet état, rien ne change |
//! | `PATCH /api/v1/pages/{id}/comments/{threadId}/messages/{commentId}` | `comment`, et en être l'auteur | `Comment` ; le même texte ne change rien, pas même `editedAt` ; `422 invalid_body` ; `409 page_comment_limit` si le texte, plus long, ferait passer celui de la page au-delà de 4 Mio (Ruling R350), ou celui de son auteur au-delà de 1 Mio (Ruling R358) |
//! | `DELETE /api/v1/pages/{id}/comments/{threadId}/messages/{commentId}` | en être l'auteur avec `comment`, ou `full` | `204` : le message part avec ses réactions, le fil avec son dernier message |
//! | `DELETE /api/v1/pages/{id}/comments/{threadId}` | `full` | `204` : tout le fil part, avec les messages de tous et leurs réactions, en une écriture ; refus dans l'ordre de R168 (page, niveau, fil, corbeille) ; un fil qui n'est pas, ou plus, de la page : `404`, et aucune trame (Ruling R352) |
//! | `PUT`, `DELETE /api/v1/pages/{id}/comments/{threadId}/messages/{commentId}/reactions/{emoji}` | `comment` | `Comment` : la réaction de la personne posée ou retirée (on ne retire que la sienne ; déjà dans cet état, rien ne change) ; `422 invalid_emoji` ; `409 reaction_limit` pour un emoji neuf sur un message qui en porte déjà 32 différents (Ruling R204), ou pour un emoji neuf pour la personne quand elle en a déjà posé 8 sur ce message, sa part (Ruling R351) ; pour toute réaction neuve si la page en porte déjà 20 000 (Ruling R357), ou si la personne y en a déjà posé 2 000, sa part (Ruling R358) |
//! | `GET /api/v1/pages/{id}/versions` | `edit` (à la corbeille aussi ; Ruling R203) | `Version[]` : les versions de la page, la plus récente d'abord, cent au plus, chacune avec sa date, sa raison (`periodic`, `closing`, `before_restore`, `before_agent`) et les comptes qui l'ont modifiée depuis la version d'avant (identifiant et nom, jamais l'adresse) (Ruling R135), dont ceux qui l'ont fait par un jeton d'API (`agentAuthors`, jamais le jeton ; lot 5) |
//! | `GET /api/v1/pages/{id}/versions/{versionId}/state` | `edit` (à la corbeille aussi ; Ruling R203) | `application/octet-stream` : l'état Yjs (v1) de la version, tel qu'il a été gardé ; une version qui n'est pas de cette page, inconnue ou mal nommée reçoit la réponse d'une page introuvable, après le `403` de niveau, comme sur la route suivante |
//! | `POST /api/v1/pages/{id}/versions/{versionId}/restore` | `edit` | `204` : une version `before_restore` garde ce qui est remplacé si aucune version ne le garde déjà (Ruling R190), puis les blocs de la version sont écrits par-dessus le contenu, identifiants compris, comme une écriture ordinaire (la version que la page a déjà comme contenu ne change rien) ; une version qui n'est pas de la page : `404`, avant `409 trashed` (Ruling R186) ; droits revérifiés une fois la page chargée (Ruling R89), une écriture du serveur à la fois par page, la session relue sous le verrou, comme `PUT …/mmd` (Rulings R200, R209) ; `429 page_busy` (avec `Retry-After: 1`) si la file des écritures de la page compte déjà 8 requêtes, ou 2 du même compte, `PUT …/mmd` compris, une fois la version trouvée et la corbeille vérifiée (Ruling R535) ; `422 invalid_blocks` si les blocs de la version ne s'écrivent pas (vérifiés avant toute coupe, Ruling R190) ; `409 restore_refused`, la page intacte, si l'état gardé de la version ne se relit pas, ou si celui de la page ne se relit pas et que la version d'avant restauration ne pourrait pas le garder (Rulings R816, R832) |
//! | `GET /api/v1/files/limits` | oui | `FileLimits` : la taille la plus grande d'un fichier envoyé (`MEMENTUM_MAX_FILE_MB`, 100 Mio par défaut) |
//! | `POST /api/v1/pages/{id}/files?name=…` | `edit` | `201`, `FileInfo` (son `url`, `/api/v1/pages/{id}/files/{fileId}`, est l'adresse du fichier) : le corps est le fichier brut, lu par trames et écrit dans le dépôt sans être gardé en mémoire, hors de la borne de 2 Mio des autres corps ; `409 trashed` ; `422 invalid_request` pour un paramètre `name` illisible (absent : « fichier ») ; ces refus faits à un compte (`400` et `409` de l'en-tête `X-Mementum-Account`, `403`, `404`, `409`, `422`) lisent et jettent d'abord un corps annoncé sous la borne, ou sans longueur annoncée, sauf sous `Expect: 100-continue`, pour arriver au navigateur (Rulings R505, R545) ; `400 invalid_request` pour un corps interrompu avant sa fin ; `413 file_too_large` sur `Content-Length`, avant de rien lire (comme le `401`, il peut être perdu pour un client qui envoie son corps sans attendre : lire `GET /api/v1/files/limits` avant d'envoyer), puis en cours de lecture ; un corps qui ne reçoit plus rien pendant 30 s : la connexion est fermée sans réponse (Ruling R391, en HTTP/1) ; vidé sur le disque, puis session et droits revérifiés, le corps reçu (Rulings R89, R390, R507) ; le contenu est posé puis attaché sous le verrou du dépôt, et va au bout même si le client coupe ; un contenu posé par l'envoi que l'attache n'a pas pris repart (Ruling R395) |
//! | `POST /api/v1/pages/{id}/files/copies` | `edit` ici, et `read` sur la page d'origine (à la corbeille aussi) | `201`, `FileInfo` : une attache neuve de cette page, au même contenu et au même nom, sans un octet renvoyé ; corps `CopyFile` (`pageId`, `fileId`) ; `409 trashed`, aussi pour une page mise à la corbeille pendant que la recopie attend son tour d'écriture, que la transaction relit (Ruling R577) ; puis `invalid_json`, puis `422 invalid_source`, la même réponse pour une page d'origine qu'on ne voit pas, inconnue ou mal nommée, et pour un fichier qui n'est pas d'elle, inconnu ou mal nommé (Ruling R558) ; un `GET` à cette adresse répond `405 method_not_allowed`, pour toute page, vue ou non : la route passe avant celle d'un fichier (Ruling R559) |
//! | `GET /api/v1/pages/{id}/files/{fileId}` | `read` (à la corbeille aussi) | les octets, toujours en téléchargement (`Content-Disposition: attachment`, `nosniff`, CSP `sandbox`, `Cross-Origin-Resource-Policy: same-origin`), sous le type que le serveur a lu à l'envoi (Ruling R246) ; `Cache-Control: private, no-cache` et `ETag` ; `304` si `If-None-Match` nomme l'`ETag`, jugé avant `Range` ; `206` pour une plage simple, `416` si elle commence après la fin ou est de longueur nulle (`bytes=-0`) ; `If-Range` qui ne nomme pas l'`ETag`, deux plages ou une plage illisible : le fichier entier (Ruling R392) ; un fichier qui n'est pas de la page, inconnu ou mal nommé : la réponse d'une page introuvable, après celle de la page ; `404` « fichier indisponible » si le dépôt n'a plus son contenu, ou plus entier (Ruling R393), jugé après le `304` |
//! | `POST /mcp`, un message JSON-RPC (MCP 2026-07-28, ou 2025-11-25) | un jeton d'API, jamais une session (le cookie n'est pas lu) | le serveur MCP intégré (lot 5, Task M2, Rulings R1236, R1237, R1240 ; `routes/mcp.rs`) : six outils (`search`, `fetch`, `create_pages`, `update_page`, `manage_pages`, `workspace_info`), chacun menant la fonction d'`agent/` de sa route REST, au nom du compte du jeton restreint à lui ; `200` et un résultat (un refus d'outil est un résultat `isError` qui dit son code, quoi faire et, pour un débit, `retry_after_seconds` ; le débit d'un jeton y est un refus `token_busy` pour un corps annoncé de 64 Kio au plus, sinon le `429` HTTP sans que le corps soit lu, toujours sans ligne de journal), `202` pour une notification, et pour une erreur JSON-RPC le statut que dit la règle `statuses` de `spec/mcp/messages.json` (`400` pour ce qui empêche de lire la requête, `404` pour une méthode inconnue en 2026-07-28, `200` en 2025-11-25 et pour une erreur des params d'une méthode servie) ; les méthodes hors outils (`initialize`, `ping`, `server/discover`, `tools/list`, `resources/list`, `resources/read`) restent servies au-delà du débit, sans être comptées : leurs réponses sont fixes et ne lisent rien de la base (Ruling R1525) ; sans jeton, `401 invalid_token` (avec `WWW-Authenticate: Bearer`), avant toute lecture du corps ; un corps de plus de 2 Mio, `400` et `-32600` (découper en plusieurs appels) ; `403 bad_origin` pour une `Origin` d'un autre site ; `GET` et `DELETE` : `405` ; aucun en-tête CORS, `Cache-Control: no-store` et `nosniff` sur toute réponse |
//! | `POST /mcp/readonly` | un jeton d'API | la même, avec les seuls outils de lecture (`search`, `fetch`, `workspace_info`) : un outil d'écriture y est un outil inconnu (`-32602`), jamais appelé, quel que soit le niveau du jeton |
//! | `GET /sync/{id}?schema=N` | `read` | WebSocket y-websocket, au nom du compte : en écriture avec `edit` hors corbeille, pour un client du schéma du serveur ; sinon en lecture seule ; refus avant la poignée de main, dans cet ordre : `403 bad_origin`, `401`, `409 account_changed` (paramètre `account`, ou en-tête `X-Mementum-Account` ; Ruling R541), `404` ; `400 invalid_request` sans demande de WebSocket |
//! | `GET /events/{spaceId}` | un rôle | WebSocket du canal de l'espace (Ruling R133) : des trames texte `{"lsn":N}`, la version de l'espace, une à l'ouverture puis une chaque fois qu'elle avance, et un ping toutes les 20 s, précédé de la version courante redite, même égale (Ruling R205 : le client, qui ne voit pas les pings, s'en sert comme signe de vie) ; rien d'autre. Toute trame de données reçue du client ferme la connexion ; la session et le rôle sont revérifiés avant de mettre en file chaque trame de version et chaque ping, et la connexion est fermée s'ils ne valent plus ; aucun pong pour les trois derniers pings : fermée au ping suivant (Ruling R153) ; plus de 64 trames en attente : fermée. Refus avant la poignée de main, dans l'ordre de `/sync` : `403 bad_origin`, `401`, `409 account_changed` (paramètre `account`, ou en-tête `X-Mementum-Account` ; Ruling R541), `404` (espace inconnu, mal nommé, ou sans rôle : la réponse de tout espace introuvable, Ruling R152), `400 invalid_request` |
//!
//! Toute erreur est en JSON : `{ "error": <code>, "message": <texte> }`
//! ([`types::ErrorBody`]). Une route qui demande une session répond `401
//! unauthenticated` sans elle. Un corps illisible reçoit `invalid_json` (ou
//! `invalid_request`), un corps de plus de 2 Mio `413 payload_too_large` (hors
//! de l'envoi d'un fichier, qui a sa borne), une route inconnue `404
//! not_found`, une méthode inconnue `405 method_not_allowed`. Une page dont
//! l'acteur ne répond plus reçoit `503 unavailable`, sur `GET` et `PUT …/mmd`
//! (Ruling R26), sur `POST …/comments` avec un `blockId`, sur la restauration
//! d'une version, et sur `POST …/duplicate` (qui lit les blocs des pages copiées). La base SQLite est le fichier [`config::DB_FILE`] du dossier
//! des données, et les fichiers envoyés sont dans son dossier `files/`.
//!
//! **Compte attendu** (`auth.rs`, Rulings R540 à R542, R545). Une requête peut nommer le compte
//! sous lequel elle part : l'en-tête `X-Mementum-Account: <id>`, ou, pour `/sync` et `/events`, à qui
//! un navigateur ne laisse poser aucun en-tête, le paramètre d'adresse `account=<id>`. Le client
//! de l'application le pose sur ses appels, sauf `GET /api/v1/me` (qui sert à apprendre le
//! compte), la connexion, l'inscription et la déconnexion : un autre onglet du même navigateur a pu
//! se connecter sous un autre compte, et le cookie, commun, est alors le sien. Toute route qui lit
//! une session le vérifie dans l'extracteur de la session (le paramètre, dans `caller`, `routes/sync.rs`),
//! juste après le `401` et avant tout le reste (page, rôle, niveau, corbeille, corps) : un autre compte, ou un compte qui n'existe pas,
//! reçoit `409 account_changed`, jamais `401` (la session vaut ; le client ne doit pas la tenir
//! pour perdue) ; donné plusieurs fois, ou qui ne se lit pas comme un identifiant, `400
//! invalid_request`. Rien de ce que la route ferait n'est lu ni écrit ; seule la session l'est,
//! comme pour toute requête qui la porte. Sans en-tête ni paramètre, rien ne change (agents,
//! outils, tests). La connexion, l'inscription, la déconnexion, `GET /health`, `GET /api/v1/info` et
//! `POST /api/v1/invitations/lookup` ne lisent pas de session : l'en-tête n'y change rien, et se déconnecter ferme toujours la session que porte le navigateur
//! (Ruling R542). L'envoi d'un fichier lit et jette son corps avant ce refus, comme avant les
//! autres refus faits à un compte (Rulings R505, R545).
//!
//! **Jetons d'API** (lot 5, `auth.rs` et `agent/`, Rulings R1213 à R1218, R1222, R1241, R1242,
//! R1244). Un appel peut venir d'un agent, avec `Authorization: Bearer mmt_…` : c'est alors un
//! appel du compte du jeton, restreint (la colonne « Session » vaut pour lui, sous la portée, le
//! niveau et l'interdit du jeton). Seul un `Bearer` qui commence par `mmt_` est un jeton ; la
//! requête ignore alors le cookie de session, et ne retombe jamais sur lui ; tout autre
//! `Authorization` est ignoré, sauf sur `/mcp`, qui n'accepte qu'un jeton (`401` sans lui, le
//! cookie n'étant pas lu). Une porte posée sur chaque route
//! déclarée (`auth::token_gate`) juge, dans l'ordre : `403 bad_origin` pour une `Origin` d'un autre
//! site, lecture comprise ; `401 invalid_token` (avec `WWW-Authenticate: Bearer`) pour un jeton mal
//! formé, inconnu, révoqué, échu ou d'un compte parti, une seule réponse ; `403 token_not_allowed`
//! hors des routes ouvertes (`auth::TOKEN_ROUTES` : le compte et les membres sans adresse,
//! l'arbre, la recherche, la création d'une page et en lot, la lecture, le document et les
//! opérations d'un agent, le renommage et la place d'une page parmi ses sœurs, la corbeille et la
//! restauration, la duplication, le MMD en lecture, les rétroliens, la lecture d'un fichier ; toute
//! autre route est dans `auth::SESSION_ONLY_ROUTES`, `PUT …/mmd`, les jetons eux-mêmes, `/sync` et
//! `/events` compris),
//! rien de lu ni d'écrit ; `429 token_busy` (avec `Retry-After`) au-delà de 120 appels par minute
//! glissante pour le jeton, ou de 240 pour tous les jetons de son compte, ou de 16 et 32 Mio de
//! corps (`MEMENTUM_TOKEN_CALLS_PER_MIN`, `MEMENTUM_ACCOUNT_TOKEN_CALLS_PER_MIN`) ; puis le compte
//! attendu (`409 account_changed`) et l'ordre de toujours. Chaque transaction qu'une écriture par
//! jeton ouvre (hors du journal d'une page, que l'acteur relit lui-même) relit le jeton une fois son
//! tour d'écriture venu : une écriture admise pendant que sa révocation attend son tour répond
//! `401 invalid_token` sans rien écrire (Ruling R1516). Une page hors de la portée d'un jeton
//! reçoit la réponse d'une page inconnue. Un jeton ne partage pas, ne supprime pas pour de bon, ne
//! change pas une page de parent (ni ne la passe à la racine), ne met à la corbeille qu'avec son
//! option, et ne crée à la racine qu'au niveau « Modification » et membre de l'espace, dans les deux
//! sections. Les gardes par compte (une recherche à la fois, deux écritures par page, une
//! duplication à la fois) donnent à tous les jetons d'un compte une part à eux, distincte de celle
//! de sa session. Chaque appel admis laisse une ligne au journal d'activité du jeton, écrite par
//! paquets ; un refus du débit n'écrit qu'un compte.
//!
//! **Droits** (`access.rs`, le seul chemin par lequel les routes les calculent ; ce qui écrit
//! un droit durable est revérifié par le store dans la transaction qui l'écrit, Ruling R100) :
//! la colonne « Session » donne ce qu'il faut : un rôle dans l'espace pour les
//! routes d'un espace, un niveau sur la page pour les routes d'une page. Qui
//! n'a aucun rôle dans un espace reçoit `404 not_found`, la réponse d'un
//! espace qui n'existe pas, admin de l'instance compris ; en dessous du rôle
//! demandé, `403 forbidden`. De même pour une page : `404 not_found` pour qui
//! ne la voit pas, exactement la réponse d'une page qui n'existe pas ; `403
//! forbidden` sous le niveau demandé ; `409 trashed` pour qui écrit dans une
//! page à la corbeille. Le détail est en tête de `routes/pages.rs`, et celui
//! du partage (une page garde toujours quelqu'un en `full`) en tête de
//! `routes/sharing.rs`. Un changement de rôle et un retrait se décident sur
//! les rôles que la route a lus, que le store revérifie dans la transaction
//! qui écrit ; s'ils ont bougé, la route relit et redécide, puis répond `409
//! roles_changed` après deux reprises. Un changement d'accès, lui, est
//! vérifié en entier dans la transaction qui l'écrit (Ruling R87) : le cache
//! ne sert qu'à répondre vite avant le corps. De même, un passage à la racine
//! n'écrit ses accès que si la personne a encore, dans la transaction, son
//! rôle et l'accès complet à la page. C'est le principe (Ruling R100) : ce
//! qui écrit un droit durable est revérifié dans la transaction qui l'écrit.
//! Les autres écritures de page (création, corbeille, restauration,
//! suppression, changement de parent hors racine) se décident sur les droits
//! lus juste avant, sans aller-retour réseau : une fenêtre de quelques
//! millisecondes, acceptée.
//!
//! **Commentaires** (`routes/comments.rs`, Ruling R134). Des fils attachés à une page, et au
//! plus à un bloc par son identifiant ; rien n'est écrit dans le document. L'ordre des refus est
//! celui des pages, avec deux étapes de plus entre le niveau et la corbeille (Ruling R168, écrit
//! en tête de `access.rs`) : `404` pour la page, `403` sous le niveau de la route (`comment`, ou `full` pour supprimer tout
//! un fil), puis `404` pour un fil qui
//! n'est pas de cette page ou un message qui n'est pas de ce fil (la même réponse qu'il existe
//! ailleurs ou nulle part), `403` de la règle de l'auteur, `409 trashed`, enfin le corps ou l'emoji
//! (`422`), puis les plafonds (`409`, ci-dessous). Le store revérifie dans la transaction qui écrit que le fil est de la page, le
//! message du fil, et l'auteur quand c'est lui qui donne le droit. Chaque écriture validée qui
//! change quelque chose envoie la trame `{"type":"comments"}` aux connexions de synchro de la
//! page (Ruling R136) ; une écriture sans effet (réaction déjà posée, fil déjà résolu, message
//! modifié avec le même texte) n'écrit rien et n'envoie rien (Ruling R170). Aucune n'avance
//! `spaces.rev` ni `spaces.lsn`. Des plafonds, comptés par le store dans la transaction qui
//! écrirait : 2 000 fils par page, résolus compris ; 500 messages par fil ; 32 emojis différents
//! par message (Ruling R204) ; 10 000 messages et 4 Mio de texte par page, ce qui borne une
//! lecture de ses fils (Ruling R350) ; et la part d'un compte, 200 fils ouverts sur une page,
//! résolus compris, et 8 emojis posés sur un message (Ruling R351) ; 20 000 réactions par page (Ruling R357) ; et, pour chaque compte sur une page, 2 000 messages, 1 Mio de texte et 2 000 réactions (Ruling R358). Au-delà, `409
//! thread_limit`, `message_limit`, `reaction_limit` ou `page_comment_limit`, qui disent quoi
//! faire, et quand c'est la part du compte. Les plafonds communs passent avant la part.
//!
//! **Favoris** (`routes/favorites.rs`, Ruling R254). Une ligne `(compte, page)`, à son auteur
//! seul. Le store la garde quand la page devient invisible ou part à la corbeille ; la liste ne
//! rend que ce que `access.rs` montre au compte, hors corbeille : elle ne nomme jamais une page
//! qu'il ne voit plus, et rien n'y dit qu'elle existe encore. Poser ou retirer un favori
//! n'avance ni `spaces.rev` ni `spaces.lsn` : il ne change ni les droits ni la barre latérale
//! des autres, et le canal de l'espace n'en dit rien.
//!
//! **Dupliquer une page** (`routes/duplicate.rs`, Rulings R252, R590 à R599). La copie prend la
//! page et ses sous-pages que la personne voit, hors corbeille : des pages neuves, écrites avec
//! leurs documents en une transaction, comme `POST /api/v1/pages`, sans passer par un acteur ;
//! aucun identifiant de bloc n'est repris, et les fichiers que les blocs montrent sont attachés
//! de nouveau à la copie, sans jamais créer de ligne de contenu (Ruling R579). Elle va sous la
//! page parente si la personne peut y créer, sinon dans le « Privé » d'un membre. Les droits sont
//! relus juste avant d'écrire, une fois les documents bâtis, puis dans la transaction, qui
//! attend son tour dans la voie des copies de la file d'écriture, commune à tous les comptes :
//! une frappe n'attend jamais plus d'une copie, que la borne de poids (16 Mio) tient sous son
//! budget. Le débit est par compte, en mémoire (`duplications.rs`, Rulings R605 à R608) : une
//! duplication à la fois, et 64 Mio de documents bâtis par minute, écrits ou non ; au-delà,
//! `429 duplication_busy`, que personne ne rejoue. Un lien vers un fichier de la page est
//! récrit comme l'adresse d'un bloc (Ruling R609).
//!
//! **Fichiers** (`files.rs`, `routes/files.rs`, Rulings R244 à R247, R390 à R399). Un fichier
//! appartient à la page où il a été envoyé : son adresse la nomme, et chaque lecture est jugée sur
//! les droits du moment. Le contenu est gardé sous son SHA-256 dans `<data_dir>/files/` ; aucun
//! nom venu d'un client n'entre dans un chemin. Toute réponse qui porte un fichier est un
//! téléchargement, sous le type lu dans les octets : rien n'y est jamais un document ouvert
//! depuis l'origine de l'application. Au démarrage, le journal compte les contenus que la base ne
//! connaît pas, sans en supprimer aucun, et ceux qu'elle connaît et que le dépôt n'a plus. Pas de
//! quota avant le lot 7 : qui peut modifier une page peut remplir le disque (Ruling R247).
//! Un fichier vit autant que sa page (Ruling R245) : un bloc retiré, annulé ou revenu par une
//! version ne le touche pas. Un bloc copié ou glissé depuis une page de l'application, puis posé
//! dans une autre, y fait recopier l'attache par le client (`POST …/files/copies`, Rulings R249,
//! R713, R714), sans un octet renvoyé. **Seul le ménage d'une
//! page supprimée pour de bon retire un contenu du disque** (Rulings R555 à R557) : il lit sans
//! verrou les contenus qu'aucune attache ne nomme, puis, par paquets sous le verrou du dépôt (que
//! prennent aussi les envois, entre la pose et l'attache), la base retire ceux qu'aucune attache ne
//! nomme encore, et le disque ceux-là ; un contenu que la base ne connaît pas n'est jamais regardé.
//!
//! **Index de recherche** (`indexer.rs`, lot 4, Rulings R906, R907). Une tâche de fond tient à
//! jour, dans la base, la projection de chaque page (`blocks_index`, `page_links`) : elle relit
//! ce que le store garde, jamais l'acteur d'une page, deux secondes après la dernière écriture
//! (quinze au plus), et tout de suite pour une page créée ou dupliquée. Au démarrage, elle
//! rattrape ce qui n'est pas indexé, sans que le serveur l'attende. Elle n'écrit que la
//! différence, dans sa voie de la file d'écriture. Elle seule tient l'index plein texte
//! (`<données>/search/`), nourri de cette projection : la route de recherche le lit par elle
//! (`IndexerHandle::search`), en lui donnant, à chaque recherche, l'ensemble des pages que le
//! compte voit (`routes/search.rs`) ; l'index ne porte ni espace, ni droit, ni corbeille.
//!
//! **Historique** (`routes/versions.rs`, Ruling R135). L'acteur de chaque page coupe ses
//! versions (`mementum-sync`) : l'état complet de la page, avec les comptes qui l'ont modifiée
//! depuis la version d'avant, avant l'écriture qui suit de `MEMENTUM_VERSION_EVERY_SECS` (dix
//! minutes par défaut) la première écriture non versionnée, à l'arrêt de l'acteur s'il en reste
//! (page vide, arrêt du serveur), et avant une restauration qui remplace ce qu'aucune version ne
//! garde (Ruling R190) ; une page garde ses cent dernières, dont vingt `before_restore` au plus
//! (Ruling R189). Chaque version coupée envoie la trame `{"type":"versions"}` aux seules connexions
//! de synchro de la page qui peuvent écrire (Ruling R355) : un lecteur n'apprend pas qu'une
//! version vient d'être coupée. L'ordre des refus est celui des commentaires (Rulings R168, R186) : la
//! page, le niveau, puis la version (une version qui n'est pas de la page reçoit la réponse
//! d'une page introuvable), enfin la corbeille. Restaurer remplace le contenu comme `PUT /mmd` :
//! une édition faite en même temps sur le contenu remplacé est perdue (Ruling R188).
//! L'historique se lit à partir de « Peut modifier » (Ruling R203) : un lecteur ne lit pas ce
//! que la page portait avant qu'on la lui partage.
//!
//! **Comptes** (`auth.rs`, `routes/auth.rs`) : comptes locaux, mot de passe
//! haché en Argon2id, session par cookie. Le premier compte créé administre
//! l'instance ; ensuite, on s'inscrit librement (`MEMENTUM_SIGNUP=open`) ou
//! avec le jeton d'une invitation (`invite`, le défaut). Se connecter ou
//! s'inscrire ferme la session que portait déjà le navigateur. Un mot de passe
//! oublié se règle par `mementum user set-password`, faute de SMTP. C'est le
//! compte qui protège les pages : plus aucune route de page ne répond sans
//! session, ni ne lit ou n'écrit une page sans en avoir vérifié le droit.
//!
//! **Synchronisation** (`routes/sync.rs`). `/sync` est sous les mêmes droits
//! que les routes de pages : session, puis droit de lire la page, vérifiés
//! avant la poignée de main, et relus juste après, une fois la page chargée.
//! Une connexion garde ensuite les droits qu'elle avait en s'ouvrant ; le
//! serveur la ferme quand l'une de ces écritures les change (corbeille,
//! restauration, changement de parent, partage, retrait d'un membre,
//! changement de rôle, suppression ; une invitation acceptée, qui ne fait
//! qu'en ajouter, ne ferme rien) ou qu'une session de son compte se ferme
//! (déconnexion, changement de mot de passe, connexion ou inscription
//! par-dessus une session), et le client revient par la poignée de main, qui
//! relit tout. Une connexion en train de s'ouvrir à cet instant est fermée
//! avant d'avoir rien reçu.
//!
//! **Canal de l'espace** (`events.rs`, `routes/events.rs`, Ruling R133). Une
//! WebSocket par espace, qui ne porte que sa version (`spaces.lsn`) : le
//! client relit l'arbre par `GET /api/v1/spaces/{id}/tree`, déjà filtré par
//! `access.rs`. Aucun second calcul de « qui voit quoi » n'est écrit pour un
//! flux poussé. Mêmes contrôles que `/sync` avant la poignée de main ; après,
//! la session et le rôle sont revérifiés avant de mettre en file chaque trame
//! et chaque ping, et une connexion qui ne répond plus aux pings est fermée
//! (Ruling R153). Chaque ping redit la version (Ruling R205).
//!
//! **Garde-fous réseau** (`guard.rs`). Une couche passe avant toute route.
//! Sur la boucle locale, l'en-tête `Host` doit la nommer (DNS rebinding,
//! Ruling R31), ou nommer l'hôte de `MEMENTUM_PUBLIC_URL` quand elle est
//! réglée (un mandataire ou un tunnel qui garde le `Host` public, Ruling
//! R78), sinon `421 bad_host`. Partout, cette couche répond `403 bad_origin`
//! à une requête qui écrit (tout sauf `GET`, `HEAD` et `OPTIONS`) et qui
//! porte l'`Origin` d'un autre site. Une connexion `/sync` ou `/events` est un
//! `GET` : la couche ne regarde pas son `Origin`. C'est la route qui la
//! contrôle, avant tout le reste, par la même règle (`routes/sync.rs`). Une
//! seconde couche, par-dessus, pose `Cache-Control: no-store` et
//! `X-Content-Type-Options: nosniff` sur toute réponse de l'API (`/api/…`) et du
//! serveur MCP (`/mcp`, `/mcp/readonly`), refus compris ; ni `/sync` ni `/events`
//! n'en sont. Aucune réponse ne porte d'en-tête CORS. Seul un fichier servi,
//! marqué `guard::FileResponse`, garde son `Cache-Control: private, no-cache`.

mod access;
pub mod agent;
pub mod auth;
pub mod config;
mod duplications;
mod error;
mod events;
mod files;
mod guard;
mod indexer;
mod inputs_cache;
mod mmd_reads;
mod page_writes;
mod searches;
pub mod types;

mod routes {
    pub(crate) mod agent;
    pub(crate) mod auth;
    pub(crate) mod backlinks;
    pub(crate) mod comments;
    pub(crate) mod duplicate;
    pub(crate) mod events;
    pub(crate) mod favorites;
    pub(crate) mod files;
    pub(crate) mod info;
    pub(crate) mod invitations;
    pub(crate) mod mcp;
    pub(crate) mod pages;
    pub(crate) mod search;
    pub(crate) mod sharing;
    pub(crate) mod spaces;
    pub(crate) mod sync;
    pub(crate) mod tokens;
    pub(crate) mod versions;
}

use std::future::Future;
use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::StatusCode;
use axum::middleware;
use axum::routing::{delete, get, patch, post, put};
use mementum_acl::AccessCache;
use mementum_store::Store;
use mementum_sync::{Hub, SyncConfig};
use tokio::net::TcpListener;

pub use auth::{PasswordSet, set_password};
pub use config::Config;
pub use duplications::{DUPLICATION_BUDGET, DUPLICATION_WINDOW};
pub use error::ApiError;
pub use events::EVENTS_PING;
pub use indexer::{INDEX_AFTER, INDEX_AT_MOST, IndexerHandle};
// Ce que `mementum reindex` fait comme l'indexeur, plutôt qu'à sa façon (Task S7).
pub use indexer::{Reindexed, SEARCH_DIR, index_stored, index_unreadable, write_lines};
// La copie d'avant migration, que `set_password` signale à qui l'appelle.
pub use mementum_store::Backup;
pub use page_writes::{MAX_ACCOUNT_WRITES, MAX_PAGE_WRITES};
pub use routes::duplicate::{MAX_DUPLICATED_BYTES, MAX_DUPLICATED_PAGES};
pub use routes::pages::{MAX_MMD_BLOCKS, MAX_TITLE_CHARS, MMD_WRITE_BUDGET, UNTITLED};
pub use routes::search::{MAX_SEARCH_BODY_BYTES, MAX_SEARCH_PAGES, SEARCH_FLOOR, SEARCH_PAGE_SIZE};
pub use searches::{MAX_SEARCHES, Searches};
pub use types::{Info, Me, PageNode, SignupMode, Tree};

use auth::LoginThrottle;
use routes::{comments, info, invitations, pages, sharing, spaces, sync, versions};
use types::ErrorCode;

/// Taille maximale d'un corps de requête (celle d'axum par défaut, posée
/// explicitement) : 2 Mio. `parse` est déjà bornée en profondeur.
pub const MAX_BODY_BYTES: usize = 2 << 20;

/// Début de la ligne écrite sur la sortie standard une fois le serveur à
/// l'écoute, suivi de l'adresse réellement liée (Ruling R24).
pub const LISTENING: &str = "mementum écoute sur http://";

/// État partagé des routes.
#[derive(Clone)]
pub(crate) struct AppState {
    store: Store,
    hub: Arc<Hub>,
    /// L'en-tête `Host` doit nommer la boucle locale : vrai quand le serveur
    /// n'écoute que sur elle (Ruling R31).
    check_host: bool,
    /// Qui peut créer un compte.
    signup: SignupMode,
    /// Le cookie de session porte `Secure` : l'adresse publique est en
    /// `https`.
    secure_cookies: bool,
    /// L'origine de l'adresse publique (`https://notes.exemple.fr`), admise
    /// sur les écritures et sur `/sync` (Ruling R51).
    public_origin: Option<Arc<str>>,
    /// Le frein des connexions, en mémoire.
    logins: Arc<LoginThrottle>,
    /// Une inscription à la fois (voir `routes::auth::signup`).
    signups: Arc<tokio::sync::Mutex<()>>,
    /// Les accès déjà calculés, par personne et par espace (`access.rs`).
    access: Arc<AccessCache>,
    /// Les données des espaces dont ces accès se calculent, lues une fois par
    /// révision pour tout le monde (`inputs_cache.rs`).
    inputs: Arc<inputs_cache::InputsCache>,
    /// Intervalle des pings du canal de l'espace (`events.rs`).
    events_ping: Duration,
    /// Une écriture du serveur à la fois par page : `PUT /mmd` et la restauration d'une
    /// version (`page_writes.rs`, Ruling R200).
    page_writes: Arc<page_writes::PageWrites>,
    /// Le dépôt des fichiers envoyés dans les pages (`files.rs`, Ruling R244).
    files: Arc<files::FileStore>,
    /// Octets au plus par fichier envoyé (`MEMENTUM_MAX_FILE_MB`, Ruling R247).
    max_file_bytes: u64,
    /// Un envoi qui ne reçoit plus rien pendant ce délai est abandonné (Ruling R247).
    upload_idle: Duration,
    /// La lecture d'un MMD reçu et l'estimation de son écriture, hors des fils du serveur,
    /// quatre de front (`mmd_reads.rs`, Ruling R383).
    mmd_reads: Arc<mmd_reads::MmdReads>,
    /// Octets au plus, tous documents confondus, d'une copie de pages (`routes/duplicate.rs`,
    /// Ruling R592).
    max_duplicated_bytes: usize,
    /// Le débit des duplications, par compte : une à la fois, et un budget d'octets bâtis par
    /// minute (`duplications.rs`, Rulings R605, R606).
    duplications: Arc<duplications::Duplications>,
    /// L'indexeur de la recherche (`indexer.rs`, lot 4) : l'index plein texte, que lui seul
    /// tient, et ce qu'il sait en retard.
    indexer: IndexerHandle,
    /// Le débit des recherches, par compte : une admise et une en attente, quatre calculs de
    /// front pour l'instance (`searches.rs`, Ruling R910).
    searches: Searches,
    /// Le débit des appels par jeton d'API et leur journal d'activité (`agent/calls.rs`, Rulings
    /// R1241, R1244).
    calls: Arc<agent::calls::Calls>,
    /// Les révisions servies aux agents, et les empreintes de leurs blocs (`agent/revisions.rs`,
    /// lot 5, Ruling R1226).
    revisions: Arc<agent::revisions::Revisions>,
}

/// Millisecondes Unix.
pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Mène `work` au bout dans sa propre tâche, même si la requête est
/// abandonnée en route (client parti) : pour une suite d'écritures qui ne doit
/// pas s'arrêter au milieu, ou une écriture de droits suivie de la fermeture
/// des connexions qui les avaient lus. Ses écritures restent dans la voie du
/// compte de la requête, dans la file d'écriture du store (Ruling R512), au nom
/// de son jeton d'API s'il en a un (Ruling R1516 : chaque transaction le relit).
pub(crate) async fn to_completion<T: Send + 'static>(
    work: impl Future<Output = Result<T, ApiError>> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::spawn(mementum_store::writes_as_caller(work))
        .await
        .map_err(|e| ApiError::internal(&e))?
}

/// Ce qu'il faut signaler, en avertissement, d'une copie d'avant migration
/// posée sans lien physique (`Backup::without_hard_link`) ; `None` si elle a
/// pris son nom de façon atomique, ou si une copie identique a été gardée.
/// Le serveur le journalise ; `mementum user set-password` l'écrit sur la
/// sortie d'erreur.
pub fn backup_warning(backup: &Backup) -> Option<&'static str> {
    backup.without_hard_link.then_some(
        "la copie d'avant migration a pris son nom par un renommage, parce que ce système de \
         fichiers ne permet pas les liens physiques : rien n'a été écrasé, mais deux processus \
         qui ouvriraient la même base au même instant pourraient viser le même nom ; ne lancer \
         qu'un processus à la fois sur ces données pendant une mise à jour",
    )
}

/// L'application : ouvre (et crée au besoin) le dossier des données et la
/// base, puis monte les routes.
pub async fn app(cfg: &Config) -> anyhow::Result<Router> {
    Ok(start(cfg).await?.router)
}

/// L'application démarrée : ses routes, et ce qui tourne derrière elles.
pub struct Started {
    /// Les routes. Elles tiennent une poignée de l'indexeur : il tourne tant qu'elles vivent.
    pub router: Router,
    /// L'indexeur de la recherche, lancé en fond.
    pub indexer: IndexerHandle,
    /// Le débit des recherches : ses compteurs. Pour les tests.
    #[doc(hidden)]
    pub searches: Searches,
    /// L'écriture du journal d'activité des jetons, en fond : arrêtée avant la base.
    pub calls: agent::calls::CallsHandle,
    store: Store,
    hub: Arc<Hub>,
}

impl Started {
    /// La base de l'application. Pour les tests.
    #[doc(hidden)]
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Le hub de l'application. Pour les tests.
    #[doc(hidden)]
    pub fn hub(&self) -> &Arc<Hub> {
        &self.hub
    }

    /// L'arrêt, dans l'ordre : chaque page ouverte coupe sa version `closing` s'il lui reste des
    /// écritures non versionnées (Ruling R135) et ferme ses connexions ; l'indexeur finit
    /// l'indexation en cours ; puis la base ferme. Ce qui restait à indexer l'est au démarrage
    /// suivant.
    pub async fn shutdown(self) {
        self.hub.shutdown().await;
        self.indexer.stop().await;
        self.calls.stop().await;
        self.store.close().await;
    }
}

/// Ouvre (et crée au besoin) le dossier des données et la base, monte les routes et lance
/// l'indexeur : ce que [`app`] et [`serve`] font tous deux.
pub async fn start(cfg: &Config) -> anyhow::Result<Started> {
    build(cfg).await
}

/// Lance le serveur sur `cfg.bind`, écrit sur la sortie standard la ligne
/// `mementum écoute sur http://<adresse liée>`, puis sert jusqu'à Ctrl-C
/// (ou `SIGTERM`). À l'arrêt, les pages ouvertes gardent leur dernière version
/// avant que la base ferme ([`Hub::shutdown`]).
pub async fn serve(cfg: Config) -> anyhow::Result<()> {
    let started = start(&cfg).await?;
    let (listener, addr) = listen(cfg.bind).await?;
    // Une sortie standard fermée n'arrête pas le serveur : l'adresse est
    // aussi dans le journal « serveur prêt ».
    let announced = {
        let mut stdout = std::io::stdout().lock();
        writeln!(stdout, "{LISTENING}{addr}").and_then(|()| stdout.flush())
    };
    if let Err(error) = announced {
        tracing::warn!(%error, "ligne d'écoute impossible à écrire sur la sortie standard ; le serveur continue");
    }
    tracing::info!(%addr, data_dir = %cfg.data_dir.display(), "serveur prêt");
    let served = axum::serve(listener, started.router.clone())
        .with_graceful_shutdown(shutdown_signal())
        .await;
    // Plus aucune requête : les pages ouvertes, l'indexeur, puis la base ([`Started::shutdown`]).
    // Aussi quand le serveur HTTP s'est arrêté sur une erreur, rendue ensuite.
    started.shutdown().await;
    served.context("serveur HTTP")
}

/// L'écoute de `mementum serve` : liée à `bind`, ses connexions sans l'algorithme de Nagle
/// ([`without_nagle`]). Rend aussi l'adresse réellement liée.
async fn listen(
    bind: std::net::SocketAddr,
) -> anyhow::Result<(
    impl axum::serve::Listener<Io = tokio::net::TcpStream, Addr = std::net::SocketAddr>,
    std::net::SocketAddr,
)> {
    let listener = TcpListener::bind(bind)
        .await
        .with_context(|| format!("impossible d'écouter sur {bind}"))?;
    let addr = listener.local_addr()?;
    Ok((without_nagle(listener), addr))
}

/// Les connexions que `listener` accepte écrivent sans l'algorithme de Nagle (`TCP_NODELAY`,
/// Ruling R445) : une petite trame (une frappe relayée, une marque de présence, une trame de page)
/// part tout de suite, au lieu d'attendre l'accusé de réception de la précédente, qu'un client
/// qui n'a rien à envoyer retarde (jusqu'à 40 ms sous Linux). Une option refusée par le système
/// laisse la connexion telle quelle.
pub fn without_nagle(
    listener: TcpListener,
) -> impl axum::serve::Listener<Io = tokio::net::TcpStream, Addr = std::net::SocketAddr> {
    use axum::serve::ListenerExt;
    listener.tap_io(|tcp| {
        if let Err(error) = tcp.set_nodelay(true) {
            tracing::debug!(%error, "TCP_NODELAY refusé sur une connexion acceptée");
        }
    })
}

async fn build(cfg: &Config) -> anyhow::Result<Started> {
    // Avant de toucher au disque : une adresse publique invalide arrête tout.
    let public_origin = cfg
        .public_url
        .as_deref()
        .map(config::public_origin)
        .transpose()?;
    std::fs::create_dir_all(&cfg.data_dir).with_context(|| {
        format!(
            "impossible de créer le dossier des données {}",
            cfg.data_dir.display()
        )
    })?;
    let db = cfg.db_path();
    let store = Store::open_with(&db, cfg.store)
        .await
        .with_context(|| format!("impossible d'ouvrir la base {}", db.display()))?;
    // Le store n'a pas de journal : c'est ici qu'on dit qu'une migration a
    // tourné, et où est la base d'avant (Rulings R67 et R72).
    if let Some(backup) = store.backup() {
        // La parenthèse ne se lit que si elle dit vrai (défaut D8 de la passe de fin).
        if backup.kept {
            tracing::info!(
                path = %backup.path.display(),
                bytes = backup.bytes,
                kept = backup.kept,
                "base migrée ; la copie d'avant migration est à côté d'elle (une copie \
                 identique, d'une tentative précédente, y était déjà : elle est gardée)"
            );
        } else {
            tracing::info!(
                path = %backup.path.display(),
                bytes = backup.bytes,
                kept = backup.kept,
                "base migrée ; la copie d'avant migration est à côté d'elle"
            );
        }
        if let Some(warning) = backup_warning(backup) {
            tracing::warn!(path = %backup.path.display(), "{warning}");
        }
    }
    let files = open_files(cfg, &store).await?;
    let expired = store
        .purge_sessions(now_ms())
        .await
        .context("purge des sessions échues")?;
    if expired > 0 {
        tracing::info!(sessions = expired, "sessions échues supprimées");
    }
    // Le réglage par défaut de la synchro, sauf l'intervalle des versions : l'e2e des budgets du
    // navigateur suit son `idle_close` (`e2e_wait_follows_idle_close`, dans `mementum-sync`).
    let sync = SyncConfig::default().with_version_every(cfg.version_every);
    let hub = Hub::new(store.clone(), sync);
    let check_host = cfg.bind.ip().is_loopback();
    if !check_host {
        tracing::warn!(
            bind = %cfg.bind,
            "écoute hors de la boucle locale : le contrôle de l'en-tête Host est désactivé, et \
             ce sont les comptes qui protègent les pages : aucune route de page ne répond sans \
             session ni sans le droit de la personne sur la page, synchronisation (/sync) \
             comprise"
        );
    }
    if store.count_users().await.context("lecture des comptes")? == 0 {
        tracing::warn!(
            "aucun compte n'existe : le premier compte créé sera admin de l'instance, quel que \
             soit le réglage des inscriptions ; le créer avant d'exposer le serveur"
        );
    }
    // L'index plein texte (`<données>/search/`) est ouvert ici : un dossier qu'un autre processus
    // tient, ou qui porte autre chose qu'un index, arrête le démarrage. Ses jetons sont comparés à
    // ceux de la base par l'indexeur, en fond (Ruling R1000).
    let text = indexer::open_text(&cfg.data_dir).await?;
    // L'indexeur de la recherche, en fond : ni son rattrapage ni une reconstruction de l'index
    // ne sont attendus (le démarrage à froid ne les paie pas).
    let indexer = indexer::spawn(
        store.clone(),
        hub.clone(),
        indexer::IndexerConfig {
            after: cfg.index_after,
            at_most: cfg.index_at_most,
            origin: public_origin.as_deref().map(Arc::from),
        },
        text,
    );
    let state = AppState {
        store: store.clone(),
        hub: hub.clone(),
        check_host,
        signup: cfg.signup,
        secure_cookies: public_origin
            .as_deref()
            .is_some_and(|origin| origin.starts_with("https://")),
        public_origin: public_origin.map(Arc::from),
        logins: Arc::new(LoginThrottle::default()),
        signups: Arc::new(tokio::sync::Mutex::new(())),
        access: Arc::new(AccessCache::new(access::ACCESS_CACHE_NODES)),
        inputs: Arc::new(inputs_cache::InputsCache::new(
            inputs_cache::INPUTS_CACHE_UNITS,
        )),
        events_ping: cfg.events_ping,
        page_writes: Arc::default(),
        files: Arc::new(files),
        max_file_bytes: cfg.max_file_bytes,
        upload_idle: cfg.upload_idle,
        mmd_reads: Arc::default(),
        max_duplicated_bytes: cfg.max_duplicated_bytes,
        duplications: Arc::default(),
        indexer: indexer.clone(),
        searches: Searches::default(),
        calls: Arc::new(agent::calls::Calls::new(agent::calls::Limits {
            token_calls: cfg.token_calls_per_min,
            account_calls: cfg.account_token_calls_per_min,
            ..agent::calls::Limits::default()
        })),
        revisions: Arc::new(agent::revisions::Revisions::new(cfg.kept_revision_bytes)),
    };
    let calls = state.calls.spawn_writer(store.clone());
    Ok(Started {
        calls,
        searches: state.searches.clone(),
        router: router(state),
        indexer,
        store,
        hub,
    })
}

/// Ouvre le dépôt des fichiers du dossier des données, ce que seul le démarrage du serveur fait :
/// il vide `files/tmp`, où un serveur qui tourne écrit ses envois en cours. Puis le journal dit,
/// en avertissement, les contenus du dépôt que la base ne connaît pas, **sans en supprimer
/// aucun** (Ruling R245 : une base revenue en arrière les nomme peut-être encore), et les
/// contenus que la base connaît et que le dépôt n'a plus. Un dépôt qui ne s'ouvre pas arrête le
/// démarrage ; un compte qui ne se fait pas, non (R279).
async fn open_files(cfg: &Config, store: &Store) -> anyhow::Result<files::FileStore> {
    let files = files::FileStore::open(&cfg.data_dir).with_context(|| {
        format!(
            "impossible d'ouvrir le dépôt des fichiers {}",
            cfg.data_dir.join(files::FILES_DIR).display()
        )
    })?;
    let known = store
        .known_files()
        .await
        .context("lecture des fichiers que la base connaît")?;
    match files.strangers(&known) {
        Ok((0, _)) => {}
        Ok((count, bytes)) => tracing::warn!(
            count,
            bytes,
            "contenus du dépôt des fichiers que la base ne connaît pas : ils restent sur le \
             disque, aucun n'est supprimé au démarrage (un envoi coupé entre la pose et \
             l'attache, ou une base revenue en arrière qui les nommait) ; renvoyé, un tel contenu \
             est repris tel quel"
        ),
        Err(error) => tracing::warn!(
            %error,
            "dépôt des fichiers illisible : les contenus que la base ne connaît pas n'ont pas été \
             comptés"
        ),
    }
    let missing = files.missing(&known);
    if missing > 0 {
        tracing::warn!(
            missing,
            "fichiers que la base connaît et que le dépôt n'a plus : ils sont dits indisponibles \
             à qui les ouvre (base restaurée sans son dossier files/, ou coupure de courant juste \
             après un envoi)"
        );
    }
    Ok(files)
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(info::health))
        .route("/api/v1/info", get(info::info))
        .route("/api/v1/auth/signup", post(routes::auth::signup))
        .route("/api/v1/auth/login", post(routes::auth::login))
        .route("/api/v1/auth/logout", post(routes::auth::logout))
        .route("/api/v1/auth/password", post(routes::auth::change_password))
        .route("/api/v1/me", get(routes::auth::me))
        .route(
            "/api/v1/tokens",
            get(routes::tokens::list).post(routes::tokens::create),
        )
        .route("/api/v1/tokens/{id}", delete(routes::tokens::revoke))
        .route("/api/v1/tokens/{id}/calls", get(routes::tokens::calls))
        .route("/api/v1/spaces", post(spaces::create))
        .route("/api/v1/spaces/{id}", patch(spaces::update))
        .route("/api/v1/spaces/{id}/members", get(spaces::members))
        .route(
            "/api/v1/spaces/{id}/members/{user_id}",
            patch(spaces::update_member).delete(spaces::remove_member),
        )
        .route(
            "/api/v1/spaces/{id}/invitations",
            get(invitations::space_invitations).post(invitations::invite_to_space),
        )
        .route("/api/v1/invitations/{id}", delete(invitations::revoke))
        .route("/api/v1/invitations/lookup", post(invitations::lookup))
        .route("/api/v1/invitations/accept", post(invitations::accept))
        .route("/api/v1/spaces/{id}/tree", get(pages::tree))
        .route("/api/v1/spaces/{id}/trash", get(pages::trash_list))
        .route(
            "/api/v1/spaces/{id}/favorites",
            get(routes::favorites::list),
        )
        .route(
            "/api/v1/spaces/{id}/search",
            // Une recherche tient en quelques centaines d'octets : sa borne est la sienne.
            post(routes::search::search).layer(DefaultBodyLimit::max(MAX_SEARCH_BODY_BYTES)),
        )
        .route("/api/v1/pages", post(pages::create))
        .route(
            "/api/v1/spaces/{id}/pages/batch",
            post(routes::agent::pages_batch),
        )
        .route("/api/v1/pages/{id}/document", get(routes::agent::document))
        .route(
            "/api/v1/pages/{id}/operations",
            post(routes::agent::operations),
        )
        .route(
            "/api/v1/pages/{id}",
            get(pages::get).patch(pages::update).delete(pages::delete),
        )
        .route("/api/v1/pages/{id}/trash", post(pages::trash))
        .route("/api/v1/pages/{id}/restore", post(pages::restore))
        .route(
            "/api/v1/pages/{id}/duplicate",
            post(routes::duplicate::duplicate),
        )
        .route(
            "/api/v1/pages/{id}/favorite",
            put(routes::favorites::add).delete(routes::favorites::remove),
        )
        .route("/api/v1/pages/{id}/backlinks", get(routes::backlinks::list))
        .route(
            "/api/v1/pages/{id}/mmd",
            get(pages::get_mmd).put(pages::put_mmd),
        )
        .route("/api/v1/pages/{id}/access", get(sharing::access))
        .route(
            "/api/v1/pages/{id}/access/general",
            put(sharing::set_general),
        )
        .route(
            "/api/v1/pages/{id}/access/users/{user_id}",
            put(sharing::set_user).delete(sharing::remove_user),
        )
        .route("/api/v1/pages/{id}/invitations", post(sharing::invite))
        .route(
            "/api/v1/pages/{id}/comments",
            get(comments::list).post(comments::open),
        )
        .route(
            "/api/v1/pages/{id}/comments/{thread_id}",
            patch(comments::resolve).delete(comments::delete_thread),
        )
        .route(
            "/api/v1/pages/{id}/comments/{thread_id}/replies",
            post(comments::reply),
        )
        .route(
            "/api/v1/pages/{id}/comments/{thread_id}/messages/{comment_id}",
            patch(comments::edit).delete(comments::delete),
        )
        .route(
            "/api/v1/pages/{id}/comments/{thread_id}/messages/{comment_id}/reactions/{emoji}",
            put(comments::react).delete(comments::unreact),
        )
        .route("/api/v1/files/limits", get(routes::files::limits))
        .route(
            "/api/v1/pages/{id}/files",
            // Le corps est le fichier : la borne de 2 Mio des autres corps n'y vaut pas (la route
            // lit le corps elle-même, sous la sienne, `MEMENTUM_MAX_FILE_MB`).
            post(routes::files::upload).layer(DefaultBodyLimit::disable()),
        )
        // Avant `{file_id}` (un segment fixe passe avant un paramètre) : un `GET` y répond `405`,
        // pour toute page (Ruling R559).
        .route("/api/v1/pages/{id}/files/copies", post(routes::files::copy))
        .route(
            "/api/v1/pages/{id}/files/{file_id}",
            get(routes::files::download),
        )
        .route("/api/v1/pages/{id}/versions", get(versions::list))
        .route(
            "/api/v1/pages/{id}/versions/{version_id}/state",
            get(versions::state),
        )
        .route(
            "/api/v1/pages/{id}/versions/{version_id}/restore",
            post(versions::restore),
        )
        // Le serveur MCP (lot 5, Task M2) : un jeton seulement, jamais une session.
        .route("/mcp", post(routes::mcp::full))
        .route("/mcp/readonly", post(routes::mcp::readonly))
        .route("/sync/{id}", get(sync::sync))
        .route("/events/{id}", get(routes::events::events))
        // Sur chaque route déclarée, pas sur les fallbacks : la porte des jetons d'API lit le
        // chemin déclaré (lot 5, Rulings R1214, R1222).
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::token_gate,
        ))
        .fallback(async || {
            ApiError::new(StatusCode::NOT_FOUND, ErrorCode::NotFound, "route inconnue")
        })
        .method_not_allowed_fallback(async || {
            ApiError::new(
                StatusCode::METHOD_NOT_ALLOWED,
                ErrorCode::MethodNotAllowed,
                "méthode non permise sur cette route",
            )
        })
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        // Les écritures de chaque requête, dans la voie de son compte (Ruling R512).
        .layer(middleware::from_fn(auth::writes_of_the_request))
        // Avant toute route, fallbacks compris (Rulings R31 et R51).
        .layer(middleware::from_fn_with_state(state.clone(), guard::check))
        // Par-dessus tout, refus de la couche précédente compris.
        .layer(middleware::from_fn(guard::api_headers))
        .with_state(state)
}

/// Ctrl-C, ou `SIGTERM` (arrêt d'un conteneur) sous Unix. Un signal qui ne
/// peut pas être surveillé n'arrête jamais le serveur.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::warn!(%error, "Ctrl-C non surveillé");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::warn!(%error, "SIGTERM non surveillé");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
    tracing::info!("arrêt demandé");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une copie posée sans lien physique se signale ; une copie posée de
    /// façon atomique, ou gardée, non. (Le cas ne se provoque pas sur le
    /// disque d'un test : il demande un système de fichiers sans liens
    /// physiques.)
    #[test]
    fn backup_without_hard_link_is_a_warning() {
        let backup = |kept, without_hard_link| Backup {
            path: "mementum.db.avant-0002.bak".into(),
            bytes: 4096,
            kept,
            without_hard_link,
        };
        assert_eq!(backup_warning(&backup(false, false)), None);
        assert_eq!(backup_warning(&backup(true, false)), None);
        let warning = backup_warning(&backup(false, true)).expect("un avertissement");
        for part in ["renommage", "liens physiques", "rien n'a été écrasé"] {
            assert!(warning.contains(part), "« {part} » absent de : {warning}");
        }
    }

    /// Une connexion acceptée par l'écoute de `serve` écrit sans l'algorithme de Nagle (Ruling
    /// R445).
    #[tokio::test]
    async fn accepted_connections_have_no_nagle() {
        use axum::serve::Listener;
        let (mut listener, addr) = listen("127.0.0.1:0".parse().unwrap()).await.unwrap();
        let client = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (accepted, _) = listener.accept().await;
        assert!(accepted.nodelay().unwrap());
        drop(client);
    }

    /// Ce que `to_completion` mène dans sa propre tâche garde le compte auquel la requête
    /// attribue ses écritures (Ruling R512) : une tâche lancée n'hérite pas de la portée.
    #[tokio::test]
    async fn work_carried_to_completion_keeps_the_writer_of_the_request() {
        let tom = uuid::Uuid::now_v7();
        let seen = mementum_store::writes_by(Some(tom), async {
            to_completion(async { Ok(mementum_store::writer()) }).await
        })
        .await
        .unwrap();
        assert_eq!(seen, Some(tom));
    }
}
