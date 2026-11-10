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

## Où en est le projet (10 novembre 2026)

| Étape | État |
|---|---|
| Recherche (Notion 2026, alternatives open source, architecture temps réel, accès des agents IA) | faite : [`docs/research/`](docs/research/) |
| Conception complète | validée : [`docs/specs/2026-11-09-mementum-design.md`](docs/specs/2026-11-09-mementum-design.md) |
| Maquettes (3 directions, **A « Papier » retenue**) | faites : [`docs/design/`](docs/design/) |
| Phase 0 : prototype jetable qui valide les choix techniques | faite sur PC et relue (relecture finale du 9 novembre, voir [`spike/RESULTS.md`](spike/RESULTS.md)) ; **mesures ARM reportées avant la phase 2** (bases de données) |
| Phase 1 « Écrire ensemble » (MVP) | en cours, découpée en 7 lots. Lot 1 « Socle et édition en direct » **fait** ([plan](docs/superpowers/plans/2026-11-09-p1-lot1-socle-edition.md), [registre](docs/superpowers/logs/2026-11-09-p1-lot1-ledger.md)). Lot 2 « Comptes, espaces, droits et corbeille » **livré** ([plan](docs/superpowers/plans/2026-11-10-p1-lot2-comptes-espaces-droits.md), [registre](docs/superpowers/logs/2026-11-10-p1-lot2-ledger.md), [checklist de recette](docs/recette/2026-11-10-p1-lot2-recette.md)). Lots 3 à 7 à venir |

Le lot 1 a livré le vrai code (`crates/mementum-*`, `apps/web`, `packages/protocol`, `spec/`) : un arbre de pages, l'édition à plusieurs en direct dans BlockNote et le format MMD de la phase 1. Le lot 2 a corrigé une de ses limites connues : l'arbre des pages ne ralentit plus l'ouverture d'une page (Ruling R43). Les quatre autres restent, décrites dans la conception (§9) :
- une édition faite sur un bloc pendant qu'un autre client le déplace est perdue (Ruling R39, à traiter au lot 3) ;
- la réécriture complète d'une page de 10 000 blocs par `PUT /mmd` bloque cette page 2,5 à 4 s selon la machine et sa charge (Ruling R40, remède prévu au lot 5) ;
- `PUT /mmd` refuse une prop hors du registre des blocs, parce qu'un navigateur l'effacerait ensuite sans bruit (Ruling R41) ;
- deux avis de sécurité RustSec sont ouverts sans mise à jour possible, sur `libsqlite3-sys` et `smallstr` ; le premier ne touche qu'un serveur qui ouvrirait une base écrite par un tiers, ce que Mementum ne fait pas (Ruling R44).

Un « Ruling » est une décision prise en cours de route, numérotée (R1, R2…) et expliquée dans le registre de son lot.

### Ce que livre le lot 2

Tout le produit est maintenant sous comptes et sous droits. Rien ne se lit ni ne s'écrit sans compte. Le serveur vérifie le droit de la personne à chaque demande, et à chaque connexion d'édition en direct, qu'il ferme dès qu'une écriture change ce droit.

- **Comptes.** Chacun a un compte local : une adresse e-mail et un mot de passe. Le premier compte créé administre l'instance. Ensuite, on ne crée un compte qu'avec le lien d'une invitation, sauf si l'exploitant ouvre les inscriptions (voir « Réglages »). Se connecter ou s'inscrire dans un navigateur qui portait déjà une session ferme cette session, et les connexions en direct du compte qui la portait (Ruling R90). Dans un navigateur, une session dure au plus 30 jours après la connexion (Ruling R117). Changer son mot de passe ferme toutes les sessions du compte, sur tous ses appareils, y compris une connexion en cours avec l'ancien mot de passe (Ruling R120).
- **Espaces**, comme les espaces de travail de Notion (demande d'Alex, à confirmer avec Lenny). Chacun reçoit le sien à l'inscription (sauf s'il arrive par une invitation), en crée d'autres (« Perso », « Pro ») et passe de l'un à l'autre. Un espace ne voit rien des autres.
- **Membres et invités.** On invite quelqu'un dans tout l'espace : il devient membre (ou admin). On l'invite sur une seule page : il devient invité, et ne voit que cette page et ses sous-pages. Le propriétaire et les admins changent les rôles et retirent des personnes. Chacun peut quitter un espace, qui garde toujours au moins un propriétaire.
- **Pages privées et partagées.** La barre latérale range les pages en trois sections, comme Notion : « Espace » (les pages de tous les membres), « Partagé » (les pages ouvertes à des personnes précises) et « Privé » (les pages que seul leur auteur voit). La section se déduit des accès de la page : rien à ranger à la main. Une sous-page a les accès de sa page parente. Il y a quatre niveaux sur une page : lire, commenter (pour l'instant, comme lire), modifier, et accès complet (partager, changer de page parente, supprimer pour de bon).
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
- la présence (le nom et le curseur de chacun dans une page) n'est pas encore garantie par le serveur. Personne ne peut plus changer ni effacer le nom et le curseur de quelqu'un qui est là (Ruling R101). Mais quelqu'un qui peut modifier la page (plus un simple lecteur, Ruling R118) peut prendre la place d'une personne pendant qu'elle est coupée du réseau : à son retour, son nom et son curseur ne se voient plus chez les autres, tant qu'il reste là, et dans un cas jusqu'à ce que tout le monde ait quitté la page. Après une coupure, la présence d'une personne qui ne tape pas peut aussi mettre jusqu'à 30 secondes à revenir. Rien de cela ne touche au contenu ni aux droits (Ruling R109) ;
- un invité à qui l'on donne l'accès complet à une page peut y inviter d'autres personnes, comme dans Notion (Ruling R86) ;
- la barre latérale ne montre pas en direct ce que font les autres (pages créées, renommées, déplacées) : elle se met à jour quand on revient sur l'onglet, au plus une fois toutes les cinq secondes (Ruling R97), et le compte est relu avec elle : un rôle changé ailleurs arrive alors (Ruling R126). Une lecture qui a échoué repart d'elle-même au retour du réseau. Le direct viendra avec le journal d'opérations, au lot 3 ;
- pas encore d'e-mails, de connexion par passkey, de suppression d'un espace ou d'un compte, de vidage automatique de la corbeille, ni de création d'espaces réservée par l'admin (Rulings R46, R47 et R52).

Le dossier `spike/` est le **prototype jetable** de la phase 0 : on en garde les leçons, les fixtures et les tests d'aller-retour, pas le code.

## Décisions déjà prises

| Sujet | Décision |
|---|---|
| Nom | **Mementum** (Memento écarté : *Memento Database* existe déjà dans la même catégorie) |
| Licence | **AGPL-3.0** pour le serveur et l'application web ([LICENSE](LICENSE)), **Apache-2.0** pour le SDK et la spec du format MMD ; **DCO sans CLA** |
| Modèle économique | Auto-hébergement **entièrement gratuit, avec toutes les fonctionnalités** ; offre hébergée payante **Mementum Cloud** pour ceux qui ne veulent pas gérer de serveur (le cloud vend l'hébergement, jamais des fonctions) |
| Pile | Serveur **Rust** en binaire unique (Axum, tokio, yrs, sqlx), **SQLite** par défaut, Postgres et S3 en option ; client React + Vite + **BlockNote** ; cache SQLite WASM |
| Temps réel | **Yjs** (CRDT) pour le texte des pages, confirmé par la phase 0 face à Loro ; journal d'opérations dont le serveur fait autorité pour l'arbre, les bases et les permissions |
| Organisation du contenu | **Espaces** façon Notion (demande d'Alex du 2026-11-09, à confirmer avec Lenny ; ils remplacent les organisations et les coffres façon Obsidian) : un compte a plusieurs espaces (« Perso », « Pro »), on invite quelqu'un dans l'espace ou sur une page, et les jetons d'API portent sur un espace ou sur des pages |
| Autonomie | Pas de miroir en fichiers ; **import Notion et vault Obsidian dès la phase 1**, export complet sans perte |
| IA | **Serveur MCP dès la phase 1** ; agents en mode suggestion par défaut ; IA indépendante du fournisseur (Claude, GPT, Mistral ou 100 % local via Ollama) |
| Design | Direction **A « Papier »**, la plus proche de Notion pour ne pas dépayser |

La liste complète, avec les raisons, est dans la conception, §12 et §13.

## Lire dans cet ordre pour reprendre le projet

1. **Ce README.**
2. **La conception** : [`docs/specs/2026-11-09-mementum-design.md`](docs/specs/2026-11-09-mementum-design.md). C'est la référence : vision, fonctionnalités, architecture, format MMD, MCP, feuille de route (§9).
3. **Les résultats de la phase 0** : [`spike/RESULTS.md`](spike/RESULTS.md). Ce qui a été mesuré, ce qui est confirmé ou corrigé, et **ce qui change pour la phase 1**.
4. **Les plans et leurs registres de décisions** : phase 0 ([plan](docs/superpowers/plans/2026-11-09-phase0-spike.md), [registre](docs/superpowers/logs/2026-11-09-phase0-ledger.md)) ; lots 1 et 2 de la phase 1 (plans dans `docs/superpowers/plans/`, registres dans `docs/superpowers/logs/`). Les Rulings cités par ce README et par la conception y sont expliqués.
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
    plans/                  plans d'implémentation (phase 0, lots 1 et 2 de la phase 1)
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

Puis ouvrir `http://localhost:5173`. Vite relaie `/api` et `/sync` (WebSocket comprise) vers le serveur. Au lot 2, l'application est encore servie par Vite, comme en développement : le binaire qui l'embarque et l'image Docker viennent au lot 7.

> **Instance du lot 1.** Au premier démarrage de ce binaire, la base est mise à jour (migration `0002`). Juste avant, le serveur en écrit une copie à côté d'elle : `data/mementum.db.avant-0002.bak`. Créez alors le premier compte vous-même, depuis la machine, avant de rouvrir l'instance à qui que ce soit (tunnel ou mandataire coupé) : la première personne qui s'inscrit reçoit toutes les pages et devient admin, et le serveur dit à tout visiteur, sans session, qu'aucun compte n'existe encore (voir « Premier compte »). Pour revenir en arrière : arrêtez le serveur, remettez la copie à la place de `data/mementum.db`, supprimez `mementum.db-wal` et `mementum.db-shm` s'ils existent, puis relancez le binaire du lot 1. Le détail est plus bas, sous « Copies avant migration ».
>
> Une base déjà ouverte par un binaire intermédiaire du lot 2, d'avant sa clôture, peut refuser de démarrer : sa migration `0002` a changé une fois en cours de lot (Ruling R53). Repartez alors de la copie d'avant migration.

### Premier compte

Sur une instance neuve, l'application propose d'abord de créer le premier compte. Il administrera l'instance, quel que soit le réglage des inscriptions. Tant qu'aucun compte n'existe, le serveur le rappelle dans son journal à chaque démarrage. Créez ce compte avant de rendre le serveur joignable par d'autres : sinon, le premier venu devient administrateur. Le serveur ne le cache pas : il répond à tout visiteur, sans session, qu'aucun compte n'existe encore. Sur une base du lot 1, ce compte devient propriétaire de l'espace « Mon espace », où sont toutes les pages, dans la section « Espace », avec leur contenu : le premier venu les recevrait toutes. Sinon, il reçoit un espace neuf, « Espace de » suivi de son nom.

### Réglages

| Réglage | Variable | Option de `serve` | Défaut |
|---|---|---|---|
| adresse d'écoute | `MEMENTUM_BIND` | `--bind` | `127.0.0.1:8080` |
| dossier des données | `MEMENTUM_DATA_DIR` | `--data-dir` | `./data` |
| inscriptions | `MEMENTUM_SIGNUP` | `--signup` | `invite` |
| adresse publique | `MEMENTUM_PUBLIC_URL` | `--public-url` | aucune |

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

1. **Recette du lot 2** par le propriétaire, avec la [checklist](docs/recette/2026-11-10-p1-lot2-recette.md). Plusieurs choix du lot attendent un avis : les espaces façon Notion eux-mêmes (à confirmer avec Lenny, Ruling R45), l'accès complet donné par défaut aux membres sur les pages d'espace (R48), les inscriptions sur invitation par défaut (R50), le menu du compte placé dans le sélecteur d'espace (R80), l'invité en accès complet qui peut inviter (R86), et la page sortie à la racine qui garde son accès d'équipe (R94).
2. **Mesures ARM** de la phase 0 (Pi 4 ou Hetzner CAX11), à faire **avant la phase 2**, et report dans `spike/RESULTS.md`. Le budget le plus à risque est « filtre + tri < 300 ms ».
3. **Lots 3 à 7**, dans cet ordre : collaboration et blocs riches ; recherche ; agents (REST complet, MCP v1, jetons d'API) ; imports et export ; livraison. Ils reprennent les limites connues des lots 1 et 2 :
   - lot 3 : commentaires, historique, arbre des pages et présence en direct (journal d'opérations, présence vérifiée par le serveur, ce qui lèvera R97 et R109) ; déplacements de blocs sans perte d'édition (pistes (a) et (b) du Ruling R39, conception §11) ; tableaux mal formés écrits directement par un client Yjs (R54) ; là ou au lot 5, un porteur durable pour les props hors registre (Ruling R41) ;
   - lot 5 : écriture MMD par différence, pour ne plus bloquer une page pendant sa réécriture (Ruling R40) ;
   - lot 7 : livraison (binaire qui embarque l'application, image Docker, fichier `mementum.toml`, inscription par domaine e-mail) ; limites du nombre de demandes (R54), dont celle par adresse IP : elle lèvera R76 et R84, et freinera R77 sans le lever (là où l'inscription est ouverte, sa réponse reste un indice en une seule demande) ; contrôle des avis de sécurité en CI (R44).
4. **Sans date, à décider** : envoi d'e-mails (invitations, mot de passe oublié), suppression d'un espace ou d'un compte, création d'espaces réservée par l'admin d'instance, vidage automatique de la corbeille, passkeys (R46).

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
