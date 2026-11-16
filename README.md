# Mementum

> **Écrivez ensemble, en direct, chez vous.**

Mementum est un espace de travail **open source et auto-hébergeable** de la famille de Notion :
- pages en blocs ;
- bases de données relationnelles (relations, rollups, formules, vues) ;
- équipes et permissions ;
- édition à plusieurs en direct ;
- contenu rangé dans des **espaces** à la manière de Notion : chacun crée les siens (« Perso », « Pro »…) et y invite qui il veut, dans tout l'espace ou sur une page.

Il est **conçu dès le départ pour que les agents IA lisent et écrivent les pages proprement** : format Markdown canonique (MMD), serveur MCP intégré, agents membres de l'espace avec des droits, un journal et un mode suggestion.

*Mementum* = *memento* (« souviens-toi ») + *momentum* (l'élan) : la mémoire de l'équipe et de ses agents, toujours en mouvement.

---

## Où en est le projet (16 novembre 2026)

| Étape | État |
|---|---|
| Recherche (Notion 2026, alternatives open source, architecture temps réel, accès des agents IA) | faite : [`docs/research/`](docs/research/) |
| Conception complète | validée : [`docs/specs/2026-11-09-mementum-design.md`](docs/specs/2026-11-09-mementum-design.md) |
| Maquettes (3 directions, **A « Papier » retenue**) | faites : [`docs/design/`](docs/design/) |
| Phase 0 : prototype jetable qui valide les choix techniques | faite sur PC et relue (relecture finale du 9 novembre, voir [`spike/RESULTS.md`](spike/RESULTS.md)) ; **mesures ARM reportées avant la phase 2** (bases de données) |
| Phase 1 « Écrire ensemble » (MVP) | en cours, découpée en 7 lots. Lot 1 « Socle et édition en direct » **fait** ([plan](docs/superpowers/plans/2026-11-09-p1-lot1-socle-edition.md), [registre](docs/superpowers/logs/2026-11-09-p1-lot1-ledger.md)). Lot 2 « Comptes, espaces, droits et corbeille » **livré** ([plan](docs/superpowers/plans/2026-11-10-p1-lot2-comptes-espaces-droits.md), [registre](docs/superpowers/logs/2026-11-10-p1-lot2-ledger.md), [checklist de recette](docs/recette/2026-11-10-p1-lot2-recette.md)). Lot 3 « Collaboration et blocs riches » **ouvert le 12 novembre**, en deux plans (Ruling R132). Partie A, « Collaboration » ([plan](docs/superpowers/plans/2026-11-12-p1-lot3-collaboration-blocs-riches.md), [registre](docs/superpowers/logs/2026-11-12-p1-lot3-ledger.md), [checklist de recette](docs/recette/2026-11-12-p1-lot3a-recette.md)) : **code fini**, relu et corrigé ; sa clôture attend les dernières vérifications (voir « Prochaines étapes »). Partie B, « Blocs riches et organisation » ([plan](docs/superpowers/plans/2026-11-12-p1-lot3b-blocs-riches-organisation.md)) : en cours d'exécution, décrite ici à sa clôture. Lots 4 à 7 à venir |

Le lot 1 a livré le vrai code (`crates/mementum-*`, `apps/web`, `packages/protocol`, `spec/`) : un arbre de pages, l'édition à plusieurs en direct dans BlockNote et le format MMD de la phase 1. Le lot 2 a corrigé une de ses limites connues : l'arbre des pages ne ralentit plus l'ouverture d'une page (Ruling R43). Les quatre autres, décrites dans la conception (§9), en sont là :
- une édition faite sur un bloc pendant qu'un autre client le déplace était perdue (Ruling R39) : **levé en partie au lot 3** ; ce qui reste perdu est dit plus bas, dans les limites de la partie A ;
- la réécriture complète d'une page par `PUT /mmd` remplace tout son contenu : une frappe faite au même instant sur le contenu remplacé est perdue (Ruling R40, remède prévu au lot 5 : l'écriture par différence). Depuis le lot 3, elle garde l'identifiant des blocs inchangés (Ruling R201), et occupe la page moins d'une seconde pour 10 000 paragraphes courts, au lieu de 2,5 à 4 s (Ruling R369, mesuré en release) ; une page plus grande ou plus riche reste plus longue à récrire (voir les limites de la partie A) ;
- `PUT /mmd` refuse une prop hors du registre des blocs, parce qu'un navigateur l'effacerait ensuite sans bruit (Ruling R41) ;
- deux avis de sécurité RustSec sont ouverts sans mise à jour possible, sur `libsqlite3-sys` et `smallstr` ; le premier ne touche qu'un serveur qui ouvrirait une base écrite par un tiers, ce que Mementum ne fait pas (Ruling R44).

Un « Ruling » est une décision prise en cours de route, numérotée (R1, R2…) et expliquée dans le registre de son lot.

### Ce que livre le lot 2

Tout le produit est maintenant sous comptes et sous droits. Rien ne se lit ni ne s'écrit sans compte. Le serveur vérifie le droit de la personne à chaque demande, et à chaque connexion d'édition en direct, qu'il ferme dès qu'une écriture change ce droit.

- **Comptes.** Chacun a un compte local : une adresse e-mail et un mot de passe. Le premier compte créé administre l'instance. Ensuite, on ne crée un compte qu'avec le lien d'une invitation, sauf si l'exploitant ouvre les inscriptions (voir « Réglages »). Se connecter ou s'inscrire dans un navigateur qui portait déjà une session ferme cette session, et les connexions en direct du compte qui la portait (Ruling R90). Dans un navigateur, une session dure au plus 30 jours après la connexion (Ruling R117). Changer son mot de passe ferme toutes les sessions du compte, sur tous ses appareils, y compris une connexion en cours avec l'ancien mot de passe (Ruling R120).
- **Espaces**, comme les espaces de travail de Notion (demande d'Alex, confirmée par Lenny le 2026-11-10). Chacun reçoit le sien à l'inscription (sauf s'il arrive par une invitation), en crée d'autres (« Perso », « Pro ») et passe de l'un à l'autre. Un espace ne voit rien des autres.
- **Membres et invités.** On invite quelqu'un dans tout l'espace : il devient membre (ou admin). On l'invite sur une seule page : il devient invité, et ne voit que cette page et ses sous-pages. Le propriétaire et les admins changent les rôles et retirent des personnes. Chacun peut quitter un espace, qui garde toujours au moins un propriétaire.
- **Pages privées et partagées.** La barre latérale range les pages en trois sections, comme Notion : « Espace » (les pages de tous les membres), « Partagé » (les pages ouvertes à des personnes précises) et « Privé » (les pages que seul leur auteur voit). La section se déduit des accès de la page : rien à ranger à la main. Une sous-page a les accès de sa page parente. Il y a quatre niveaux sur une page : lire, commenter (lire, et commenter sans modifier, depuis le lot 3 : voir « Ce que livre le lot 3, partie A »), modifier, et accès complet (partager, changer de page parente, supprimer pour de bon).
- **Partage d'une page.** Le bouton « Partager », en haut à droite, s'offre à qui a l'accès complet sur la page. Son menu invite une adresse à un niveau, change ou retire l'accès d'une personne, et règle l'accès général : « Seulement les invités », ou « Tous les membres de l'espace » à un niveau. L'accès général qu'une page parente donne est un plancher pour ses sous-pages : il s'y relève, sans se baisser ni se retirer (Rulings R93 et R110). Qui restreint l'accès général d'une page y garde l'accès complet : une page créée dans « Espace » peut ainsi redevenir privée, en cochant « Seulement les invités », après une confirmation : elle n'est alors plus vue des autres membres, propriétaires et admins compris (Rulings R122 et R127). Qui ne peut que lire une page la voit en lecture seule, avec un bandeau qui le dit. Un droit qui baisse ou se retire s'applique en direct chez qui a la page ouverte : elle passe en lecture seule, ou se ferme sur « Tu n'as plus accès à cette page ». Un niveau qui monte propose « Rouvrir la page » (Ruling R111).
- **Déplacer une page.** À la souris, ou par le menu « … » de sa ligne, pour qui travaille au clavier (Ruling R95). Une page reste dans sa section. Une sous-page qu'on sort à la racine garde son accès d'équipe ; les personnes nommées sur son ancienne page parente la perdent (Ruling R94).
- **Invitations par lien.** Il n'y a pas encore d'envoi d'e-mails : une invitation est un lien, à copier et à envoyer soi-même. Il sert une fois et vaut 7 jours. On peut le révoquer tant qu'il attend. C'est le lien qui donne l'accès, jamais l'adresse e-mail, que personne ne vérifie. Un lien ne vaut plus si son auteur a perdu le droit de le faire.
- **Corbeille.** Une page mise à la corbeille part avec ses sous-pages, et revient avec elles. À la corbeille, elle se lit encore mais ne s'écrit plus. On la supprime pour de bon depuis la corbeille.
- **Pages du lot 1.** Le binaire du lot 2 met à jour une base du lot 1 à sa première ouverture, après en avoir écrit une copie (voir plus bas). Ses pages vont dans un espace « Mon espace », section « Espace », que reprend le premier compte créé.

Le premier compte administre l'instance, mais au lot 2 ce titre ne lui donne aucun pouvoir dans l'application. Comme tout le monde, il ne voit que les espaces où il a un rôle. Personne, ni lui, ni le propriétaire ou un admin de l'espace, ne voit les pages privées des autres.

**Limites connues du lot 2** (détail dans la conception, §9) :
- dix mots de passe faux pour une même adresse la bloquent, même avec le bon mot de passe, pendant 15 minutes au plus : le quart d'heure court depuis le premier essai raté. Quelqu'un d'autre peut donc empêcher une personne de se connecter. Ses sessions déjà ouvertes continuent (Ruling R76) ;
- l'inscription dit si une adresse a déjà un compte (Ruling R77) ;
- même sur une instance où l'on s'inscrit sur invitation, quelqu'un du dehors peut savoir si une adresse a un compte. Il lui faut une douzaine d'essais par adresse quand il en sonde beaucoup à la fois, et plus de 10 000 pour une seule (Ruling R84). Une limite du nombre de demandes par adresse IP, prévue au lot 7, lèvera ce point et le blocage par un tiers ; elle freinera seulement ce que dit l'inscription ;
- dans un navigateur, il faut se reconnecter 30 jours après sa dernière connexion, même si l'on se sert de l'application tous les jours. Le serveur prolonge bien la session à l'usage, mais ne renvoie pas le cookie, qui s'éteint 30 jours après avoir été posé (Ruling R117) ;
- une personne qui a un compte, même invitée en lecture seule sur une page, peut surcharger le serveur si elle le veut : rien ne plafonne encore ses connexions en direct. Pour l'instant, n'invitez que des personnes de confiance ; les plafonds viennent au lot 7 (Rulings R54 et R121) ;
- un invité à qui l'on donne l'accès complet à une page peut y inviter d'autres personnes, comme dans Notion (Ruling R86) ;
- pas encore d'e-mails, de connexion par passkey, de suppression d'un espace ou d'un compte, de vidage automatique de la corbeille, ni de création d'espaces réservée par l'admin (Rulings R46, R47 et R52).

### Ce que livre le lot 3, partie A (« Collaboration »)

À plusieurs sur un espace, on voit tout de suite ce que font les autres, on commente une page ou un bloc, et on revient à une version d'avant. Ce que le serveur garde d'une page est désormais ce que les navigateurs affichent : plusieurs écarts entre eux sont fermés. La recette du propriétaire se déroule avec la [checklist](docs/recette/2026-11-12-p1-lot3a-recette.md).

- **La barre latérale en direct** (Rulings R133, R141, R150). Une page créée, renommée, déplacée, mise à la corbeille, restaurée ou supprimée par un autre y arrive en une fraction de seconde, comme un nom d'espace ou un rôle changé. Le serveur pousse la version de l'espace par une WebSocket à part (`/events/{id}`) ; l'onglet relit alors l'arbre, déjà filtré par les droits, et le compte. Rien d'autre qu'un numéro ne passe par ce canal : ni titre, ni page, ni compte. Un canal resté muet 45 secondes est tenu pour tombé, et il revient aussitôt au retour du réseau (Ruling R215). Le canal tombé, la barre se rabat sur ce qui la faisait vivre avant : relue toutes les dix secondes tant que l'onglet est visible (Ruling R128), et au retour sur l'onglet, au plus une fois toutes les cinq secondes (Ruling R97), le compte aussi (Ruling R126). Le contenu des pages, les commentaires et l'historique n'y passent pas : ils arrivent par la connexion d'édition de chaque page, à ses seuls lecteurs (Ruling R136).
- **Qui regarde la page** (Rulings R136, R163 à R167). La barre du haut montre les autres comptes qui ont la page ouverte : cinq avatars au plus, puis « +N », l'initiale sur la couleur du compte, le nom en infobulle, suivi de « (lecture) » pour qui ne peut pas écrire. La liste vient du serveur, d'après les connexions, pas de ce que les onglets annoncent. Le nom montré près du curseur de quelqu'un est celui de son compte : le serveur l'écrit, quoi que l'onglet annonce, et personne ne change ni n'efface le nom et le curseur de quelqu'un qui est là (Rulings R101, R136).
- **Commentaires** (Ruling R134). Le bouton « Commentaires », en haut de la page, dit combien de fils sont ouverts et ouvre un panneau à droite (par-dessus la page sur un écran de moins de 1 100 px). On y commente la page ; un bloc se commente par « Commenter » dans le menu de sa poignée, ou par Ctrl+Alt+M (Cmd+Option+M) depuis le bloc. Les fils suivent l'ordre des blocs dans la page ; on y répond, on réagit (👍 ❤️ 😄 🎉 👀 ✅), on modifie ou supprime son message, on résout un fil (qui va sous « Fils résolus ») et on le rouvre. Qui a l'accès complet supprime un fil entier, après une confirmation qui le nomme : c'est ce qui fait de la place quand un plafond est atteint (Ruling R352). Chaque bloc qui a un fil ouvert porte une pastille dans la marge droite, qui ouvre le fil. Un fil dont le bloc a été supprimé reste lisible, rangé sous « Bloc supprimé ». Rien n'est écrit dans le document : « Peut commenter » suffit, l'éditeur restant en lecture seule (un bouton « Commenter ce bloc » suit alors le bloc survolé, et Ctrl+Alt+M commente le bloc où est la sélection ; au clavier seul, il faut activer la navigation au curseur du navigateur, F7, pour viser un bloc). Sous une fenêtre ouverte (l'historique, une confirmation), Ctrl+Alt+M ne fait rien (Ruling R214). Un texte tapé et pas encore envoyé se retrouve quand on rouvre le panneau ; Échap, dans un champ qui en a, sort du champ sans rien effacer. Si le fil ou le message visé a été supprimé entre-temps, ou si l'on ne peut plus écrire (droit perdu, corbeille), le texte reste affiché en haut du panneau, sous « Textes non envoyés », avec la raison, pour être copié ou écarté ; le droit revenu, il retrouve sa place (Ruling R211). « Peut lire » lit les fils sans écrire ; à la corbeille, tout se lit et rien ne s'écrit. Ce que d'autres écrivent arrive en direct, par la connexion d'édition de la page. Plafonds (Rulings R204, R350, R351, R357, R358) : par page, 2 000 fils, 10 000 messages, 4 Mio de texte et 20 000 réactions ; par fil, 500 messages ; par message, 32 emojis différents et 10 000 caractères ; et la part de chaque compte sur une page, 200 fils, 2 000 messages, 1 Mio de texte et 2 000 réactions, dont 8 emojis par message. Un plafond atteint est dit par le serveur, tel quel, et rien n'est écrit.
- **Historique** (Ruling R135). Le serveur garde des versions de chaque page : l'état complet de la page à un instant, avec les personnes qui l'ont modifiée depuis la version d'avant. Une version est coupée au fil des modifications (au plus une toutes les dix minutes, réglable : `MEMENTUM_VERSION_EVERY_SECS`), quand plus personne n'a la page ouverte, et avant une restauration si rien ne garde déjà ce qu'elle remplace ; une page garde ses cent dernières, dont vingt « avant une restauration » au plus (Ruling R189). L'historique se lit à partir de « Peut modifier » : un lecteur ne lit pas ce qui a été effacé avant qu'on lui partage la page (Rulings R203, R216). Le menu « … », en haut à droite de la page, ouvre « Historique » : à gauche, « Version actuelle » puis les versions, la plus récente d'abord, avec leur date, leur âge et leurs auteurs (« avant une restauration » marque ce qu'une restauration a remplacé) ; à droite, la version choisie, telle que la page la montrerait, sans que rien de la page ne change. « Restaurer cette version », hors corbeille, remplace son contenu pour tout le monde, après une confirmation qui prévient quand d'autres ont la page ouverte ; ce qui est remplacé reste dans l'historique. À la corbeille, l'historique se lit sans restaurer. Le menu « … » ne montre que ce que le niveau permet, et n'est pas là quand il n'a rien à offrir. Si le niveau tombe pendant que la fenêtre est ouverte, elle reste, avec le refus du serveur, jusqu'à ce qu'on la ferme.
- **Déplacer un bloc pendant que d'autres écrivent** (Rulings R220, R221). Un bloc glissé à la souris, monté ou descendu au clavier, ou décalé par Tab et Maj+Tab, est recréé au même identifiant, et lui seul : une frappe dans un autre bloc reste à sa place, deux personnes qui déplacent deux blocs différents gardent la page entière, et les fils de commentaires gardent leur bloc. Le curseur et la sélection de chacun suivent leur bloc quand un autre le déplace. Ce qui a été tapé dans le bloc déplacé, ou ses sous-blocs, pendant le déplacement est gardé quand la frappe arrive au serveur avant le déplacement ; le reste est dit dans les limites, plus bas.
- **Ce que le serveur garde est ce que les navigateurs affichent** (Rulings R130, R131, R137, R198, R431, R496). Le serveur n'écrit ni ne relaie jamais les octets reçus d'un client, mais ce qu'il en a intégré. Une mise à jour qui commence au-delà de ce qu'il connaît d'un client, ou qui supprime ce qu'il n'a pas encore reçu, attend sa suite, comme dans un navigateur. Un tableau mal formé par un client se lit toujours en un MMD que `PUT /mmd` relit. Une marque de texte hors du schéma, une valeur de marque trop longue (une couleur de plus de 100 octets, une adresse de lien de plus de 16 Kio) ou un contenu sans type posé hors d'un texte sont refusés : le premier rendait la lecture d'une page démesurée, le dernier cachait la page aux agents. L'éditeur ne pose pas de lien de plus de 16 Kio, et le dit (Ruling R495).
- **`PUT /mmd`** (Rulings R201, R259, R368, R435, R535). Une page récrite par MMD garde l'identifiant de ses blocs inchangés : les fils de commentaires gardent leur bloc. Un MMD porte 12 000 blocs au plus (`422 too_many_blocks`), et un MMD dont l'écriture est estimée à plus de 8 secondes est refusé (`422 too_long_to_write`). Le serveur écrit une page une requête à la fois : `PUT /mmd` et les restaurations attendent leur tour, huit au plus par page, dont deux d'un même compte ; au-delà, `429 page_busy`, à réessayer après une seconde (`Retry-After: 1`). Un agent qui envoie trois `PUT /mmd` de front sur une même page reçoit donc un `429` pour le troisième.
- **Le compte attendu** (Rulings R540 à R545). Le cookie de session est commun aux onglets d'un navigateur. Rien de ce qu'un onglet envoie ne part sous un autre compte : si un autre onglet du même navigateur se connecte sous un autre compte, le serveur refuse ce que l'onglet envoie ensuite, sans rien faire, et l'onglet repart sous le compte connecté ; un geste fait à ce moment est à refaire. Les outils et agents qui appellent l'API peuvent nommer le compte attendu par l'en-tête `X-Mementum-Account` (refus `409 account_changed`). Se déconnecter déconnecte ce navigateur, quel que soit le compte que ses autres onglets y ont connecté (Ruling R542).
- **Les écritures, une à la fois et tour à tour** (Rulings R510 à R512). Le serveur écrit dans sa base une écriture à la fois, et sert tour à tour chaque page ouverte et chaque compte. Un compte qui écrit sans relâche ne fait plus attendre la frappe des autres : sous huit boucles d'écriture, une frappe passe en quelques millisecondes au lieu de plus d'une demi-seconde, et le refus `500 database is locked` qui tombait sous la contention a disparu (Ruling R485).

**Limites connues de la partie A** (détail dans la conception, §9 et §11) :
- Restaurer une version remplace tout le contenu de la page, comme `PUT /mmd` : une frappe faite au même instant par quelqu'un d'autre, sur le contenu remplacé, est perdue, et n'est dans aucune version (Ruling R188) ; de même pour `PUT /mmd` (Ruling R40). L'écriture par différence du lot 5 les gardera. La restauration n'est pas bornée en taille (on ne refuse pas à une page son propre historique) : une page de 30 000 blocs se restaure en plus de dix secondes, pendant lesquelles la page est occupée (Rulings R259, R369). Récrire par `PUT /mmd` une page déjà énorme (venue des navigateurs, qui ne sont pas bornés) coûte aussi le retrait de tout ce qu'elle porte : un paragraphe écrit par-dessus 600 000 cellules de tableau occupe la page près de 8 secondes (Ruling R382).
- Les lectures de MMD de `PUT /mmd` passent quatre à la fois, les comptes tour à tour, sans borne sur le nombre de requêtes qui attendent (Rulings R383, R436) ; une couleur de plage de plus de 100 octets est refusée par `PUT /mmd`, et lue absente d'une page (Ruling R381).
- Commentaires : rien ne pagine encore la lecture des fils d'une page ; une page pleine jusqu'à ses plafonds fait lire environ 10 Mio à chaque onglet qui l'ouvre, à sa première synchro. Une mention dans un commentaire ne prévient personne (boîte de réception en P3). Un texte non envoyé ne vit qu'en mémoire : recharger l'onglet le perd, et rien ne l'annonce aux lecteurs d'écran quand il passe sous « Textes non envoyés ».
- Historique : quelqu'un qui peut modifier une page peut en faire partir les versions les plus anciennes, en y écrivant puis en la laissant se vider, à répétition (le débit sera plafonné au lot 7).
- La présence (Ruling R109) : pendant qu'une personne est coupée du réseau, personne ne prend sa place, **tant que la page reste ouverte quelque part** (le serveur l'oublie trente secondes après le départ du dernier) ; sinon, quelqu'un qui peut modifier la page, qui a vu son onglet avant et rouvre la page le premier, peut la rendre invisible à son retour, jusqu'à ce que la page se vide de nouveau. Et de nombreux comptes qui peuvent modifier la page, environ quatre mille, peuvent faire oublier au serveur à qui appartient l'onglet d'une personne coupée (Ruling R162). Après une coupure, l'onglet annonce un état neuf à chaque connexion : la présence d'une personne qui ne tape pas revient tout de suite (Ruling R165). Rien de cela ne touche au contenu ni aux droits. La liste des présents donne à chacun l'identifiant du compte des autres présents, qui dit sa date de création.
- Le canal de l'espace dit à un invité quand l'espace change, et combien d'écritures de structure y ont lieu, pages privées comprises : jamais ce qu'elles sont (Ruling R133).
- Les écritures tour à tour (Ruling R511) : la part est égale par page et par compte, pas par personne. Un compte qui tape dans beaucoup de pages à la fois fait attendre d'autant les écritures des autres (la frappe d'un autre sort de son budget vers 80 pages) ; et rien ne borne encore le nombre d'écritures en file (Ruling R516). Les plafonds par compte viennent au lot 7.
- Une base d'avant le lot 3 peut garder, au journal d'une page, une suppression qui vise ce que tapera encore un onglet ouvert depuis : ce qu'il tape dans cette plage est supprimé chez tous, jusqu'à ce qu'il soit rechargé (Ruling R198). Aucun client officiel n'en écrit : il y faut un client écrit à la main par quelqu'un qui peut modifier la page.

Le dossier `spike/` est le **prototype jetable** de la phase 0 : on en garde les leçons, les fixtures et les tests d'aller-retour, pas le code.

## Décisions déjà prises

| Sujet | Décision |
|---|---|
| Nom | **Mementum** (Memento écarté : *Memento Database* existe déjà dans la même catégorie) |
| Licence | **AGPL-3.0** pour le serveur et l'application web ([LICENSE](LICENSE)), **Apache-2.0** pour le SDK et la spec du format MMD ; **DCO sans CLA** |
| Modèle économique | Auto-hébergement **entièrement gratuit, avec toutes les fonctionnalités** ; offre hébergée payante **Mementum Cloud** pour ceux qui ne veulent pas gérer de serveur (le cloud vend l'hébergement, jamais des fonctions) |
| Pile | Serveur **Rust** en binaire unique (Axum, tokio, yrs, sqlx), **SQLite** par défaut, Postgres et S3 en option ; client React + Vite + **BlockNote** ; cache SQLite WASM |
| Temps réel | **Yjs** (CRDT) pour le texte des pages, confirmé par la phase 0 face à Loro ; journal d'opérations dont le serveur fait autorité pour l'arbre, les bases et les permissions |
| Organisation du contenu | **Espaces** façon Notion (demande d'Alex du 2026-11-09, confirmée par Lenny le 2026-11-10 ; ils remplacent les organisations et les coffres façon Obsidian) : un compte a plusieurs espaces (« Perso », « Pro »), on invite quelqu'un dans l'espace ou sur une page, et les jetons d'API portent sur un espace ou sur des pages |
| Autonomie | Pas de miroir en fichiers ; **import Notion et vault Obsidian dès la phase 1**, export complet sans perte |
| IA | **Serveur MCP dès la phase 1** ; agents en mode suggestion par défaut ; IA indépendante du fournisseur (Claude, GPT, Mistral ou 100 % local via Ollama) |
| Design | Direction **A « Papier »**, la plus proche de Notion pour ne pas dépayser |

La liste complète, avec les raisons, est dans la conception, §12 et §13.

## Lire dans cet ordre pour reprendre le projet

1. **Ce README.**
2. **La conception** : [`docs/specs/2026-11-09-mementum-design.md`](docs/specs/2026-11-09-mementum-design.md). C'est la référence : vision, fonctionnalités, architecture, format MMD, MCP, feuille de route (§9).
3. **Les résultats de la phase 0** : [`spike/RESULTS.md`](spike/RESULTS.md). Ce qui a été mesuré, ce qui est confirmé ou corrigé, et **ce qui change pour la phase 1**.
4. **Les plans et leurs registres de décisions** : phase 0 ([plan](docs/superpowers/plans/2026-11-09-phase0-spike.md), [registre](docs/superpowers/logs/2026-11-09-phase0-ledger.md)) ; lots 1 à 3 de la phase 1 (plans dans `docs/superpowers/plans/`, registres dans `docs/superpowers/logs/`). Les Rulings cités par ce README et par la conception y sont expliqués.
5. **Si besoin, la recherche** : [`docs/research/2026-11-09-etat-de-l-art.md`](docs/research/2026-11-09-etat-de-l-art.md) (synthèse) et [`docs/research/notes/`](docs/research/notes/) (notes sourcées).
6. **Pour un agent IA** : [`CLAUDE.md`](CLAUDE.md) (conventions et pièges déjà rencontrés).

## Organisation du dépôt

```
LICENSE                     AGPL-3.0
README.md                   ce fichier
CLAUDE.md                   consignes pour les agents IA (Claude Code, etc.)
docs/
  specs/                    conception (la référence)
  research/                 état de l'art + notes sourcées
  design/                   maquettes des 3 directions (A retenue)
  recette/                  checklists de recette du propriétaire, par lot
  superpowers/
    plans/                  plans d'implémentation (phase 0, lots 1 à 3 de la phase 1)
    logs/                   registres d'exécution (décisions prises en cours de route)
crates/mementum-*/          serveur Rust : schema, md, crdt, store, acl, sync, server, cli
apps/web/                   client React + Vite + BlockNote, tests e2e Playwright
packages/protocol/          types TypeScript de l'API, générés depuis Rust
spec/                       registre des blocs (schema/) et fixtures MMD (mmd/fixtures/)
tools/                      contrôle des licences npm
spike/                      phase 0 : prototype JETABLE
  RESULTS.md                mesures et verdict
  server/                   serveur de synchro Yjs (axum + yrs + SQLite) + agent pair CRDT
  mmd/                      convertisseur MMD ⇄ arbre de blocs ⇄ Yjs (format BlockNote)
  dbbench/                  bases de 100 000 lignes (SQLite JSON + index)
  crdtbench/, crdtbench-js/ benchmark Yjs/yrs contre Loro
  web/                      éditeur BlockNote collaboratif + tests e2e Playwright
  fixtures/                 fixtures partagées (MMD canonique, Yjs, BlockNote)
```

La structure cible du vrai projet (crates `mementum-*`, `apps/web`, etc.) est décrite dans la conception, §8.

## Lancer Mementum

Prérequis : Rust 1.96+, Node 24+ et pnpm 11+ (`corepack pnpm` si pnpm n'est pas dans le PATH).

```bash
corepack pnpm install                     # une fois, à la racine
cargo run -p mementum-cli -- serve        # serveur sur 127.0.0.1:8080, données dans ./data
corepack pnpm -C apps/web dev             # dans un autre terminal : Vite sur le port 5173
```

Puis ouvrir `http://localhost:5173`. Vite relaie `/api`, `/sync` et `/events` (WebSockets comprises) vers le serveur. Au lot 3, l'application est encore servie par Vite, comme en développement : le binaire qui l'embarque et l'image Docker viennent au lot 7. Arrêté par Ctrl-C (ou `SIGTERM`), le serveur garde une dernière version des pages encore ouvertes avant de fermer la base (Ruling R135) ; après un arrêt brutal, rien du contenu n'est perdu, et les écritures pas encore versionnées le sont après la prochaine ouverture de la page.

> **Instance du lot 1.** Au premier démarrage de ce binaire, la base est mise à jour (migration `0002`). Juste avant, le serveur en écrit une copie à côté d'elle : `data/mementum.db.avant-0002.bak`. Créez alors le premier compte vous-même, depuis la machine, avant de rouvrir l'instance à qui que ce soit (tunnel ou mandataire coupé) : la première personne qui s'inscrit reçoit toutes les pages et devient admin, et le serveur dit à tout visiteur, sans session, qu'aucun compte n'existe encore (voir « Premier compte »). Pour revenir en arrière : arrêtez le serveur, remettez la copie à la place de `data/mementum.db`, supprimez `mementum.db-wal` et `mementum.db-shm` s'ils existent, puis relancez le binaire du lot 1. Le détail est plus bas, sous « Copies avant migration ».
>
> Une base déjà ouverte par un binaire intermédiaire du lot 2, d'avant sa clôture, peut refuser de démarrer : sa migration `0002` a changé une fois en cours de lot (Ruling R53). Repartez alors de la copie d'avant migration.
>
> **Instance du lot 2.** Au premier démarrage du binaire du lot 3, la base est mise à jour (migrations `0003`, puis `0004` de la partie B), après une copie écrite à côté d'elle : `data/mementum.db.avant-0003.bak`. La migration `0003` a changé plusieurs fois en cours de lot (Ruling R139) : une base déjà ouverte par un binaire intermédiaire du lot 3 peut refuser de démarrer. Repartez alors de la copie d'avant migration.

### Premier compte

Sur une instance neuve, l'application propose d'abord de créer le premier compte. Il administrera l'instance, quel que soit le réglage des inscriptions. Tant qu'aucun compte n'existe, le serveur le rappelle dans son journal à chaque démarrage. Créez ce compte avant de rendre le serveur joignable par d'autres : sinon, le premier venu devient administrateur. Le serveur ne le cache pas : il répond à tout visiteur, sans session, qu'aucun compte n'existe encore. Sur une base du lot 1, ce compte devient propriétaire de l'espace « Mon espace », où sont toutes les pages, dans la section « Espace », avec leur contenu : le premier venu les recevrait toutes. Sinon, il reçoit un espace neuf, « Espace de » suivi de son nom.

### Réglages

| Réglage | Variable | Option de `serve` | Défaut |
|---|---|---|---|
| adresse d'écoute | `MEMENTUM_BIND` | `--bind` | `127.0.0.1:8080` |
| dossier des données | `MEMENTUM_DATA_DIR` | `--data-dir` | `./data` |
| inscriptions | `MEMENTUM_SIGNUP` | `--signup` | `invite` |
| adresse publique | `MEMENTUM_PUBLIC_URL` | `--public-url` | aucune |
| intervalle des versions d'une page, en secondes (1 au moins) | `MEMENTUM_VERSION_EVERY_SECS` | `--version-every-secs` | `600` |

Une option l'emporte sur sa variable. Une valeur invalide arrête le démarrage, avec un message qui la nomme. Par exemple : `cargo run -p mementum-cli -- serve --signup open`.

- **`MEMENTUM_SIGNUP`** dit ce que peut faire quelqu'un qui arrive sans invitation. Avec `invite` (le défaut), il peut seulement se connecter : pour créer un compte, il faut le lien d'une invitation. Avec `open`, n'importe qui crée un compte, et reçoit son propre espace. Le premier compte est permis dans les deux cas (Ruling R50).
- **`MEMENTUM_PUBLIC_URL`** est l'adresse sous laquelle les gens atteignent l'instance, par exemple `https://notes.exemple.fr` derrière un mandataire HTTPS ou un tunnel. Elle sert à trois choses. En `https`, le cookie de session ne part que sur une connexion chiffrée (`Secure`). Son nom d'hôte est accepté dans l'en-tête `Host`, même quand le serveur n'écoute que sur la boucle locale (Ruling R78). Et les écritures et la synchronisation (`/sync`) venues de cette adresse sont acceptées (Ruling R51). Sans elle, derrière un mandataire HTTPS :
  - le cookie n'a pas `Secure` ;
  - si le mandataire garde le `Host` public et que le serveur n'écoute que sur la boucle locale, tout répond `421 bad_host` ;
  - si le mandataire réécrit `Host`, toute écriture et toute connexion `/sync` reçoivent `403 bad_origin`.
- Un lien d'invitation reprend l'adresse sous laquelle on a ouvert l'application. Pour envoyer un lien qui marche chez les autres, ouvrez l'application par son adresse publique avant d'inviter.
- Les journaux vont sur la sortie d'erreur, au niveau `info`. `RUST_LOG` les règle, par exemple `RUST_LOG=mementum_sync=debug`.

> **Écouter hors de la boucle locale** (`serve --bind 0.0.0.0:8080`, ou une adresse du réseau). Ce sont alors les comptes qui protègent les pages : aucune ne se lit ni ne s'écrit sans session et sans droit. Le contrôle de l'en-tête `Host`, qui protège du DNS rebinding, ne vaut que sur la boucle locale ; le serveur écrit dans son journal qu'il est coupé. Créez d'abord le premier compte. Pour un accès depuis Internet, mettez un mandataire HTTPS devant, et réglez `MEMENTUM_PUBLIC_URL`.

### Mot de passe perdu

Il n'y a pas encore d'envoi d'e-mails. Un mot de passe oublié se règle sur le serveur, par quelqu'un qui a accès au dossier des données (Ruling R47) :

```bash
cargo run -p mementum-cli -- user set-password --email alice@exemple.fr
```

Mettez l'adresse du compte à la place de `alice@exemple.fr`. Ajoutez `--data-dir` suivi du dossier si les données ne sont ni dans `./data` ni dans `MEMENTUM_DATA_DIR`. Avec le binaire compilé, la commande est `mementum user set-password --email alice@exemple.fr`. Sous Windows, pendant que le serveur tourne depuis `target/debug`, `cargo run` échoue (« Accès refusé ») dès qu'il doit recompiler : lancez alors le binaire déjà compilé, `target\debug\mementum.exe user set-password --email alice@exemple.fr`.

La commande lit le nouveau mot de passe sur **la première ligne de son entrée standard** : tapé au clavier puis Entrée, ou passé par un tuyau ou une redirection. Tapé dans un terminal, il reste affiché à l'écran ; la commande le rappelle avant de lire. Il fait 10 à 200 caractères, en UTF-8, sans caractère de contrôle (tabulation, retour chariot au milieu de la ligne, octet nul) : sinon, la commande refuse et ne change rien. Elle n'affiche jamais le mot de passe, seulement le nombre de caractères enregistrés : vérifiez ce nombre.

Ensuite, toutes les sessions du compte sont fermées. La commande peut tourner pendant que le serveur tourne. Mais elle ne ferme pas les pages que ce compte a déjà ouvertes dans un navigateur : elles restent connectées, en lecture comme en écriture, jusqu'à leur prochaine reconnexion, puis ne reviennent pas, faute de session (Rulings R73 et R125). Elle ne lève pas non plus le blocage après dix mots de passe faux : il dure 15 minutes au plus, et le serveur le garde en mémoire. Pour chasser quelqu'un d'un compte, ou lever ce blocage tout de suite, redémarrez le serveur juste après la commande.

**Sous Windows PowerShell 5.1** (celui livré avec Windows, pas PowerShell 7) :
- un tuyau (`'…' | mementum user set-password …`) remplace par défaut chaque lettre accentuée par `?`, sans aucune erreur. La commande enregistre alors un autre mot de passe, de même longueur. Tapez plutôt le mot de passe quand la commande le demande, ou lancez d'abord `$OutputEncoding = [System.Text.UTF8Encoding]::new()` dans la même fenêtre ;
- une entrée en UTF-16, comme un fichier écrit par `>` ou par `Out-File`, est refusée sans rien changer.

### Copies avant migration

Au démarrage, si une migration de schéma attend une base existante, le serveur en écrit d'abord une copie à côté (`data/mementum.db.avant-0002.bak` avant la migration `0002`). Il ne démarre pas s'il ne peut pas l'écrire. `mementum user set-password` fait de même quand il est le premier à ouvrir la base.

Chaque tentative refait la copie et la compare à toutes celles de cette version. Si l'une est identique, elle suffit. Sinon (la base a changé, par exemple une migration ratée puis des pages écrites avec l'ancienne version), la nouvelle copie prend le nom sans date s'il est libre, sinon un nom daté en UTC (`mementum.db.avant-0002.20261110T081500Z.bak`, puis `…Z-2.bak` dans la même seconde). Aucune copie n'est écrasée ni supprimée automatiquement.

Une copie garde pour toujours le contenu d'avant la migration, pages supprimées depuis comprises. Seuls les droits du dossier des données la protègent. Une fois la migration vérifiée, supprimez ces copies, ou rangez-les à l'abri.

Pour trouver la plus récente, regardez la date de modification des fichiers. Ni le nom ni l'ordre alphabétique ne suffisent : si l'on a déplacé la copie sans date (pour la restaurer, par exemple), la copie suivante reprend ce nom.

Sur un système de fichiers sans liens physiques (partage réseau SMB, exFAT, certains volumes Docker), la copie est quand même écrite, et le journal le signale : pendant une mise à jour, ne lancez alors qu'un processus à la fois sur ces données (Ruling R75).

Pour restaurer une copie : arrêter le serveur, la remettre à la place de `data/mementum.db`, supprimer `mementum.db-wal` et `mementum.db-shm` s'ils existent (ils appartiennent à la base remplacée), puis relancer le serveur.

Arrêtez toujours le serveur avant de restaurer une copie, ou de récrire la base à la main. Il garde en mémoire les droits de chaque espace, datés par un numéro de révision qui ne fait que monter. Sur une base revenue en arrière, il servirait ces droits d'avant jusqu'à son redémarrage (Ruling R98).

### Tests

```bash
cargo test --workspace                                  # Rust
corepack pnpm -C apps/web test                          # Vitest (schéma BlockNote ⇄ registre)
corepack pnpm -C apps/web typecheck                     # TypeScript
node tools/check-npm-licenses.mjs                       # licences npm de production
corepack pnpm -C apps/web exec playwright install chromium
corepack pnpm e2e                                       # Playwright : lance le serveur et Vite
corepack pnpm e2e:budgets                               # budgets du navigateur (§5.10), à part
```

Les tests e2e ne réutilisent jamais un serveur déjà lancé : si le port 5173 ou 8080 est pris, ils s'arrêtent en le nommant. Choisir alors d'autres ports, par exemple `MEMENTUM_E2E_WEB_PORT=5174 corepack pnpm e2e`. Ces deux variables valent aussi hors des tests : `MEMENTUM_E2E_WEB_PORT` règle le port de Vite, `MEMENTUM_E2E_API_PORT` le serveur que vise son mandataire (à donner aussi à `serve --bind 127.0.0.1:<port>`).

`corepack pnpm e2e` ne mesure pas les budgets du navigateur. `corepack pnpm e2e:budgets` les mesure sur ce que les gens utiliseront : l'application construite (`vite build`, servie par `vite preview`) et un serveur compilé en `--release`. Sa première compilation en release est longue.

## Lancer le prototype de la phase 0

Prérequis : Rust 1.96+, Node 24+, pnpm 11+.

```bash
cd spike
cargo test -p spike-server -p spike-mmd -p spike-db
cargo test --release -p spike-crdt-bench
```

Tests e2e (lancent eux-mêmes le serveur et Vite) :

```bash
cd spike/web
pnpm install
pnpm exec playwright install chromium
pnpm exec playwright test
```

Les ports 5173 et 8080 doivent être libres : Playwright réutilise un serveur déjà lancé, même s'il appartient à un autre projet. Pour régénérer les fixtures BlockNote et Yjs : `EXPORT_FIXTURES=1 pnpm exec playwright test e2e/export-fixture.spec.ts`. Le test de la page de 10 000 blocs est ignoré tant que `measure` n'a pas créé la page `bench-10k`.

Mesures (à refaire sur un Raspberry Pi 4 ou un VPS ARM, voir la fin de `spike/RESULTS.md`) :

```bash
cd spike
cargo build --release -p spike-server   # measure lance ce binaire
cargo run --release -p spike-server --bin measure
cargo test --release -p spike-db -- --ignored budgets_100k --nocapture
cargo run --release -p spike-db -- bench --rows 100000
cargo run --release -p spike-crdt-bench
```

L'éditeur de démo s'ouvre avec `cargo run --release -p spike-server --bin spike-server` (dans `spike/`), puis `pnpm dev` (dans `spike/web/`), à l'adresse `http://localhost:5173/?page=demo&user=Moi`.

## Prochaines étapes

Pour reprendre le projet sans contexte, humain ou agent : [`docs/REPRISE.md`](docs/REPRISE.md).

1. **Clôture du lot 3, partie A** ([plan](docs/superpowers/plans/2026-11-12-p1-lot3-collaboration-blocs-riches.md), validé par le propriétaire le 12 novembre ; décisions au [registre](docs/superpowers/logs/2026-11-12-p1-lot3-ledger.md) ; constats de la [relecture finale](docs/superpowers/logs/2026-11-12-p1-lot3-relecture-finale.md) ; état au jour le jour dans l'[avancement](docs/superpowers/logs/2026-11-12-p1-lot3-avancement.md)). Le code est fini et sur `main` : les quatorze tâches (la Task 13 en trois parts), la relecture finale et ses tours de correction, la remise au vert de la CI, la file d'écriture du serveur et le compte attendu. Restent : la passe de la relecture finale qui relit la documentation contre le code, les suites sur un clone neuf comme la CI, les budgets mesurés au calme, l'essai de migration sur la copie d'une base du lot 2, puis la **recette du propriétaire**, avec la [checklist](docs/recette/2026-11-12-p1-lot3a-recette.md). Ce qui attend un avis du propriétaire : la forme à clés et ses régressions acceptées (Rulings R220, R223) et la limite qui reste (R222) ; l'historique lu à partir de « Peut modifier » (R203) ; la restauration qui remplace tout le contenu (R188).
2. **Recette du lot 2** : aucun défaut à corriger avant le lot 3 (réponse de Lenny, 12 novembre). [Checklist](docs/recette/2026-11-10-p1-lot2-recette.md). Les espaces façon Notion sont confirmés par Lenny (2026-11-10, Ruling R45). Plusieurs choix du lot attendent encore un avis : l'accès complet donné par défaut aux membres sur les pages d'espace (R48), les inscriptions sur invitation par défaut (R50), le menu du compte placé dans le sélecteur d'espace (R80), l'invité en accès complet qui peut inviter (R86), et la page sortie à la racine qui garde son accès d'équipe (R94).
3. **Mesures ARM** de la phase 0 (Pi 4 ou Hetzner CAX11), à faire **avant la phase 2**, et report dans `spike/RESULTS.md`. Le budget le plus à risque est « filtre + tri < 300 ms ».
4. **Lots 3 à 7**, dans cet ordre : collaboration et blocs riches ; recherche ; agents (REST complet, MCP v1, jetons d'API) ; imports et export ; livraison. Ils reprennent les limites connues des lots 1 et 2 :
   - lot 3 : partie A faite (commentaires, historique, arbre des pages et présence en direct ; R97 et R128 ne sont plus qu'un repli, R109 est levé en partie ; tableaux mal formés lus en grille, R54 ; déplacements de blocs, R39 levé en partie). Partie B en cours : fichiers envoyés, blocs riches, mentions, favoris, récents, fil d'Ariane ; là ou au lot 5, un porteur durable pour les props hors registre (Ruling R41). Après le lot 3, un plan à part pour la frappe arrivée après le déplacement d'un bloc (R222) ;
   - lot 5 : écriture MMD par différence, pour garder une frappe faite pendant une réécriture ou une restauration (Rulings R40, R188) ;
   - lot 7 : livraison (binaire qui embarque l'application, image Docker, fichier `mementum.toml`, inscription par domaine e-mail) ; limites du nombre de demandes et plafonds par compte (R54, R121, R511), dont celle par adresse IP : elle lèvera R76 et R84, et freinera R77 sans le lever (là où l'inscription est ouverte, sa réponse reste un indice en une seule demande) ; contrôle des avis de sécurité en CI (R44).
5. **Sans date, à décider** : envoi d'e-mails (invitations, mot de passe oublié), suppression d'un espace ou d'un compte, création d'espaces réservée par l'admin d'instance, vidage automatique de la corbeille, passkeys (R46).

## Méthode de travail

Le projet suit la méthode *superpowers* :
- conception validée avant tout code ;
- plan découpé en tâches ;
- exécution en TDD (test qui échoue, puis code, puis test qui passe) ;
- relecture de toute la branche ;
- chaque écart au plan consigné comme « Ruling » dans le registre.

Les documents vont dans `docs/specs/`, les plans dans `docs/superpowers/plans/`, les registres dans `docs/superpowers/logs/`.

## Licence

Serveur et application web : [GNU AGPL-3.0](LICENSE). SDK, bibliothèques clientes et spécification du format MMD : Apache-2.0, à publier avec ces composants. Contributions sous DCO (`git commit -s`).
