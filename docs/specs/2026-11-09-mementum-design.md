# Mementum — Document de conception

> **Statut** : proposition. Nom (**Mementum**) et direction visuelle (**A « Papier »**) choisis par Lenny ; le reste est à valider
> **Date** : 2026-11-09 (d'abord « Vivace », puis « Memento », écarté parce que Memento Database existe déjà ; finalement « Mementum »)
> **Rédigé par** : Claude Code, à partir de 4 recherches (fonctionnalités Notion 2026, alternatives open source, architecture temps réel, accès des agents IA). La synthèse est dans [`docs/research/2026-11-09-etat-de-l-art.md`](../research/2026-11-09-etat-de-l-art.md) et les notes sourcées dans [`docs/research/notes/`](../research/notes/).
> **Amendement du 2026-11-10** : à la demande d'Alex (2026-11-09), des **espaces façon Notion** remplacent les organisations et les coffres façon Obsidian (§4.1, §4.4, §12). À confirmer avec Lenny (§13).
> **Mise à jour du 2026-11-10, lot 2 de la P1** : ce que le code du lot 2 (comptes, espaces, droits, corbeille) précise ou corrige est reporté aux §4.1 à §4.4, §5.6, §7.2, §7.4 et §9, avec le numéro de la décision (« Ruling ») du [registre du lot 2](../superpowers/logs/2026-11-10-p1-lot2-ledger.md). Les décisions marquées « à confirmer » attendent l'avis du propriétaire. Le lot est livré le 2026-11-10 : le §4.3 décrit le menu Partager tel qu'il est.

---

## 0. En une minute

**Mementum** est un espace de travail open source et auto-hébergeable, au niveau de Notion : pages en blocs, bases de données relationnelles, vues, équipes et permissions, édition à plusieurs en direct. Le contenu est rangé dans des **espaces** à la manière de Notion : chacun crée les siens (« Perso », « Pro »…), y invite qui il veut, et y garde des pages privées ou partagées. Mementum est **conçu dès le départ pour que les agents IA lisent et écrivent les pages aussi proprement que les humains**.

> *Mementum* = *memento* (du latin « souviens-toi ») + *momentum* (l'élan). C'est la mémoire de l'équipe et de ses agents, toujours en mouvement : en direct, chez soi.
> Slogan : **« Écrivez ensemble, en direct, chez vous. »**

Les 5 promesses :

1. **Rapide** : la page s'ouvre instantanément depuis le cache local, les bases restent fluides à 100 000 lignes.
2. **En direct** : curseurs, présence, édition simultanée, y compris quand un agent écrit en même temps que toi.
3. **Chez toi** : un seul binaire ou un seul conteneur, SQLite par défaut. Ça tourne sur un Raspberry Pi comme sur un cluster.
4. **Vraiment libre** : licence OSI, aucune limite de sièges, aucune fonction d'équipe payante, export sans perte.
5. **Natif IA** : un format Markdown canonique, un serveur MCP intégré qui comprend les bases de données, des agents membres de l'espace avec des droits, un journal et un mode suggestion.

---

## 1. Pourquoi

### 1.1 Les problèmes de Notion (état nov. 2026)
- **Fermé et cloud uniquement** : pas d'auto-hébergement, données chez un tiers. L'aide Notion reconnaît elle-même qu'un export complet ne permet pas de recréer l'espace de travail.
- **Lent sur les grosses bases** et les relations. Le mode hors ligne (août 2025) ne couvre que les pages marquées hors ligne.
- **IA chère et cloisonnée** : incluse seulement en Business/Enterprise, agents facturés en crédits depuis mai 2026, et uniquement avec les modèles choisis par Notion.
- **API bridée** : environ 3 requêtes/s (Free/Plus), 1 000 blocs par requête, limite de blocs du plan gratuit appliquée à l'API depuis le 8 sept. 2026.

### 1.2 Le trou du marché open source
Aucune alternative ne réunit les quatre critères suivants : **vraie base relationnelle dans les pages**, **temps réel**, **équipes et permissions**, **auto-hébergement léger sous licence libre sans plafond**.

| | Bases (relations + rollups + formules) | Temps réel | Équipes / permissions | Auto-hébergement simple | Licence vraiment libre pour les équipes |
|---|---|---|---|---|---|
| AppFlowy | relations + rollups, **pas de formules** | oui | **serveur devenu fermé**, 1 siège gratuit | non (12 services) | non |
| AFFiNE | **rien de tout ça** | oui | **10 sièges max** | oui (4 services) | non |
| Docmost | **Bases payantes** | oui | permissions par page **payantes** | oui (3 services) | non |
| Outline / La Suite Docs | **aucune base** | oui | oui | moyen (SSO obligatoire) | oui |
| Anytype | relations, pas de formules | sync | oui | non (15 services) | non (source-available) |
| SiYuan | la plus riche | non | **mono-utilisateur** | oui | oui |
| **Mementum** | **oui** | **oui** | **oui, sans plafond** | **1 conteneur** | **oui** |

Les plaintes récurrentes des auto-hébergeurs : fonctions d'équipe passées en payant (open-core), piles de 12 à 15 conteneurs, absence de comptes locaux (issue n°1 d'Outline), projets abandonnés, export en masse impossible (issue n°1 d'AppFlowy).

### 1.3 Ce qui change en 2026 : les agents
Notion a dû refaire son interface IA : le JSON de blocs coûtait trop de tokens, il est passé à un Markdown enrichi et à des éditions par chercher-remplacer. Chez les alternatives open source, aucun MCP ne comprend les bases de données. Écrire dans un document en direct (CRDT) pendant qu'un humain tape, avec attribution et annulation, n'est documenté nulle part. **C'est là que Mementum se distingue.**

---

## 2. Objectifs, non-objectifs, anti-objectifs

### Objectifs
- Parité avec le **cœur** de Notion : pages et blocs, bases avec les 24 types de propriétés, vues, relations, rollups et formules, espaces partagés (membres, invités, partage par page), commentaires, historique, suggestions.
- **Temps réel** partout : contenu des pages, arborescence, lignes de base, présence.
- **Performance mesurée** contre des budgets chiffrés (§5.10), vérifiés en CI, y compris sur machine ARM type Raspberry Pi.
- **Plusieurs espaces sur un même serveur** : n'importe qui monte son instance et y héberge sa famille, son asso ou son entreprise, chacune dans son espace. Une même personne a aussi les siens (« Perso », « Pro »).
- **Agents IA de première classe** : format canonique, MCP intégré, identité, permissions, journal, mode suggestion, annulation par exécution.
- **IA indépendante du fournisseur** : Claude, GPT, Mistral… ou 100 % local via Ollama. Rien n'est obligatoire.
- **Autonomie et portabilité** : **import complet depuis Notion et depuis un vault Obsidian dès la P1** (§4.6), plus Markdown et CSV. Export complet sans perte et réimportable (format `.mementum`). Mementum se suffit à lui-même : on y entre par l'import, on en sort par l'export.

### Non-objectifs (v1)
- Notion Mail et un vrai Calendar (Notion a d'ailleurs fermé Mail le 22/09/2026).
- Apps mobiles natives : PWA d'abord.
- Fédération entre serveurs : chaque instance est indépendante en v1 (§7.5).
- Miroir des espaces en fichiers `.md` synchronisés en continu avec Obsidian ou git. **Décision de Lenny** : Mementum est autonome, on passe d'un outil à l'autre par l'import et l'export, pas par une synchro permanente.
- Agents qui pilotent un ordinateur ou un navigateur, marketplace de templates, sites avec domaine personnalisé avancé, tableau blanc.

### Anti-objectifs (échec même si « ça marche »)
- **Open-core** : une fonction d'équipe (sièges, SSO, permissions, MCP, historique) qui deviendrait payante.
- **Dépendance cloud** : l'instance doit fonctionner sans connexion internet (air-gapped), IA locale comprise.
- **Pile lourde** : plus d'un conteneur requis pour la configuration de base.
- **IA invisible** : un agent qui modifie sans trace, sans attribution ou sans possibilité d'annuler.
- **Télémétrie activée par défaut.**
- **Format prisonnier** : du contenu impossible à exporter sans perte.
- **Fonction réservée au cloud** : tout ce que fait Mementum Cloud (§7.6) doit être possible gratuitement en auto-hébergement. Le cloud vend l'hébergement, pas des fonctions.

---

## 3. Contraintes et principes

### 3.1 Licence et gouvernance *(AGPL choisie par Lenny le 09/11/2026)*
- **Serveur et application web : AGPL-3.0.** Elle empêche qu'un hébergeur ferme le code tout en restant une licence OSI. Elle est compatible avec les paquets GPL-3.0 de BlockNote si on en a besoin.
- **SDK, spécification du format MMD, bibliothèques clientes : Apache-2.0**, pour que n'importe quel outil ou agent puisse s'intégrer sans contrainte.
- **DCO (Developer Certificate of Origin), pas de CLA.** Sans CLA, personne ne peut relicencier le projet en propriétaire. C'est une promesse irrévocable, et c'est exactement ce qui a manqué chez AppFlowy, NocoDB et Plane.
- **Modèle économique** *(décision de Lenny)* : l'auto-hébergement est **entièrement gratuit, avec toutes les fonctionnalités et sans limite**. Le projet se finance par **Mementum Cloud** (§7.6), une offre hébergée payante pour ceux qui ne veulent pas gérer de serveur. Aucune fonction n'est réservée à une édition payante.
- Conséquence de l'AGPL, assumée : d'autres hébergeurs peuvent aussi proposer Mementum en service payant, mais ils doivent publier toutes leurs modifications. L'avantage de Mementum Cloud tient à la qualité du service et au fait d'être l'équipe qui fait le logiciel.

### 3.2 Matériel cible
- Minimum : Raspberry Pi 4 (2 Go) ou VPS à 1 vCPU / 1 Go pour une petite équipe.
- Le même binaire passe à l'échelle d'une organisation de milliers de personnes avec Postgres et S3.

### 3.3 Principes d'architecture
1. **L'interface lit d'abord en local** (cache SQLite dans le navigateur), le réseau ne sert qu'à synchroniser.
2. **Le serveur fait autorité** sur la structure, les permissions et les bases. Le CRDT sert au texte riche des pages.
3. **Un seul format pour les humains et les agents.** Le MMD est la représentation texte officielle, et son convertisseur est le même code (Rust → WASM) côté serveur et côté client.
4. **Tout changement est une opération journalisée**, ce qui donne la synchro, l'historique, l'audit, l'annulation et les webhooks.
5. **Les permissions sont vérifiées par le serveur à chaque opération**, que la requête vienne d'un humain, d'une API ou d'un agent.
6. **Une seule couche d'outils** : l'IA intégrée et les agents externes (MCP) passent par les mêmes outils, donc mêmes droits, même audit, même fusion.

---

## 4. Le produit

### 4.1 Hiérarchie

```
Instance (ton serveur)
└── Espace (ex. « Perso », « Pro », « Famille », « Asso »)  ← plusieurs par instance, plusieurs par compte
    ├── Membres, groupes, invités, agents, jetons d'API
    └── Dossiers et pages (arbre), en trois sections : « Espace », « Partagé », « Privé » (§4.4)
        ├── Blocs (texte, titres, listes, toggles, callouts, code…)
        ├── Sous-pages
        └── Bases de données (pleine page ou en ligne)
            ├── Sources de données (schéma + lignes ; une base peut en avoir plusieurs)
            ├── Vues (tableau, kanban, calendrier…)
            └── Lignes = pages (propriétés + contenu)
```
L'**espace** est le « workspace » de Notion : c'est l'unité de rangement, de partage et d'accès par API (§4.4). Il remplace l'organisation et les coffres des premières versions de ce document.

- **Admin d'instance** : gère les inscriptions (ouvertes, sur invitation, par domaine e-mail), la création d'espaces (ouverte à tous les comptes ou réservée), les quotas, les fournisseurs IA autorisés, les sauvegardes et le SMTP. Au lot 2 de la P1, c'est le premier compte créé ; ce titre ne donne encore aucun pouvoir dans l'application, et les inscriptions se règlent par la configuration du serveur (§7.2).
- **Rôles d'espace** : Propriétaire, Admin, Membre, Membre restreint, Invité. Les agents forment une catégorie à part (§6.7). Au lot 2 de la P1 : `owner`, `admin`, `member` et `guest` ; les membres restreints viennent en P3.
- **Niveaux de permission sur une page** : Accès complet · Peut modifier · Peut modifier le contenu (bases) · Peut créer (bases) · Peut commenter · Peut lire. Au lot 2 de la P1 : `read` < `comment` < `edit` < `full` ; `comment` donne les droits de `read` jusqu'aux commentaires (lot 3). Ce que chaque niveau permet (Ruling R48) : `edit` pour écrire, renommer, créer une sous-page, changer de place parmi les mêmes frères, mettre à la corbeille et restaurer ; `full` pour changer de page parente, voir et changer les accès, inviter sur la page et supprimer pour de bon.
- **Accès général** : seulement les invités · tous les membres de l'espace · toute personne ayant le lien (avec expiration et mot de passe en option) · publié sur le web.

### 4.2 Écrans principaux
- **Barre latérale** : sélecteur d'espace (ses espaces, ceux où on l'a invité, « Créer un espace »), recherche, Accueil, Boîte de réception, **Agents** (exécutions en cours, suggestions en attente), **Vue graphe**, Favoris, puis les trois sections de l'espace, **Espace**, **Partagé** et **Privé**, chacune avec son arbre de pages, Modèles, Corbeille, Réglages. Le menu du compte (nom, mot de passe, déconnexion) est dans le sélecteur d'espace, en haut à gauche, comme dans Notion (Ruling R80, à confirmer). Au lot 2 de la P1, la barre porte, de haut en bas : le sélecteur d'espace, « Nouvelle page », les trois sections et « Corbeille » ; les réglages de l'espace s'ouvrent depuis le sélecteur. Le reste vient avec son lot. Une page se déplace à la souris, ou par le menu « … » de sa ligne pour qui travaille au clavier (Ruling R95). Une adresse d'espace où l'on n'a pas de rôle montre « Espace introuvable » (Ruling R96). La barre est relue quand l'onglet revient au premier plan (Ruling R97, §9), et le compte avec elle : un rôle changé ailleurs y arrive alors (Ruling R126). Un écran dont la lecture a échoué offre « Réessayer », et une erreur d'affichage montre un écran « Recharger » plutôt qu'une page blanche (Ruling R103).
- **Page** : couverture, icône, titre, propriétés si c'est une ligne de base, éditeur en blocs, rétroliens, commentaires dans la marge, avatars des présents (agents compris, avec un badge « agent »).
- **Base de données** : onglets de vues, filtres, tris et groupes, recherche interne, ouverture des lignes en panneau latéral, au centre ou en pleine page.
- **Palette de commandes** (Ctrl+K) : navigation, actions, et questions à l'IA avec citations.
- **Revue des suggestions** : comparaison par exécution d'agent, avec accepter ou refuser bloc par bloc ou en bloc.
- **Historique** : frise des versions avec attribution humain ou agent, et restauration en un clic.
- **Réglages** : espace (membres, groupes, sécurité, IA, agents, jetons d'API, journal d'audit), compte (profil, jetons personnels) et instance (admin).

### 4.3 Direction visuelle retenue : A « Papier » *(choix de Lenny)*
Maquettes : canvas interactif https://claude.ai/artifact/5VPJk8gPwX1cMdwufLHTXw (artboard « A · Papier »). Le principe est de **ne pas dépayser quelqu'un qui vient de Notion** : mêmes emplacements, mêmes gestes, identité propre.
- **Mise en page** :
  - barre latérale claire (fond `#F6F5F1`, bordure `#E8E6E1`, éléments de 30–32 px) ;
  - page blanche centrée (≈ 760 px max), bandeau de couverture et icône de page en tuile, titre en 38 px gras ;
  - propriétés en liste « libellé / valeur » sous le titre.
- **Typographie et couleurs** :
  - **Inter** partout ;
  - texte `#2F2E2B`, texte secondaire `#66655F` (contraste ≥ 4,5:1) ;
  - **accent vert `#23704F`**, réglable par espace ;
  - pastilles de statut en couleurs douces.
- **Barre du haut** : fil d'Ariane, « Modifié il y a… », avatars des présents (l'agent a une pastille ✦ à la couleur d'accent), bouton **Partager**, menu « … ». Au lot 2 de la P1, elle porte le titre de la page, l'état de la connexion quand il mérite d'être dit (« Hors ligne », après 1,5 s de coupure) et le bouton Partager ; le fil d'Ariane, « Modifié il y a… », les avatars et le menu « … » restent à faire.
- **Menu Partager** (déroulant en haut à droite, comme Notion), de haut en bas :
  1. onglets Partager / Publier ;
  2. champ d'invitation (personnes, groupes, agents) ;
  3. liste des accès avec leur niveau (l'agent porte un badge « Agent » et le nom de son parrain) ;
  4. **Accès général** en 3 choix : seulement les invités / tous les membres de l'espace / toute personne ayant le lien ;
  5. « Copier le lien ».

  Le partage porte toujours sur la page ouverte et ses sous-pages. Les maquettes montrent encore la bascule « Tout le coffre / Cette page seulement » de la première version : elle disparaît avec les coffres (§4.4).

  **Tel qu'il est livré au lot 2 de la P1.** Le bouton « Partager » est en haut à droite de la page, pour qui a l'accès complet sur elle. Il n'est pas montré sur une page à la corbeille. Le menu s'ouvre sous le bouton, sans onglets : Publier vient en P3. De haut en bas :
  1. le champ d'invitation : une adresse e-mail et un niveau (« Peut modifier » est proposé d'abord). L'invitation est un lien, à copier et à envoyer soi-même (§4.4). Il reste montré tant que l'invitation attend et que la page est ouverte, puis ne l'est plus jamais : le serveur n'en garde que le hachage (Ruling R114) ;
  2. les personnes qui ont un accès à leur nom, avec leur niveau, qui se change sur place, et « Retirer », qui demande confirmation. Un accès posé sur une page parente est marqué « hérité » : il se change sur elle ;
  3. les invitations de la page qui attendent, avec « Révoquer » ;
  4. l'**accès général**, en deux choix : « Seulement les invités », ou « Tous les membres de l'espace » à un niveau (« Peut lire » est proposé d'abord). « Toute personne ayant le lien » vient en P3, avec les liens publics. L'accès général qu'une page parente donne est un plancher : sous lui, « Seulement les invités » et les niveaux plus bas sont grisés (Rulings R93 et R110). Cocher « Seulement les invités » sur une page ouverte aux membres la retire à tous les autres : une fenêtre le fait confirmer, « Fermer cette page aux membres ? » (Ruling R127). Qui confirme reste sur la page, nommé en « Accès complet » : la page passe dans son « Privé », ou dans « Partagé » s'il reste d'autres personnes nommées (Ruling R122) ;
  5. « Copier le lien » de la page.

  Les accès d'une page ne se voient et ne se changent qu'avec l'accès complet sur elle. Un invité en accès complet y fait tout, sauf changer l'accès général, qu'il lit sans le régler : ce que voient les membres de l'espace, c'est aux membres d'en décider (Ruling R85). Le serveur fait foi : un refus s'affiche tel qu'il le dit, et le menu revient à l'état du serveur.

  Chez qui a la page ouverte, un droit qui change s'applique en direct : le serveur ferme les connexions de synchro de la page, et le client relit la page avant de se reconnecter (§5.6). Un niveau qui baisse sous « Peut modifier » verrouille aussitôt le titre et le texte, sous un bandeau « Tu peux lire cette page, pas la modifier. ». Un niveau qui monte ne remplace pas l'éditeur sous les yeux de qui lit : un bandeau propose « Rouvrir la page » (Ruling R111). Un accès retiré ferme la page sur « Tu n'as plus accès à cette page ».

  Quand la session se perd, la connexion est demandée par-dessus la page, qui reste ouverte avec ce qu'on y a tapé. Dès que le compte de la session n'est plus celui que l'application a chargé, l'onglet se recharge : le texte de l'un ne part pas sous le nom de l'autre (Ruling R115). L'application l'apprend quand un autre compte se connecte par-dessus la page, et, s'il s'est connecté dans un autre onglet du même navigateur, au retour sur l'onglet, où le compte est relu (Ruling R126).
- **Collaboration visible** : curseur nommé de chaque personne. Une suggestion d'agent apparaît surlignée dans le texte, avec une petite barre « Scribe (agent) propose un ajout · Accepter · Refuser ». Après décision : « Suggestion acceptée · Annuler ».
- **Bases** : en-tête avec onglets de vues (Tableau, Kanban, Calendrier) et bouton « Nouveau » à la couleur d'accent ; tableau sobre à lignes fines.
- **Thème sombre** : dérivé du même système, sans changer la mise en page (au lot 2 de la P1, seul le thème clair existe). Les directions B (« Atelier ») et C (« Nuit ») restent des sources d'idées : panneau d'activité des agents, barre IA flottante Ctrl+K.

### 4.4 Espaces façon Notion *(demande d'Alex du 2026-11-09 ; remplace les « coffres façon Obsidian »)*
La première version de ce document rangeait le contenu dans des coffres à la manière d'Obsidian, à l'intérieur d'une organisation (idée de Lenny). Alex a demandé le fonctionnement de Notion : « on se fait des spaces », par exemple un espace « Perso » et un espace « Pro » ; on demande à Claude Code de verser tel contenu dans son espace « Pro » ; on invite des gens dans un espace ou sur une page de l'espace ; on ouvre des accès d'API sur un espace ou sur des pages. Ce qui suit traduit cette demande. Les points marqués *(choix de rédaction)* ne viennent pas de lui : ils sont à relire (§12).

- **Un compte, plusieurs espaces.**
  - Chacun crée autant d'espaces qu'il veut (« Perso », « Pro », « Asso »…). Le sélecteur de la barre latérale montre ceux qu'il a créés et ceux où on l'a invité.
  - Un espace est cloisonné : ses pages, ses membres, ses agents, ses jetons d'API et ses réglages ne se voient pas depuis un autre espace.
  - Le premier espace est créé à l'inscription, sauf quand on arrive par une invitation *(choix de rédaction)*. L'admin d'instance peut réserver la création d'espaces à certains comptes *(choix de rédaction ; pas encore fait au lot 2 de la P1, Ruling R52)*. Au lot 2, ce premier espace s'appelle « Espace de » suivi du nom de la personne ; sur une base du lot 1, le premier compte de l'instance reprend à la place son espace, « Mon espace ».
- **Trois sections dans un espace**, comme dans la barre latérale de Notion :
  - **Espace** : les pages que voient tous les membres de l'espace ;
  - **Partagé** : les pages partagées avec des personnes précises ;
  - **Privé** : les pages que seul leur auteur voit.

  La section d'une page se déduit de ses accès : il n'y a rien à ranger à la main. Règles du lot 2 de la P1. Elles se calculent pour la plus haute page que la personne voit ; ses sous-pages suivent.
  - « Espace » : la personne est membre, et un accès général s'applique à la page ou à une page parente.
  - « Privé » : la personne est membre, et aucun accès de la page ni de ses pages parentes ne nomme quelqu'un d'autre qu'elle.
  - « Partagé » : tout le reste.

  Partager une page privée avec une personne la fait donc passer dans « Partagé ». Et un invité ne voit que « Partagé », même pour une page où il est seul (Ruling R57). Le propriétaire et les admins de l'espace, comme l'admin d'instance, ne voient pas les pages privées des autres. Celles d'un membre qui part restent en base, invisibles, et il les retrouve s'il revient (Ruling R49).
- **Inviter dans l'espace** : la personne devient **membre**. Elle voit la section « Espace », y crée des pages et a ses propres pages privées. Rôles : Propriétaire, Admin, Membre, Membre restreint (§4.1). Au lot 2 de la P1, une page créée à la racine de la section « Espace » donne l'accès complet à tous les membres, comme dans Notion (Ruling R48, à confirmer : passer à « Peut modifier » est un changement d'une ligne) ; le propriétaire et les admins ont l'accès complet partout où les membres voient. Un espace garde toujours au moins un propriétaire.
- **Inviter sur une page** : la personne devient **invitée** de l'espace. Elle ne voit que les pages partagées avec elle, sous-pages comprises, au niveau qu'on lui donne. Une personne qui a déjà un rôle dans l'espace le garde : une invitation ne fait jamais baisser ce qu'on a.
- **Invitations sans SMTP** (lot 2 de la P1, Ruling R47) : une invitation est un lien à copier, que l'on envoie soi-même. Il sert une fois, vaut 7 jours et se révoque tant qu'il attend. C'est le jeton du lien qui donne l'accès, jamais l'adresse e-mail, qui n'est pas vérifiée. Un lien ne vaut que tant que son auteur a encore le droit de le faire : être propriétaire ou admin de l'espace pour une invitation dans l'espace, avoir l'accès complet sur la page pour une invitation sur une page (Ruling R69).
- **Partage d'une page** (menu Partager, §4.3) :
  - avec des **personnes**, des **groupes** ou des **agents** ; niveaux : Accès complet · Peut modifier · Peut commenter · Peut lire, et pour les agents `suggérer` ;
  - les sous-pages héritent de ces accès ; une sous-page peut être plus ouverte que son parent et, à partir de la P3, plus fermée ;
  - **accès général** : seulement les invités · tous les membres de l'espace · toute personne ayant le lien · publié sur le web.

  Précisions du lot 2 de la P1 (personnes seulement : groupes et liens en P3, agents en P4) :
  - l'accès général se règle par les membres de l'espace, jamais par un invité, même en accès complet (Ruling R85) ;
  - un invité en accès complet sur une page peut y inviter d'autres personnes, jusqu'à l'accès complet, et changer ou retirer leurs accès, comme dans Notion (Ruling R86, à confirmer : une option d'espace « les invités n'invitent pas » se poserait en une ligne) ;
  - l'accès général qu'une page parente donne aux membres est le plancher de ses sous-pages : on peut l'y relever, pas le baisser ni le retirer ; ramené au plancher, l'accès propre de la sous-page disparaît (Ruling R93). Dans le menu Partager, « Seulement les invités » est donc grisé dès qu'une page parente pose un plancher (Ruling R110) ;
  - qui restreint l'accès général d'une page y garde l'accès complet : si elle le perdait, une ligne à son nom est posée dans la même transaction. Une page d'équipe peut ainsi redevenir privée, et n'est alors plus vue des autres membres, propriétaires et admins compris (Ruling R122). Sous un plancher, quand l'accès propre d'une sous-page disparaît, un simple membre qui retomberait au niveau du plancher y reçoit de même une ligne en accès complet ; un propriétaire ou un admin, non. Le refus « plus personne en accès complet » ne vient donc plus que d'un accès nommé qu'on retire ou qu'on baisse ;
  - une page garde toujours quelqu'un en accès complet. Un changement qui la laisserait sans personne est refusé, et quand un membre quitte l'espace, sur chaque page dont il était le dernier en accès complet, les autres accès posés sur la page qui ont le plus haut niveau passent à l'accès complet (Ruling R70) ;
  - une page qu'on sort à la racine garde son accès général : une sous-page d'équipe reste dans la section « Espace ». Les personnes nommées sur son ancienne page parente la perdent, comme dans Notion. Qui la déplace y garde l'accès complet (Rulings R82 et R94, R94 à confirmer).
- **API et agents, sur un espace ou sur des pages** (§6.5, §6.7) :
  - un **jeton personnel** se crée dans le compte. Il agit au nom de son propriétaire, avec ses droits, et on le limite si on veut : tous ses espaces, un seul espace, ou des pages choisies avec leurs sous-pages ;
  - un **jeton d'intégration** se crée dans les réglages d'un espace. Il appartient à un agent de cet espace et n'ouvre que ce qu'on partage avec cet agent : l'espace entier, ou des pages choisies avec leurs sous-pages ;
  - un agent comme Claude Code désigne l'espace par son nom : « verse ce compte rendu dans mon espace Pro ».
- **Ce qui reste de l'idée de Lenny** : les conventions d'Obsidian, sans les coffres :
  - liens `[[Page]]` et `[[Page#Titre]]`, **rétroliens**, **vue graphe** de l'espace ;
  - dossiers ;
  - frontmatter YAML = propriétés de la page ;
  - callouts `> [!tip]`, références de bloc `^b4`, tags `#tag`.

  Le format MMD (§6.2) est déjà du Markdown compatible avec ces conventions.
- **Modèle d'espace « LLM wiki »**, prêt à l'emploi, inspiré du vault de Lenny :
  - `raw/` pour les sources jamais modifiées ;
  - pages tenues par l'IA ;
  - `index`, `log` (journal) et page de schéma (`CLAUDE.md`).

  Un agent d'espace s'en occupe : ingestion, réponses aux questions, vérification. Il est en mode suggestion par défaut.
- **Pas de miroir en fichiers** (décision de Lenny) : un espace vit dans Mementum. On arrive avec l'import d'un vault Obsidian (§4.6) et on repart avec l'export. Les agents comme Claude Code passent par le MCP (§6.5) ou la CLI, pas par les fichiers.
- **Plus tard (P3)** : des **espaces d'équipe** (les « teamspaces » de Notion) à l'intérieur d'un espace, pour qu'un groupe de membres ait sa propre section *(choix de rédaction)*.

### 4.5 Inventaire fonctionnel et parité Notion
Phases : **P1** à **P5** (§9) ; **+** = après la 1.0 ; **✘** = hors périmètre.

#### Éditeur et blocs
| Fonctionnalité | Phase |
|---|---|
| Texte, titres H1 à H4, titres repliables, listes à puces, numérotées et à cocher, toggle, citation, séparateur, callout | P1 |
| Code avec coloration (~75 langages, dont Mermaid rendu en diagramme), équation KaTeX (bloc et en ligne) | P1 |
| Image, vidéo, audio, fichier, PDF, signet (aperçu de lien), intégrations oEmbed sur liste blanche | P1 |
| Colonnes, table simple (cellules fusionnées en P5), table des matières, fil d'Ariane, onglets (Tabs) | P1–P5 |
| Mentions : @personne, @page, @date (langage naturel), @rappel ; `[[` pour lier ou créer une page | P1 |
| Commandes `/`, raccourcis Markdown, glisser-déposer, « Transformer en », couleurs, Ctrl+D, Tab/Maj+Tab | P1 |
| Icônes, emojis personnalisés, couvertures, pleine largeur, petite police, verrouillage de page | P1 |
| Blocs synchronisés (même contenu à plusieurs endroits) | P3 |
| Bouton (actions sur la page ou la base), template de bloc | P5 |
| Graphique dans une page, mode présentation, bloc HTML interactif sandboxé | + |
| Notes de réunion avec transcription (Whisper local) | + |

#### Bases de données
| Fonctionnalité | Phase |
|---|---|
| Base pleine page et en ligne, vues liées, **plusieurs sources de données par base** | P2 |
| Propriétés : titre, texte, nombre (45 formats), sélection, multi-sélection, statut (3 groupes), date (plage, rappel), personne, fichiers, case à cocher, URL, e-mail, téléphone, créé le/par, modifié le/par, ID unique avec préfixe | P2 |
| **Relation** (simple ou bidirectionnelle) et **rollup** (les 24 fonctions de Notion) | P2 |
| **Formules** : moteur compatible Formulas 2.0 (opérateurs, `if/ifs/let/lets`, texte, regex, maths, dates, listes avec `map/filter/find/some/every`, `style`, `link`) ; sous-ensemble en P2, complet en P5 | P2–P5 |
| Bouton, vérification, lieu (avec la vue carte) | P5 |
| Vues tableau, kanban, liste, galerie, calendrier | P2 |
| Vues frise (avec dépendances), graphique, fil, carte, formulaire, tableau de bord | P5 |
| Filtres simples et avancés (groupes ET/OU sur 3 niveaux), tris multiples, groupes et sous-groupes, calculs en pied de colonne, couleurs conditionnelles | P2 |
| Sous-éléments, dépendances, templates de lignes, templates récurrents | P2–P5 |
| Import CSV (typage auto, fusion dans une base existante) | P2 |
| Automatisations de base (déclencheur → action), automatisations planifiées, webhooks | P5 |
| Permissions par ligne (règle sur une propriété Personne ou Créé par) | P5 |
| Permissions par colonne, bases synchronisées depuis des API externes (GitHub, Jira…) | + |

#### Collaboration et organisation
| Fonctionnalité | Phase |
|---|---|
| Édition simultanée, curseurs, présence, « qui regarde » | P1 |
| Commentaires de page et ancrés sur un bloc, réactions, résolution | P1 |
| Corbeille, restauration, duplication, déplacement, favoris, récents | P1 |
| Historique de versions (instantanés + attribution + restauration) | P1 (simple) → P3 (frise complète) |
| Espaces (plusieurs par compte), membres invités dans l'espace, pages privées et pages d'espace, partage d'une page avec des personnes (invités compris), `[[liens]]` et rétroliens, comptes locaux | P1 |
| Groupes, espaces d'équipe à l'intérieur d'un espace, sous-page plus fermée que son parent, membres restreints, vue graphe | P3 |
| Partage fin, liens publics (expiration, mot de passe), demandes d'accès | P3 |
| **Suggestions de modification** (suivi des changements) pour humains et agents | P3 |
| Boîte de réception, notifications groupées, mentions, mise en sourdine | P3 |
| Publication web (thème, SEO, `llms.txt`), pages archivées, pages vérifiées | P3–P5 |
| SSO **OIDC et SAML gratuits**, passkeys, TOTP, journal d'audit, panneau d'admin | P3 |
| Analytique de page (vues, opt-out, aucune donnée externe) | P5 |
| SCIM, rétention personnalisée, legal hold | + |

#### Import, export, intégrations
| Fonctionnalité | Phase |
|---|---|
| Export `.mementum` sans perte (MMD + JSON + fichiers) et réimport | P1 |
| Import d'un **vault Obsidian** complet et de dossiers Markdown (§4.6) | P1 |
| Import **Notion** : pages, arborescence, fichiers, mentions (§4.6) | P1 |
| Import **Notion** : bases complètes (propriétés, relations, rollups, formules, vues) | P2 |
| Export Markdown + CSV, HTML, PDF | P2–P3 |
| Import Confluence | + |
| API REST + OpenAPI, webhooks signés HMAC, flux d'événements SSE | P2–P5 |
| MCP intégré (§6.5), **dès la première version** (décision de Lenny) | P1 (base) → P4 (complet) |
| Jetons d'API (§4.4) : jetons personnels, limitables à un espace ou à des pages ; jetons d'intégration portés par un agent d'espace | P1 (personnels) → P4 (intégration) |

### 4.6 Importer depuis Notion et Obsidian *(exigence de Lenny)*
Mementum est autonome : il n'y a **pas de synchro permanente** avec un autre outil. On importe une fois, proprement, puis on travaille dans Mementum.

**Chaque import :**
- crée un **nouvel espace** (ou remplit un espace choisi) ;
- produit un **rapport d'import** qui liste ce qui a été converti, approximé ou ignoré, avec un lien vers chaque page concernée ;
- est **rejouable** : réimporter met à jour les pages déjà importées (l'identifiant d'origine est gardé) au lieu de créer des doublons, ce qui permet une bascule progressive ;
- tourne en tâche de fond avec une barre de progression. On le lance depuis l'interface (glisser-déposer d'un zip ou d'un dossier) ou depuis la CLI : `mementum import obsidian <dossier>`, `mementum import notion <zip>` ou `mementum import notion --token`.

**Vault Obsidian** (dossier ou zip) :

| Dans Obsidian | Dans Mementum |
|---|---|
| Dossiers | Dossiers de l'espace |
| Notes `.md` | Pages (converties en MMD) |
| Frontmatter YAML | Propriétés de la page (type deviné : date, nombre, liste, case à cocher, texte) |
| `[[Note]]`, `[[Note\|alias]]`, `[[Note#Titre]]`, `[[Note#^bloc]]` | Liens vers la page, le titre ou le bloc, résolus comme Obsidian : par nom unique, sinon par chemin |
| `![[image.png]]`, `![[Note]]` | Image ou fichier ; une note intégrée devient un bloc synchronisé (un simple lien en cas de boucle) |
| Callouts `> [!tip]`, `> [!warning]`… | Callouts (le type donne l'icône et la couleur) |
| `#tags` (dans le texte ou le frontmatter) | Propriété « Tags » en multi-sélection, plus la recherche |
| Références de bloc `^id` | Ancres de bloc conservées |
| Pièces jointes | Fichiers de l'espace (dédoublonnés) |
| Liens vers des notes inexistantes | Liens « page à créer », comme dans Obsidian |
| `.obsidian/` (réglages, plugins), requêtes Dataview, fichiers Canvas | Non convertis, mais listés dans le rapport (une requête Dataview peut devenir une vue de base plus tard) |

**Notion**, deux voies :
1. **Par l'API Notion** (recommandé, la plus fidèle). L'utilisateur colle un jeton d'intégration Notion, ou se connecte en OAuth, puis choisit les pages partagées avec l'intégration. On récupère :
   - les pages, les blocs, les mentions et les commentaires ;
   - les bases avec leurs sources et leurs 24 types de propriétés : relations et rollups recâblés, formules recopiées puis recalculées par notre moteur ;
   - les vues principales (filtres, tris, groupes) et les fichiers.

   La limite de Notion (~3 requêtes/s) est respectée grâce à une file d'attente avec reprise après coupure. Le jeton est stocké chiffré et supprimé à la fin de l'import.
2. **Par le zip d'export** (« Markdown & CSV » ou « HTML ») : sans jeton, mais moins fidèle. Notion reconnaît lui-même que son export ne recrée pas tout. On récupère les pages, l'arborescence et les images. Les bases arrivent en CSV : types devinés, relations reconstruites par titre.

**Correspondances Notion → Mementum** :
- un workspace Notion → un espace ; ses teamspaces → des dossiers de la section « Espace » (des espaces d'équipe à partir de la P3). Les accès sont recopiés quand les personnes ont un compte sur l'instance ; sinon, des invitations sont proposées ;
- pages privées → section « Privé » de la personne qui importe ;
- blocs synchronisés → blocs synchronisés ;
- boutons, automatisations et intégrations → listés dans le rapport, à refaire.

**Phases** : en P1, pages, arborescence, fichiers et mentions ; en P2, bases complètes.

---

## 5. Architecture technique

### 5.1 Vue d'ensemble

```
┌──────────────────────────── Navigateur / Tauri ────────────────────────────┐
│  React + TypeScript + Vite                                                  │
│  Éditeur BlockNote (ProseMirror) ⇄ Yjs        Grille virtualisée (bases)   │
│  ── Worker partagé (un seul écrivain) ─────────────────────────────────────│
│  SQLite WASM sur OPFS : pages, lignes, mises à jour Yjs, file d'opérations  │
│  Modules WASM partagés : mementum-md (MMD), mementum-formula, mementum-filter     │
└──────────────────────────────┬─────────────────────────────────────────────┘
                               │ 1 WebSocket multiplexé (MessagePack)
┌──────────────────────────────▼─────────── binaire `mementum` (Rust) ─────────┐
│ Axum + tokio : HTTP, WebSocket, fichiers statiques de l'app embarqués      │
│ ├─ Sync : docs Yjs (yrs) par page + journal d'opérations par espace        │
│ ├─ Domaine : pages, bases, vues, membres, commentaires, notifications      │
│ ├─ Permissions : tuples façon Zanzibar + héritage de l'arbre, cache        │
│ ├─ Moteur de requêtes des bases : filtres → SQL, formules, rollups         │
│ ├─ Recherche : Tantivy (plein texte) + index vectoriel embarqué            │
│ ├─ IA : passerelle LLM multi-fournisseurs, agents, exécutions, budgets     │
│ ├─ MCP (/mcp) · REST (/api/v1) · Webhooks · CLI                            │
│ └─ Tâches de fond : embeddings, exports, automatisations, compaction CRDT  │
└───────┬──────────────────────────┬───────────────────────────┬─────────────┘
   SQLite (WAL, défaut)       Disque local (défaut)      Fournisseurs IA (option)
   ou PostgreSQL              ou S3 (Garage, SeaweedFS,  OpenAI-compatible (Ollama,
   + Litestream (option)      R2, AWS)                   vLLM…), Anthropic, Mistral
```

### 5.2 Choix de la pile *(mes choix, justifiés par la recherche)*

| Couche | Choix | Pourquoi | Écarté |
|---|---|---|---|
| Serveur | **Rust** : Axum, tokio, sqlx | Binaire unique, faible mémoire (Pi), CRDT Rust mûr (yrs, utilisé par AppFlowy ; Zed et AFFiNE ont aussi des serveurs CRDT en Rust). Lenny connaît déjà Rust (Avero, Snoopr) | TypeScript + Hocuspocus (impose Postgres et un runtime plus lourd), Go (pas d'implémentation Yjs/Loro de premier plan), Elixir (distribution en binaire unique difficile) |
| Contenu des pages | **Yjs** (client) + **yrs** (serveur, feature `sync`), derrière une façade `mementum-crdt`. **Confirmé par la P0** (`spike/RESULTS.md`). Côté BlockNote, on passe par le chemin Yjs 13 (`@blocknote/core/yjs`). Le serveur écrit la racine d'une page neuve (sinon deux clients sur une page vide créent deux racines et l'une est perdue) ; cela ne protège que les éditions faites après la première synchro (§11) | Standard de fait (Outline, Docmost, Plane, La Suite Docs, AFFiNE…), liaison éditeur mûre. P0 : Yjs 3 à 16 fois plus rapide que Loro dès qu'on lit toute la page. Passer à Yjs 14 quand yrs le comprendra (yrs 0.28 ne lit pas le texte des nœuds Yjs 14) | Loro (arbre déplaçable sans doublons ni éditions perdues, chargement paresseux plus de 20 fois plus économe, mais 3 à 16 fois plus lent dès qu'on lit toute la page ; plan B via la façade), Automerge |
| Structure et bases | **Journal d'opérations dont le serveur fait autorité** (façon Linear/Figma) : version monotone par espace, application optimiste côté client puis rebase, dernier écrivain gagnant par propriété | Permissions, requêtes serveur sur 100 000 lignes, formules : impossible proprement en tout-CRDT. Aucun moteur existant n'accepte SQLite par défaut (Zero, Electric : Postgres uniquement ; PowerSync : FSL) | Zero, ElectricSQL, PowerSync, Triplit (à l'arrêt), tout-CRDT façon Anytype |
| Éditeur | **BlockNote** (cœur MPL-2.0) sur ProseMirror/Tiptap | Expérience Notion prête (menu `/`, poignées, imbrication) ; choisi par La Suite Docs (État français). Adaptations P0 : types custom (callout, equation, bookmark, pageLink, databaseRef, syncedBlock, toc, unknown, contenus en ligne mention et math), multi-colonnes via `@blocknote/xl-multi-column` (GPL-3.0). y-prosemirror supprime du document partagé les nœuds qu'il ne connaît pas : versionnage de schéma obligatoire (§11) | Tiptap nu (tout à construire), Lexical, BlockSuite (trop couplé à AFFiNE) |
| Base serveur | **SQLite en WAL** par défaut, **PostgreSQL** en option, même schéma via sqlx | Zéro service à installer, sauvegarde continue avec Litestream (v0.5). Postgres pour les grosses instances | Postgres obligatoire |
| Cache client | **SQLite WASM sur OPFS** dans un worker partagé (un seul écrivain) | Technique de Notion : navigation 20 % plus rapide, hors ligne | IndexedDB brut |
| Permissions | **Tuples façon Zanzibar intégrés** à la base + héritage de l'arbre | Pas de service externe ; OpenFGA possible plus tard pour les très grosses organisations | SpiceDB (mode embarqué limité aux vérifications) |
| Recherche | **Tantivy** (plein texte, Rust, embarqué) + **index vectoriel embarqué**, fusion RRF | Même comportement sur SQLite et Postgres, rapide, sans service ; index reconstructibles depuis la base | Meilisearch/Elastic (services en plus), sqlite-vec (encore en 0.1) |
| Fichiers | **Disque local adressé par contenu**, S3 compatible en option | Simple ; S3 pour passer à l'échelle | MinIO CE (archivé début 2026) |
| Bureau / mobile | **Tauri** (P5) / **PWA** | Même code web ; Tauri mobile plus tard | Electron (lourd), Flutter (deux bases de code) |

### 5.3 Modèle de données (simplifié)

```text
instance_settings(key, value)
users(id, email, name, avatar, password_hash?, created_at)            -- comptes de l'instance
passkeys / totp / sessions / oauth_identities
spaces(id, slug, name, icon, settings_json, created_by, created_at)   -- l'espace (§4.4)
members(space_id, user_id, role, status)                             -- owner|admin|member|restricted|guest
groups(id, space_id, name) · group_members(group_id, user_id)
agents(id, space_id, name, avatar, sponsor_user_id, policy_json)     -- §6.7
invitations(id, space_id, node_id?, email, grant, token_hash,
            expires_at)                                               -- dans l'espace, ou sur une page (§4.4)

nodes(id, space_id, parent_id, kind, title, icon, cover,
      position, created_by, created_at, updated_by, updated_at,
      archived_at, trashed_at, locked)                               -- kind: folder|page|database|row
      -- position = index fractionnaire ; déplacement vérifié contre les cycles

doc_updates(node_id, seq, update_blob, actor_id, run_id?, created_at) -- mises à jour Yjs (journal)
doc_snapshots(node_id, seq, state_blob, created_at)                   -- compaction régulière
blocks_index(block_id, node_id, type, text, path, hash)               -- projection du doc Yjs :
                                                                      -- recherche, rétroliens, ancres agents

databases(node_id, settings_json)
data_sources(id, database_id, name, schema_json)                      -- propriétés typées
rows(node_id, data_source_id, props_json, computed_json)              -- valeurs + formules/rollups calculés
relations(from_row, prop_id, to_row)                                  -- arêtes de relations (rollups rapides)
views(id, database_id, data_source_id, kind, config_json, shared)     -- filtres/tris/groupes/visibilité

acl(object_id, relation, subject)                                     -- ex. (page:42, can_edit, group:7)
share_links(id, node_id, level, expires_at, password_hash?)
comments(id, node_id, block_id?, thread_id, author_id, body_mmd, resolved_at)
suggestions(id, node_id, run_id?, author_id, status)                  -- §6.4
notifications(id, user_id, kind, payload_json, read_at)
ops_log(space_id, lsn, actor_id, run_id?, op_json, created_at)        -- tout changement structurel
agent_runs(id, agent_id, on_behalf_of?, trigger, model, status, cost, started_at, ended_at)
audit_log(id, space_id, actor_id, action, object, details_json, created_at)
files(hash, size, mime, storage_key) · attachments(node_id, file_hash)
api_tokens(id, user_or_agent_id, space_id?, scopes, expires_at, hash) -- portée : un espace (ou tous), ou des pages (acl)
automations(id, database_id, trigger_json, actions_json) · webhooks(id, space_id, url, events, secret_hash)
jobs(id, kind, payload_json, status, run_at, attempts)                -- file de tâches intégrée
```

Identifiants : **UUIDv7** (triables par date). Côté utilisateur et agent, on montre des alias courts (`pg_7Hq2Lm`), traduits par le serveur.

### 5.4 Synchronisation temps réel (hybride)
Le client ouvre **une seule WebSocket**, multiplexée, encodée en MessagePack :

| Canal | Contenu | Mécanisme |
|---|---|---|
| `doc:<page>` | Contenu riche de la page | Synchro Yjs (étapes 1 et 2), puis mises à jour incrémentales. *Awareness* pour les curseurs et la sélection |
| `space:<id>` | Arbre des pages, métadonnées, lignes de base, commentaires, notifications | **Pull** : « donne-moi tout depuis la version N », filtré par permissions. **Push** : diffusion en direct des nouvelles opérations |
| `mutate` | Écritures structurelles | Le client envoie `{client_mutation_id, name, args}`. Le serveur valide (permissions, schéma), applique, attribue un numéro de version et répond. L'opération est idempotente |
| `query:<view>` | Fenêtre d'une vue de base (lignes 0 à 200…) | Requête serveur, abonnement aux changements de la fenêtre |

- **Hors ligne** : les mutations restent en file dans SQLite (OPFS) et sont rejouées au retour. Pour les propriétés de base, le dernier écrivain gagne, propriété par propriété. Le texte fusionne via Yjs.
- **Chargement paresseux** : l'arbre se charge niveau par niveau ; une page est d'abord rendue depuis le cache, puis synchronisée ; les lignes de base arrivent par fenêtres.
- **Compaction** : un instantané Yjs est pris toutes les N mises à jour, puis le journal est purgé. Les instantanés d'historique sont conservés (§4.5).

### 5.5 Bases de données : stockage, requêtes, formules
- **Stockage** *(mon choix, confirmé par la P0 sur PC)* : `rows.props_json` (JSON typé selon le schéma), avec des **index d'expression** créés automatiquement sur les propriétés filtrées ou triées le plus souvent (SQLite JSON1 / Postgres JSONB + GIN). Les relations vont dans une table d'arêtes.
  - Mesures P0 sur 100 000 lignes (PC) : fenêtre triée 2,5 ms, filtre ET/OU + tri 127 ms, groupes 13 ms.
  - Les requêtes doivent reprendre **l'expression exacte de l'index** (un `CAST` faisait passer les groupes de 13 à 707 ms).
  - La table SQL typée par source gagne environ 20 % sur les filtres, 2 fois sur les groupes et 3 fois sur les rollups à la volée : elle reste le **repli** si les mesures ARM échouent.
  - **Mesures ARM encore à faire.**
- **Rollups et formules : toujours matérialisés** (obligatoire depuis la P0 pour les rollups ; les formules, non mesurées, suivent par prudence). Un rollup calculé à la lecture prend 1,9 s sur 100 000 lignes en JSON et 0,6 s en table typée. Les valeurs vivent dans `computed_json` et sont recalculées par le graphe de dépendances décrit plus bas.
- **Moteur de requêtes** (`mementum-query`) : filtre (même JSON que l'interface) → SQL paramétré. Tris, groupes et pagination par curseur côté serveur. Pour une base petite et en cache, le même moteur tourne en WASM dans le client et répond hors ligne.
- **Formules** (`mementum-formula`) : analyseur, typage, puis évaluation compilée en bytecode. Le même code Rust tourne sur le serveur et en WASM dans le client, pour des résultats identiques partout. Les valeurs calculées sont stockées dans `computed_json` et invalidées par graphe de dépendances (propriété → formule → rollup → ligne liée).
- **SQL en lecture seule pour les agents** : chaque source de données est exposée comme une table virtuelle (`SELECT … FROM "Tâches"`), avec les permissions appliquées (lignes et colonnes filtrées). Notion propose la même chose avec du SQLite en lecture seule.

### 5.6 Permissions
- **Sujets** : utilisateur, groupe, invité, agent, jeton d'API, lien public, « tous les membres de l'espace ».
- **Objets** : espace → dossier/page/base → source → ligne (→ colonne, plus tard).
- **Héritage** : une sous-page hérite de son parent, sauf surcharge. Le droit le plus large l'emporte. Les règles par ligne sont évaluées dans la requête SQL (clause ajoutée).
- **Vérification** : `check(sujet, action, objet)` en Rust, avec un cache invalidé par les opérations ACL. Toute diffusion WebSocket est filtrée : personne ne reçoit une opération sur un objet qu'il ne peut pas lire.
- **Agents** : droits effectifs = leurs droits ∩ droits de l'humain pour qui ils agissent (s'il y en a un) ∩ portée du jeton (§6.7).
- **Cloisonnement des espaces** : un sujet n'a aucun droit dans un espace dont il n'est ni membre ni invité. L'admin d'instance gère le serveur (comptes, quotas, liste et suppression des espaces) ; par l'application, il ne lit pas les pages d'un espace dont il n'est pas membre *(choix de rédaction, §12)*.

**Ce qu'en fait le lot 2 de la P1** (crate `mementum-acl`, et `crates/mementum-server/src/access.rs`, seul chemin par lequel une route apprend les droits) :
- **Sujets et calcul.** Une ligne d'accès donne un niveau (§4.1) à un compte ou à « tous les membres de l'espace ». Le niveau d'une personne sur une page est le plus fort de ceux que lui donnent la page et ses pages parentes ; rien ne retire un droit hérité avant la P3. Sur une page que les membres voient, le propriétaire et les admins ont l'accès complet. Qui n'a aucun rôle dans un espace n'y a aucun droit, admin d'instance compris.
- **Réponses.** Une page qu'on ne voit pas répond `404 not_found`, exactement comme une page qui n'existe pas, sur toutes les routes, synchronisation comprise ; `403 forbidden` ne sert que sous le niveau, sur une page qu'on voit ; `409 trashed` refuse une écriture dans une page à la corbeille. Deux autres refus disent un conflit, pas une panne : `409 page_moved` quand une page a changé de place, ou ce qu'on peut en faire, à chaque tentative d'un déplacement, sans que rien soit écrit (Ruling R91) ; `409 inherited_access` quand on veut baisser ou retirer l'accès général qu'une page parente donne (Ruling R93). Un changement de l'accès général qui passe répond toujours les accès de la page, où figure qui l'a demandé (Ruling R122). Toute réponse de l'API, refus compris, porte `Cache-Control: no-store` et `X-Content-Type-Options: nosniff`.
- **Ce que le modèle laisse deviner**, sans jamais donner l'id ni le titre d'une page invisible. Cela concerne qui voit une sous-page sans voir sa page parente, un invité le plus souvent :
  - `PATCH parentId: null` répond `200` si la page est vraiment à la racine, et peut répondre `403` si elle a une page parente cachée ;
  - un `afterId` qui nomme une autre page vue à la racine est accepté si les deux ont la même page parente cachée, refusé sinon ;
  - le `trashedAt` d'une page à la corbeille par héritage donne la date de cette page parente ;
  - les positions des pages de racine sont communes à tout l'espace : leurs écarts laissent voir que d'autres y ont des pages.
- **Cache.** `spaces.rev` est incrémenté, dans la même transaction, par toute écriture qui change une entrée du calcul des accès : création et suppression d'une page, changement de page parente, corbeille et restauration, accès, membres ; pas par un renommage ni par un changement de place parmi les mêmes frères (Ruling R83). Ce n'est pas la version monotone du journal d'opérations (§5.2), qui reste à faire. Deux caches sont rangés sous cette révision : les données de l'espace (pages, accès, rôles), lues en une transaction une fois par révision pour tout le monde (Ruling R92), et le calcul de chaque personne, borné en nombre de pages gardées (Ruling R59). Une demande lit d'abord la révision courante de l'espace. Rien ne lui est servi de plus ancien que cette révision. Des données plus récentes peuvent l'être, qu'elles viennent de la base ou du cache (Ruling R98) : elles sont rangées sous leur propre révision, et disent ce que la demande aurait vu en arrivant un instant plus tard. L'argument tient à une hypothèse, que le store garantit : la révision d'un espace ne fait que croître. Il est écrit en tête de `crates/mementum-server/src/inputs_cache.rs`. Qui n'a aucun rôle dans un espace reçoit sa réponse sans que l'espace soit lu : la durée d'un refus ne dit donc rien de l'espace (Ruling R99).
- **Contrôles dans la transaction qui écrit** (Ruling R100). Ce qui écrit un droit durable est revérifié dans la transaction qui l'écrit :
  - un changement de rôle et un retrait, sur les rôles que la route a lus (s'ils ont bougé, la route relit et redécide, puis `409 roles_changed`) ;
  - un changement d'accès, vérifié en entier au nom de qui le demande (Ruling R87) ;
  - le passage d'une page à la racine : qui la déplace doit avoir encore son rôle et l'accès complet sur l'ancienne chaîne (Rulings R82 et R94).

  Un changement de page parente est lié au parent lu : si la page a bougé entre-temps, la route relit et redécide. Les autres écritures de page (création, corbeille, restauration, suppression, changement de page parente hors racine) se décident sur les droits lus juste avant, sans aller-retour réseau : c'est une fenêtre acceptée, dite plus bas. Pour une seule page, le store calcule le niveau sur sa chaîne de pages parentes (`mementum_acl::chain_level`), avec la même règle par ligne que le calcul de tout l'espace ; un test de propriétés compare les deux.
- **Synchronisation.** `/sync` vérifie la session et le droit de lire avant la poignée de main, puis les relit une fois la page chargée. La connexion ne tient ses droits (lecture seule ou écriture) que de cette seconde lecture (Ruling R89). Elle les garde jusqu'à ce que le serveur la ferme, dans deux cas. Une écriture change ses droits : corbeille, restauration, changement de parent, partage, retrait d'un membre, changement de rôle, suppression. Ou une session de son compte se ferme (déconnexion, changement de mot de passe, connexion ou inscription par-dessus une session) : toutes les connexions du compte sont alors fermées, et celles de ses autres sessions reviennent aussitôt (Rulings R73 et R90). Changer de mot de passe ferme toutes les sessions du compte, y compris une connexion en cours avec l'ancien : une session n'est ouverte, et un mot de passe n'est remplacé, que si le mot de passe vérifié est encore celui du compte (Ruling R120). Une demande en vol reçoit alors la réponse d'un mot de passe faux. Le client revient alors par la poignée de main : avant chaque connexion, il relit la page, et apprend ainsi ce qu'elle est devenue pour lui (§4.3). Deux onglets d'un même navigateur ne se synchronisent pas entre eux directement : tout passe par le serveur, donc par cette vérification. Une connexion qui s'ouvre au même instant est fermée à son inscription, avant d'avoir rien reçu (Rulings R71 et R88). Une connexion à qui le serveur n'a rien envoyé depuis 20 s reçoit un message d'entretien : sans lui, le client la fermerait après 30 s de silence, puis la rouvrirait (Ruling R107).
- **Présence.** Le nom et le curseur de chacun passent par le serveur, qui ne les vérifie pas encore en entier : la présence vérifiée est prévue au lot 3 (Ruling R52). Ce que le lot 2 garantit déjà :
  - l'identifiant de présence d'un client appartient au compte de la première connexion vivante qui l'annonce ; d'autres connexions du même compte peuvent le tenir avec elle, et il se libère quand la dernière part. Une entrée qui vise un identifiant tenu par un autre compte est ignorée, comme le retrait d'un identifiant que personne ne tient (Rulings R101 et R104). Personne ne change donc ni n'efface le nom et le curseur de quelqu'un qui est là ;
  - un identifiant libre ne se prend que par un état plus récent que celui que le serveur connaît (Ruling R112) ; la présence d'un message de plus de 512 entrées est ignorée, sans fermer la connexion (Ruling R113 révisé) ; une entrée à la dernière horloge possible est ignorée (Ruling R108) ;
  - la présence d'une connexion en lecture seule est ignorée en entier (Ruling R118) ;
  - pour une connexion qui écrit, un état de présence n'est gardé et relayé que s'il a la forme de ceux que l'éditeur écrit, 2 Kio au plus (Ruling R119) : un nom de 1 à 100 caractères sans caractère de contrôle et une couleur en `#RRGGBB`, chacun facultatif ; un curseur nul, ou fait de deux positions de Yjs bien formées, les deux exigées. Chez les autres, la couleur va dans un attribut de style, et un curseur mal formé lève une exception dans leur éditeur : il n'affiche alors plus ce qui vient du réseau, et la frappe suivante efface celle des autres. Avant cette règle, une invitée en lecture seule pouvait ainsi faire perdre des frappes ;
  - le serveur ne relaie jamais les octets de présence qu'il reçoit, mais ce qu'il en a lu et gardé, réencodé : une différence de lecture entre le serveur et les clients ne contourne donc plus ces règles (Ruling R123) ;
  - un message WebSocket qui porte plus de 32 messages de synchronisation est illisible, et ferme la connexion : le client officiel en envoie un par message WebSocket (Ruling R124).

  Ce que cela ne garantit pas (Ruling R109) : pendant qu'une personne est coupée du réseau, quelqu'un qui peut modifier la page (plus un simple lecteur, depuis R118) peut prendre son identifiant, et la priver de nom et de curseur chez les autres tant qu'il reste là ; il peut aussi avancer l'horloge de cet identifiant, et elle reste alors invisible jusqu'à ce que la page se vide ; entre onglets d'un même compte, les états renvoyés comptent encore dans la limite de huit clients par connexion. Et après une reconnexion, la présence d'une personne inactive ne revient qu'à son deuxième changement d'état, soit jusqu'à 30 s. Aucune donnée de page ni aucun droit n'est en jeu : l'attribution des écritures vient de la session.
- **Fenêtres acceptées.** Dans les trois premières, la personne ne fait rien qu'elle n'aurait pu faire l'instant d'avant.
  - De l'ordre de la milliseconde (Ruling R74) : les messages déjà en file quand un droit change sont traités avec les anciens droits ; `PUT /mmd` relit les droits une fois la page chargée, puis écrit.
  - De quelques millisecondes (Ruling R100) : une création, une mise à la corbeille, une restauration, une suppression ou un changement de page parente hors racine se décident sur les droits lus juste avant. Quelqu'un dont le droit vient d'être retiré peut donc encore les faire pendant ce temps.
  - À l'acceptation d'une invitation sur une page : le contrôle (son auteur a encore l'accès complet à la page) et l'écriture sont deux transactions. Elles sont séparées de quelques millisecondes pour un compte qui existe, et d'une fraction de seconde à l'inscription, le temps de hacher le mot de passe. Dans sa transaction, le store revérifie que l'auteur a encore un rôle, pas son niveau sur la page.
  - D'une autre nature : une connexion restée ouverte sans coupure jusqu'à l'échéance de sa session (30 jours), ou dont la session est supprimée par `mementum user set-password`, n'est fermée qu'à sa reconnexion. La relecture périodique de la session des connexions ouvertes est reportée au lot 7 (Ruling R125).

### 5.7 Recherche
- Tantivy indexe les titres et les blocs (`blocks_index`), avec le fil d'Ariane de chaque page. Les résultats sont filtrés par permissions au moment de la requête.
- **Recherche hybride** (P4) : BM25 + vecteurs (embeddings par bloc, avec le contexte du fil d'Ariane), fusion RRF, puis rerank optionnel. Les citations pointent sur un **bloc précis**.
- Les deux index sont reconstructibles : `mementum reindex`.

### 5.8 Fichiers
- Stockage adressé par contenu (hash SHA-256), donc dédoublonné. Miniatures générées en tâche de fond.
- URL signées à durée limitée. Quotas par espace. Antivirus ClamAV en option.

### 5.9 Client : vitesse et hors ligne
- **Rendu d'abord depuis le local**, synchronisation ensuite. Un seul onglet écrit dans SQLite (worker partagé), les autres lisent.
- **Virtualisation** des longues pages (blocs hors écran non montés) et des grilles (lignes et colonnes).
- **Préchargement** : survoler un lien précharge la page.
- **Hors ligne** : pages récentes et favoris en cache automatique, plus « Rendre disponible hors ligne » pour un sous-arbre.
- **Accessibilité et confort** : clavier partout, lecteur d'écran, RTL, thème clair/sombre/contraste élevé, i18n (FR et EN dès P1).

### 5.10 Budgets de performance *(cibles à vérifier en CI)*
| Mesure | Cible |
|---|---|
| Ouvrir une page déjà en cache | < 100 ms |
| Ouvrir une page jamais vue (réseau local) | < 300 ms |
| Frappe → visible chez un autre utilisateur (même région) | < 150 ms |
| Base de 100 000 lignes : première fenêtre | < 500 ms |
| Base de 100 000 lignes : changer un filtre ou un tri | < 300 ms |
| Mémoire du serveur au repos | < 150 Mo |
| Démarrage à froid du binaire | < 1 s |
| Raspberry Pi 4 (4 Go) | ~25 utilisateurs actifs en simultané |
| VPS 2 vCPU / 4 Go | ~200 utilisateurs actifs en simultané |

**Taille des pages** *(décision validée par Alex le 9 novembre 2026, lot 1 de la P1)* :
- « Ouvrir une page jamais vue < 300 ms » vaut pour les pages **jusqu'à 1 000 blocs** ;
- au-delà, une page doit seulement s'ouvrir sans geler l'onglet : **moins de 3 s pour 10 000 blocs** ;
- raison : le coût d'une grande page est la liaison avec l'éditeur (y-prosemirror construit et affiche tous les nœuds d'un coup, et BlockNote n'a pas de virtualisation prête à l'emploi), pas Yjs ni le serveur, qui charge la page de 10 000 blocs depuis SQLite en moins de 300 ms ;
- mesure : du clic dans l'arbre (application déjà chargée) jusqu'à la fin du rendu (styles, mise en page et peinture compris) de la première image où le premier bloc est affiché dans l'éditeur synchronisé, sur des pages que l'onglet n'a jamais ouvertes et que le serveur relit dans SQLite. La mesure se fait sur ce que les gens utiliseront : un build de production (`vite build`, servi par `vite preview`) et un serveur compilé en `--release` (Ruling R38). Tests : e2e `apps/web/e2e/budgets.spec.ts` (p95 sur 5 ouvertures pour 1 000 blocs, `corepack pnpm e2e:budgets`, à part de la suite ordinaire), et `crates/mementum-sync/tests/budgets.rs` pour la part du serveur (p95 sur 20 chargements, en release, job `budgets` de la CI) ;
- arbre des pages (Ruling R43, corrigé au lot 2) : l'arbre ne met plus dans le document que les lignes visibles, une trentaine, et l'ouverture d'une page ne dépend plus du nombre de pages. Mesures sur un build de production et un serveur `--release`, avec 1 000 pages de plus dans l'arbre : p95 de 274 ms avant la correction (environ 0,11 ms par page affichée ; le lot 1 avait mesuré 0,26 ms sur Vite en développement), 142 à 167 ms après, comme avec un petit arbre. Test : `open_page_with_1000_pages_in_tree_under_300ms`. L'instabilité de `open_1000_block_page_under_300ms` (en développement, un seul pic sur 5 ouvertures suffisait à le faire échouer) est levée par R38 : dix lancements de suite au vert, p95 de 143 à 170 ms ;
- part du serveur : charger depuis SQLite une page de 10 000 blocs prend moins de 300 ms, avec le pire journal de frappes comme après une grosse suppression (page réécrite par `PUT /mmd`, ou remplacée par un client). Le ramasse-miettes de yrs, qui coûte environ 2 s pour une page de 10 000 blocs réécrite, se paie donc une fois, à l'écriture : l'acteur compacte la page dès que les mises à jour écrites depuis la dernière compaction ont supprimé 2 000 unités Yjs (`compact_after_deletions`, Ruling R40). Limite connue du lot 1 : la réécriture elle-même bloque l'acteur de la page (2,5 à 4 s selon la machine et sa charge, pour 10 000 blocs, frappes des autres en attente) ; remède prévu au lot 5, une écriture MMD par différence.

---

## 6. Conçu pour les agents IA

> C'est le cœur du projet. Objectif : qu'un agent (Claude Code, Claude Desktop, Cursor, ChatGPT, un modèle local…) puisse **lire, comprendre et modifier** une page ou une base aussi proprement qu'un humain, **sans rien casser** de ce que les humains écrivent en même temps, et **sous contrôle**.

### 6.1 Principes
1. **Du texte, pas du JSON** : les LLM éditent mieux du texte brut. Notion a abandonné le JSON de blocs pour ses agents (trop de tokens).
2. **Un format canonique sans perte** : tout ce qui existe dans Mementum a une représentation MMD, et l'aller-retour arbre → MMD → arbre est **identique** (condition de sortie de chaque version).
3. **Des éditions ciblées** : chercher-remplacer et opérations par ancre plutôt que tout réécrire.
4. **L'agent est un pair CRDT côté serveur** : ses écritures sont des transactions Yjs, donc elles fusionnent avec la frappe humaine simultanée.
5. **Mode suggestion par défaut**, contenu en attente inerte, attribution visible, annulation par exécution.
6. **Moindre privilège** : un agent n'a accès à rien par défaut, et chaque appel est vérifié et journalisé.

### 6.2 Le format MMD (Mementum Markdown)
CommonMark + GFM (tableaux, cases à cocher, barré) + `$maths$`. Les blocs propres à Mementum s'écrivent en **balises de type XML**, avec des noms alignés sur le Markdown de Notion pour que les modèles les reconnaissent et que la migration soit facile.

```markdown
---
id: pg_7Hq2Lm
title: Lancement v1
icon: 🚀
parent: pg_Root01
revision: r_8f3a1c
properties:
  Statut: En cours
  Responsable: "@lenny"
  Échéance: 2026-12-01
---

# Objectif ^b1

Sortir la **v1** avant décembre. ^b2

<callout id="b3" icon="⚠️" color="yellow">
Penser à la migration des données.
</callout>

- [ ] Écrire la doc ^b4
- [x] Préparer la démo ^b5

<toggle id="b6" title="Détails techniques">
Le serveur tourne sur un Raspberry Pi 4.
</toggle>

<database id="db_Taches" view="Tableau" anchor="b7"/>

Voir <mention type="page" id="pg_Spec01">Spécification</mention>. ^b8
```

- **Balises** : `<callout>`, `<toggle>`, `<columns>/<column width>`, `<tabs>/<tab>`, `<page>`, `<database>`, `<mention>`, `<embed>`, `<synced ref>`, `<file>`, `<span color>`, `<comment>` (fils ancrés, sur demande).
- **Échappatoire sans perte** : `<block type="x" id="…" data="…"/>` pour tout bloc sans équivalent (bloc d'une extension, par exemple). L'agent doit le laisser tel quel ; le supprimer exige `allow_deleting_content`.
- **Ancres optionnelles** : à la lecture, avec `anchors: "block"`, chaque bloc reçoit un alias court local à la page (`^b4`, la syntaxe des références de bloc d'Obsidian). Sans ancres, le Markdown reste propre et économe en tokens.
- **`revision`** : une poignée opaque, hash de l'instantané Yjs (vecteur d'état **et** suppressions, §6.3), renvoyée à chaque lecture. Les pages très longues sont paginées (`truncated: true`, `<unloaded id="…"/>`).
- La **spécification MMD** est publiée sous Apache-2.0 et exposée aux agents comme ressource MCP (`mementum://spec/mmd`), pas dans les descriptions d'outils.
- **Règles de format établies en P0** (convertisseur `spike/mmd`, aller-retour MMD ⇄ arbre validé sur 12 800+ cas aléatoires ; la couche Yjs n'est couverte que par la fixture, et le frontmatter n'est pas encore géré) :
  - **Marques** : en Markdown (`**`, `*`, `~~`, `` ` ``) quand le texte marqué commence et finit par une lettre ou un chiffre et qu'il est entouré de texte sans marque ; sinon en balises (`<strong>`, `<em>`, `<s>`, `<code>`). `<u>` et `<span color>` sont toujours en balises. *(P1 : en plus, un seul genre de délimiteur ; voir « Contenu en ligne » plus bas.)*
  - **Espaces en début et fin de contenu** écrites `&#32;` (Markdown les supprimerait). Un paragraphe vide s'écrit `<p/>`.
  - **Ancres** : ` ^bN` en fin de ligne pour les blocs de texte, `^bN` sur la ligne suivante pour le code, les équations, les séparateurs, les images et les tableaux, attribut `id="bN"` pour les balises, attribut `anchor="bN"` pour `<page>` et `<database>` (leur `id` est déjà pris).
  - **Indentation** : les enfants d'un élément numéroté sont indentés de la largeur du marqueur (« 1. » = 3 espaces).
  - **Maths en ligne** `$…$` : LaTeX à accolades équilibrées. *(P1 : une formule que `$…$` ne relit pas exactement s'écrit `<math latex="…"/>`.)*
  - **Échappement** : un « ! » juste avant un lien est échappé (sinon il serait relu comme une image).
  - **Texte littéral** : `` \ * _ ` [ ] ~ $ | ^ `` sont précédés de `\` ; `<` s'écrit `&lt;` et `&` s'écrit `&amp;`. Dans un attribut, `&`, `"` et `<` s'écrivent `&amp;`, `&quot;` et `&lt;`. *(P1 : `{` et `}` aussi, et les caractères de contrôle ; voir « Échappements » plus bas.)*
  - **Début de ligne** : un contenu qui commence par `#`, `>`, `-`, `+`, `N.` ou `N)` est échappé (`\#`, `1\.`) pour ne pas être lu comme un bloc.
  - **Blocs de code** : clôture de backticks plus longue d'un cran que la plus longue suite de backticks du code (3 au minimum).
  - **Listes** : les éléments consécutifs d'un même type forment une seule liste, sans ligne vide entre eux.
  - **Toggle** : le titre est du MMD en ligne écrit dans l'attribut `title`. **Callout** : le premier paragraphe est le contenu du callout, les suivants sont ses enfants.
  - **Enfants** : seuls les listes, toggle, synced, columns, column et callout ont des enfants. *(Remplacé en P1 : tout bloc a des enfants ; voir « Enfants de tout bloc » plus bas.)*
  - **Tableaux** : la première ligne est toujours l'en-tête. *(Remplacé en P1 : la première ligne reste la ligne d'en-tête GFM, mais `header-rows` et `header-cols` disent combien de lignes et de colonnes sont des en-têtes.)* **Colonnes** : `width` est omis quand il vaut 1.
  - **Texte brut** (sans marques) : titre d'un `<page>` et texte alternatif d'une image.
- **Format MMD de la P1** (crate `mementum-md`, lot 1). Ces règles complètent celles de la P0 ; là où elles diffèrent (enfants, tableaux, échappements, marques), elles les remplacent. Le principe : **tout ce que BlockNote sait représenter a une forme MMD, et la relecture est exacte** (aller-retour vérifié par proptest sur tous les types du registre). Les types, props, défauts et formes viennent du registre `mementum-schema` (`spec/schema/blocks.json`). Une prop égale à son défaut ne s'écrit pas.

  | Type BlockNote | Forme MMD | Props portées par la forme | Autres props (nom MMD) |
  |---|---|---|---|
  | `paragraph` | une ligne ; vide : `<p/>` (props en attributs) | — | `bg`, `color`, `align` |
  | `heading` | `#` × niveau, puis une espace (niveaux 1 à 4) | `level` | `bg`, `color`, `align`, `toggle` (`isToggleable`) |
  | `bulletListItem` | `- ` | — | `bg`, `color`, `align` |
  | `numberedListItem` | `N. ` (N = `start` sur le premier élément de la liste, s'il vaut autre chose que 1) | `start` | `bg`, `color`, `align` |
  | `checkListItem` | `- [ ] ` ou `- [x] ` | `checked` | `bg`, `color`, `align` |
  | `toggleListItem` | `<toggle title="…">` … `</toggle>` | contenu en ligne = `title` | `bg`, `color`, `align` (attributs) |
  | `quote` | `> ` | — | `bg`, `color` |
  | `codeBlock` | clôture de backticks + langue ; contenu en texte brut (BlockNote : `content: "plain"`) : ni marque, ni lien, ni mention, ni formule, un saut de ligne reste un saut de ligne du texte (`\n`) ; toute langue est gardée, même hors de celles que colore le client | `language` | — |
  | `divider` | `---` | — | — |
  | `image` | `![légende](url)` | `url`, `caption` | `name`, `preview` (`showPreview`), `width` (`previewWidth`), `align`, `bg` |
  | `video` | `<video src="…"/>` | `url` (`src`) | `caption`, `name`, `preview`, `width`, `align`, `bg` |
  | `audio` | `<audio src="…"/>` | `url` (`src`) | `caption`, `name`, `preview`, `bg` |
  | `file` | `<file src="…" name="…"/>` | `url` (`src`), `name` | `caption`, `bg` |
  | `table` | tableau GFM ; la ligne `{…}` qui suit porte `header-rows` (omis s'il vaut 1), `header-cols` (`headerCols`, omis s'il vaut 0), `widths` (`"120,,80"`, vide = auto : une largeur par colonne de la grille, comme `columnWidths` de BlockNote, une cellule fusionnée de la première ligne en couvrant `colspan`) et `color`. En-têtes dans la grille, sous la forme que portent les cellules `tableHeader` de BlockNote : un tableau tout en en-têtes s'écrit `header-rows` = nombre de lignes, sans `header-cols` | lignes | `header-rows`, `header-cols`, `widths`, `color` |
  | cellule de tableau | `\| texte {…} \|` | contenu | `bg`, `color`, `align`, `colspan` (de 1 à 100), `rowspan` (de 1 au nombre de lignes restantes) |
  | `callout` | `<callout icon="…" color="…">` : 1ᵉʳ paragraphe = contenu, puis enfants ; contenu vide et 1ᵉʳ enfant paragraphe : `<p/>` en tête | `icon`, `color` | — |
  | `equation` | `$$`, le LaTeX, puis `$$` | `latex` | — |
  | `bookmark` | `<bookmark url="…"/>` | `url` | — |
  | `columnList` / `column` | `<columns>` / `<column>`, au schéma de `@blocknote/xl-multi-column` 0.57 : un `<columns>` contient deux `<column>` ou plus, et rien d'autre ; une `<column>` n'existe que dans un `<columns>`, contient au moins un bloc, et jamais directement un `<columns>` | — | `width` (omis s'il vaut 1) |
  | `pageLink` | `<page id="…">titre</page>` | `pageId`, `title` | — |
  | `databaseRef` | `<database id="…" view="…"/>` | `databaseId`, `view` | — |
  | `syncedBlock` | `<synced ref="…">` … `</synced>` | `ref` | — |
  | `toc` | `<toc/>` | — | — |
  | `unknown` et **tout type inconnu du serveur** | `<block type="…" id="…" data="…"/>`, `data` = base64 du JSON `{"attrs":{…},"content":[…],"children":[…]}` du nœud Yjs d'origine | `originalType` (`type`), `data` | `foreign` (`foreign=true` pour un nœud étranger à recréer tel quel) |

  - **Props hors forme** : sur une forme en ligne ou à clôture, liste `{…}` en fin de première ligne, avant l'ancre (`Texte {align="center" color="red"} ^b3`, `` ``` {language="mon langage"} ``) ; pour un tableau, sur la ligne qui suit ses lignes ; sur une balise, en attributs. Clés triées par ordre alphabétique. Chaîne toujours entre guillemets (échappée comme un attribut), nombre et booléen sans guillemets, y compris dans les balises (`width=320`, `preview=false`, `<column width=0.5>`). Un flottant d'une prop du registre s'écrit sans « .0 » (`width=320`).
  - **Sac de props** : une prop inconnue du registre garde son nom BlockNote et son type d'après son écriture (guillemets : chaîne ; nombre sans point : entier ; nombre avec point : flottant, `2.0` gardé tel quel ; `true`/`false`). Rien ne se perd **à la lecture** : `GET /mmd` rend telle quelle une prop inconnue déjà présente dans Yjs. À l'écriture, au lot 1, `PUT /mmd` la refuse, sur un bloc comme sur une cellule de tableau (`422 invalid_blocks`, Ruling R41 ; voir « Limites connues »).
  - **Valeur que la forme ne peut pas porter** : elle passe dans la liste `{…}` sous son nom MMD. Titre de niveau 5 : `# Titre {level=5}` ; `start` sur un élément qui n'est pas le premier de sa liste, ou valant 1, ou hors de 0 à 999 999 999 : `{start=…}` ; langue avec une espace : `` ``` {language="mon langage"} `` ; adresse d'image avec une espace : `![légende]() {url="…"}` ; LaTeX qui contient une ligne `$$` : `$$ {latex="…"}`.
  - **Balises** : attributs dans cet ordre : ancre `id`, `title` (toggle), attributs portés par la forme (toujours écrits, même à leur défaut : `<callout icon="💡" color="default">`), autres props triées, puis l'ancre `anchor` pour `<page>` et `<database>`. Pas d'espace avant `/>`. `<block>` écrit toujours le vrai id du bloc, même avec les ancres, et ne reçoit pas d'alias.
  - **Enfants de tout bloc** : les enfants d'un bloc à forme en ligne, à clôture ou autofermante (`<video/>`, `<page>…</page>`, `<p/>`, `<block/>`…) suivent sa dernière ligne (ancre et ligne `{…}` comprises), sans ligne vide, indentés de 2 espaces (de la largeur du marqueur pour un élément numéroté : « 1. » = 3, « 10. » = 4). Entre deux enfants, la séparation est la même qu'au premier niveau ; une ligne vide n'a jamais d'espaces. Les balises `toggle`, `callout`, `columns`, `column` et `synced` contiennent leurs enfants entre leurs balises, sans ligne vide après l'ouvrante ni avant la fermante. Dans un callout, le premier enfant suit le contenu sans ligne vide.
  - **Lignes vides** : une entre deux blocs frères, aucune entre deux éléments voisins d'une même liste (puces, numérotés, cases) ou entre deux `<column>`. Un marqueur sans contenu n'a pas d'espace finale (`##`, `-`, `- [ ]`, `1.`, `>`).
  - **Contenu en ligne** : règles de la P0, avec trois précisions. Délimiteurs Markdown seulement pour un seul genre de marque (`**`, `*`, `***` pour gras et italique, `~~` ou `` ` ``) : deux genres imbriqués (`~~**a**~~`, `` **`a`** ``) ne se refermeraient pas devant une lettre, ils passent en balises. Formule en ligne : `$…$` quand cette forme relit exactement la formule (`$\frac{a}{b}$`, `$\$5$`) ; sinon (formule vide, blanc en bord, `$` non échappé, `$$`, accolades déséquilibrées, saut de ligne, barre oblique finale…) `<math latex="…"/>`, attribut échappé comme les autres ; la lecture refuse la balise pour une formule que `$…$` porte. Surlignage : `<span bg="…">` ; couleur et surlignage : `<span bg="…" color="…">`. Ordre des marques, de l'intérieur vers l'extérieur : `<code>`, `<em>`, `<strong>`, `<s>`, `<u>`, puis `<span bg="…" color="…">` ; un délimiteur Markdown prend la place de sa balise, donc à l'intérieur de `<u>` et de `<span>` (`<span color="blue"><u>souligné coloré</u></span>`, `<u>**gras souligné**</u>`) ; un lien entoure ses textes marqués (`[**a**](x)`). Saut de ligne (`hardBreak`) : `<br/>`. Lien : `[texte](href)` si le `href` ne contient ni blanc, ni caractère de contrôle, ni `(`, `)`, `<`, `>`, `\`, `&` ; sinon `<a href="…">texte</a>`. Deux liens voisins de même `href` n'en font qu'un, comme dans Yjs et ProseMirror : la lecture refuse `[a](x)[b](x)` et demande `[ab](x)`.
  - **Échappements** : le jeu de la P0, plus `{` et `}` (écrits `\{` et `\}`), puisqu'une liste `{…}` porte des props. Un caractère de contrôle du texte (tabulation, saut de ligne…) s'écrit en référence numérique (`&#9;`, `&#10;`) ; dans un attribut, saut de ligne et retour chariot s'écrivent `&#10;` et `&#13;`. Le titre d'un `<page>` échappe `&`, `<`, `\n` et `\r`.
  - **Document** : chaque ligne finit par `\n` (LF) ; un document vide est le texte vide.
  - **Identifiants** : `^xyz` en fin de ligne (ou seul sur la ligne qui suit un bloc à clôture) et `id="…"` (`anchor="…"` pour `<page>` et `<database>`) donnent l'id du bloc, tel quel ; la traduction des alias `bN` est faite par l'API des agents (lot 5). Un bloc sans id est lu avec `id = ""`, et `mementum-crdt` lui donne un UUIDv7 à l'écriture.
  - **Lecture stricte** : `parse` n'accepte que la forme canonique. Tout le reste est une erreur `MmdError { line, message }`, avec un message en français qui dit quoi écrire : marqueurs `*` ou `+`, `N)`, titres soulignés (setext), `#####`, code indenté, clôture `~~~`, balise ou clôture non fermée, indentation incohérente, ligne vide manquante ou en trop, liste `{…}` mal formée, accolade non échappée, fusions de cellules hors grille (voir « Tableaux » plus bas), fins de ligne CRLF, imbrication de plus de 100 niveaux (`MAX_DEPTH`, « imbrication trop profonde : 100 niveaux au plus »), colonnes hors du schéma de BlockNote (`<column>` hors de `<columns>`, `<columns>` d'une seule colonne ou qui contient autre chose, `<column>` vide, `<columns>` directement dans une `<column>`)… Après la lecture, l'arbre est réécrit et doit redonner l'entrée octet pour octet ; sinon, la première ligne qui diffère est signalée avec la forme attendue (valeur par défaut écrite, échappement ou marque non canonique). L'import de Markdown quelconque (Obsidian, Notion) est un autre convertisseur, prévu au lot 6.
- **Limites connues du format P1** :
  - **Imbrication** : 100 niveaux au plus (`MAX_DEPTH`). En lisant la page, `mementum-crdt` place les descendants d'un bloc de niveau 100 à sa suite, au même niveau, et défait une liste de colonnes trop profonde ou mal formée (ses blocs prennent sa place). Une page Yjs ne dépasse jamais `MAX_YJS_DEPTH` éléments imbriqués (2 par niveau de bloc, plus une marge) : la suppression et le ramasse-miettes de yrs sont récursifs, et un arbre plus profond ferait déborder la pile du serveur.
  - **Page vide** : BlockNote n'a pas de document sans bloc. `mementum-crdt` écrit un arbre vide en un paragraphe vide, relu `<p/>`.
  - **Sac de props** : le nom d'une prop inconnue doit être un identifiant (`[A-Za-z][A-Za-z0-9_-]*`) qui ne recoupe aucun nom MMD du bloc ni `id`, `anchor`, `title`, `type`, `data`, `header-rows`, `header-cols`, `widths`. Les props connues sont supposées typées selon le registre ; un flottant non fini (`NaN`, infini) n'a pas de forme. **Refusé à l'écriture au lot 1** (Ruling R41) : `mementum-md` lit et rend toujours le sac, mais `PUT /mmd` répond `422 invalid_blocks` à toute prop hors registre, avec un message qui nomme la prop et le type du bloc. Raison : y-prosemirror retire du document partagé tout attribut que le schéma du client ne déclare pas, dès qu'un navigateur modifie le bloc, chez tout le monde et sans trace ; un 204 aurait promis une donnée qui disparaît ensuite. Le refus sera levé quand un porteur durable existera : un attribut déclaré au schéma du client, qui porte le sac (à voir au lot 3 ou au lot 5). D'ici là, une page qui porte une prop inconnue écrite par un client Yjs se relit par `GET /mmd`, mais ce texte est refusé par `PUT /mmd`.
  - **Tableaux** : un tableau sans ligne ou une ligne sans cellule n'ont pas de forme (BlockNote n'en produit pas). Les fusions forment une grille (Ruling R37) : `colspan` vaut de 1 à 100 (`MAX_WIDTH_COLUMNS`), `rowspan` de 1 au nombre de lignes restantes, ligne courante comprise, et chaque ligne couvre autant de colonnes que la première, cases occupées par une cellule d'une ligne au-dessus comprises, sans chevauchement ; sinon `parse` refuse le tableau, en disant quoi écrire. prosemirror-tables parcourt toute la grille et y crée une colonne par case : un `colspan` démesuré fige l'onglet, un négatif fait lever BlockNote, et une ligne trop courte ou trop longue, ou un `rowspan` trop long, sont corrigés par `fixTables`, qui réécrit le document à la première édition. Un client Yjs peut encore écrire des fusions hors de ces règles (reporté au lot 3, Ruling R54) : le serveur les relit et les rend telles quelles, mais ce MMD est refusé à l'écriture. `widths` s'écrit dès qu'une colonne a une largeur, et en porte une par colonne de la grille de la première ligne, ni plus ni moins (une cellule fusionnée en couvre `colspan`) : `widths="1,2,3"` sur deux colonnes se lit comme une erreur qui demande `widths="1,2"`. Dans Yjs, chaque cellule porte la largeur des colonnes qu'elle couvre (`colwidth`, `[120, undefined]` pour une cellule fusionnée dont seule la première colonne a une largeur), colonnes décalées par un `rowspan` comprises. Au plus 100 colonnes de la grille portent une largeur (`MAX_WIDTH_COLUMNS`) : le MMD borne `colspan` à cette même valeur (Ruling R37), mais pas un client Yjs, dont une seule cellule ferait sinon écrire des millions de largeurs. Lignes et colonnes d'en-tête sont ramenées dans la grille (au plus le nombre de lignes, et la longueur de la plus courte ligne), puis à ce que distinguent les cellules d'en-tête : `header-rows=5` sur deux lignes se lit comme une erreur qui demande `header-rows=2`.
  - **Type hors registre** : `render` l'écrit `<block type="…"/>`, relu en `unknown` ; `mementum-crdt` lit d'abord tout nœud étranger en `unknown` avec `foreign=true`.

### 6.3 Opérations d'édition
Un seul outil `update_page` accepte une liste d'opérations, **atomique** (jusqu'à 100 opérations par appel) :

```json
{
  "page": "pg_7Hq2Lm",
  "base_revision": "r_8f3a1c",
  "mode": "suggest",
  "operations": [
    { "op": "str_replace", "old": "avant décembre", "new": "avant le 1er décembre" },
    { "op": "insert", "after": "b5", "markdown": "- [ ] Enregistrer la vidéo de démo" },
    { "op": "replace_block", "anchor": "b3", "markdown": "<callout icon=\"✅\" color=\"green\">Migration prête.</callout>" },
    { "op": "set_properties", "properties": { "Statut": "Terminé" } }
  ]
}
```

| Opération | Usage |
|---|---|
| `str_replace {old, new, replace_all?}` | Opération par défaut pour les modèles forts. Le texte cherché doit être unique ; en cas d'échec, le serveur renvoie les candidats les plus proches |
| `insert {after \| before \| position, markdown}` · `replace_block {anchor, markdown}` · `delete_block {anchor}` · `move_block {anchor, after}` | Édition précise par ancre, idéale pour les modèles locaux plus faibles |
| `replace_page {markdown}` | Seulement pour une page neuve ou petite. Sous-pages et bases protégées. **Refusé (409) si des humains éditent en ce moment** |
| `set_properties {…}` | Propriétés de la page ou de la ligne, validées selon le schéma |

- **Concurrence** : `on_conflict: "rebase"` (par défaut). Si les blocs touchés n'ont pas changé depuis `base_revision`, le serveur applique ; sinon il renvoie un conflit avec les extraits à jour. `"fail"` rejette dès que le document a changé (sémantique de Google Docs).
  - *Validé en P0* : la `revision` est le hash de l'instantané Yjs complet (vecteur d'état **et** suppressions, sinon une suppression pure ne change pas la révision).
  - L'appel est atomique (répétition à blanc avant application).
  - Les remplacements se font aux positions UTF-16, comptées sur le **texte brut** du bloc (le spike comptait sur `get_string()`, qui écrit les marques en balises, et corrompait le texte après du gras ou une mention : corrigé à la relecture).
  - Un agent qui édite pendant qu'un humain tape : 0 perte, visible en 13 à 16 ms (p95, depuis l'envoi de l'appel).
  - La répétition à blanc copie le document à chaque appel : c'est un raccourci de spike. En P1, valider sans copie, et calculer révision et instantané dans la même section critique que l'écriture. Les ids des blocs insérés par un agent sont régénérés par le serveur.
- **Réponse** : le MMD des blocs touchés après l'édition (ou la page entière si elle est petite), la nouvelle `revision`, des `warnings[]` (`no_match`, `ambiguous_match`, `stale_anchor`) et les `suggestion_ids` créés.

### 6.4 Fusion avec l'édition humaine en direct
- Le serveur rejoue chaque appel d'agent comme **une transaction Yjs**, avec `origin = {agent_id, run_id, on_behalf_of}`. Les opérations visent des identifiants de bloc et des positions relatives, donc la frappe humaine ailleurs dans la page fusionne sans conflit.
- **Trois modes d'écriture** :
  - `suggest` (par défaut pour les agents d'espace et les droits « suggérer ») : marques de suivi des changements visibles par tous, à accepter ou refuser par suggestion ou pour toute l'exécution ;
  - `direct` (agent interactif qui a le droit de modifier) : appliqué tout de suite, mais attribué et annulable ;
  - `branch` (grosses réécritures) : copie de travail, comparaison, puis fusion.
- **Le contenu IA en attente est inerte** : pas de chargement d'image ou d'embed externe, pas d'aperçu de lien, pas de mention ni de notification, pas de déclenchement d'automatisation tant qu'il n'est pas accepté. C'est la leçon de l'exfiltration PromptArmor chez Notion, corrigée le 07/01/2026.
- **« Annuler l'exécution »** retire toutes les transactions d'un `run_id`. Le contenu IA porte un badge visible et apparaît dans l'historique (agent, modèle, humain responsable).
- **Contre-pression** : écriture bloc par bloc, débit limité par agent, présence de l'agent regroupée (~300 ms) et affichée « 🤖 Agent X écrit… ».

### 6.5 Serveur MCP intégré
- **Transport** : Streamable HTTP sur `/mcp`, sans état, cible spec MCP **2026-07-28** et rétrocompatible avec la 2025-11-25. `/mcp/readonly` expose seulement la lecture.
- **Auth** : OAuth 2.1 (métadonnées RFC 9728, enregistrement des clients par CIMD, audience RFC 8707, portées progressives). **Jetons personnels** pour les modèles locaux et la CLI. Tout jeton a une **portée** : tous les espaces de son propriétaire, un seul espace, ou des pages choisies avec leurs sous-pages (§4.4). Pont stdio pour les clients qui ne savent pas faire de HTTP.
- **14 outils** :

| # | Outil | Rôle |
|---|---|---|
| 1 | `search` | Recherche hybride. Filtres : espace, type, date, auteur. Renvoie titres, fils d'Ariane, extraits ancrés |
| 2 | `fetch` | Page, base, ligne ou bloc par ID ou URL → MMD + propriétés + `revision` (ancres en option, pagination) |
| 3 | `create_pages` | Création en lot sous un parent, ou à la racine d'un espace désigné par son nom ou son ID (implicite si le jeton n'en ouvre qu'un), contenu MMD, propriétés |
| 4 | `update_page` | Opérations du §6.3, `mode: suggest \| direct \| branch` |
| 5 | `manage_pages` | Déplacer, dupliquer, mettre à la corbeille, restaurer. La corbeille demande une confirmation humaine |
| 6 | `query_database` | Lignes via filtre JSON, via une vue enregistrée ou via **SQL en lecture seule** ; curseur, `limit` ≤ 100 |
| 7 | `upsert_rows` | Création ou mise à jour de lignes en lot, avec résultat de validation par ligne |
| 8 | `get_database` | Schéma typé, vues, nombre de lignes, options des sélections |
| 9 | `update_database_schema` | Ajouter, renommer ou supprimer des propriétés et des options (portée plus élevée) |
| 10 | `manage_views` | Créer et modifier des vues (filtres, tris, groupes) |
| 11 | `comments` | Lister, créer, résoudre ; ancrés sur un bloc |
| 12 | `suggestions` | Lister, accepter ou refuser (par défaut, seul un humain accepte) |
| 13 | `workspace_info` | Qui suis-je, espaces accessibles et leurs membres ; correspondance nom ↔ ID |
| 14 | `get_changes` | Ce qui a changé depuis une `revision` ou une date, pour qu'un agent se remette à jour sans tout relire |

- **Ressources** : `mementum://spec/mmd`, `mementum://spec/filters`, `mementum://spec/sql` (tables virtuelles disponibles), `mementum://spec/formulas`.
- **Prompts** : quelques procédures prêtes (« résumer la page avec citations », « trier la base », « rédiger un compte rendu »).
- **Hygiène des sorties** : quelques milliers de tokens par défaut, plafond dur bien sous 25 000 avec une consigne pour affiner la requête. Les noms sont préférés aux UUID. Les erreurs (`isError`) disent comment corriger, avec un exemple. Les limites de débit sont structurées (`retry_after_seconds`). Les opérations longues passent par l'extension Tasks.
- **Plus tard** : une MCP App de revue des suggestions et un « mode code » (SDK dans un bac à sable) pour les opérations en masse.

### 6.6 API REST, SDK, CLI, événements
- **REST `/api/v1`** avec spec **OpenAPI**. Ce sont les mêmes opérations que le MCP : les schémas MCP sont générés depuis l'OpenAPI, donc il n'y a qu'une source de vérité. Le MMD est accepté et renvoyé partout (`Accept: text/markdown`).
- **SDK TypeScript et Python** générés, sous Apache-2.0.
- **CLI `mementum`** : administration (`serve`, `backup`, `restore`, `user create`, `import notion`, `reindex`) et contenu (`mementum page cat <url>`, `mementum page edit`). Elle sert aussi aux agents en terminal.
- **Webhooks** signés HMAC et **flux SSE** des événements, filtrés par permissions.
- **`llms.txt`** et versions Markdown des pages **publiées**, pour les robots IA du web.

### 6.7 Identité et permissions des agents
- **Trois sortes de sujets** :
  1. les humains ;
  2. les **agents d'espace** : membres « robots », gratuits, créés par un admin ou un membre autorisé, **sans aucun accès par défaut**, avec un humain **parrain** responsable ;
  3. les **clients MCP externes**, qui agissent au nom d'un humain (portée du jeton ∩ droits de cet humain).
- **Niveaux d'accès des agents** : `lire < suggérer < modifier < complet`, accordés sur un espace entier, un sous-arbre de pages ou une base.
- **Jetons d'API** (§4.4) : un **jeton personnel** agit au nom de son propriétaire (droits de cet humain ∩ portée du jeton) ; un **jeton d'intégration** appartient à un agent d'espace et ne sort jamais de cet espace. Les deux sont stockés hachés, montrés une seule fois à leur création, révocables, avec une expiration en option.
- **Délégation façon Linear** : une page ou une ligne peut avoir un **responsable humain** et un **délégué IA**. L'humain reste redevable.
- **Politique par agent** : modèles autorisés, mode d'écriture, canaux sortants (web, e-mail, webhook), maximum d'appels, de tokens et de durée par exécution, débit, budget mensuel.
- **Approbations obligatoires** : corbeille, suppression de propriété, changement de permissions, éditions de plus de N blocs ou lignes, tout envoi vers l'extérieur.
- **Audit** : journal immuable de chaque appel d'outil et de la transaction qui en résulte (sujet, au nom de qui, client, modèle, exécution, objets, révisions, décision). Exportable, avec annulation par exécution.

### 6.8 Sécurité IA
- **Règle des deux** (Meta) / **« trifecta létale »** (Willison) : à la configuration d'un agent, Mementum **avertit ou bloque** s'il cumule sans validation humaine (a) la lecture de contenu non fiable, (b) l'accès à des données privées et (c) la capacité d'agir ou de communiquer vers l'extérieur.
- **Contenu non fiable étiqueté** dans les sorties d'outils (provenance), **CSP stricte** et **liste blanche de sortie réseau** pour tout contenu rendu par l'IA. La recherche ne va jamais chercher d'URL arbitraire.
- **Le serveur vérifie les permissions à chaque appel.** Un classifieur LLM n'est jamais la barrière de sécurité.
- Les outils web des agents intégrés sont **désactivés par défaut**.

### 6.9 IA intégrée (optionnelle, P4)
- **Passerelle multi-fournisseurs** : API compatibles OpenAI (Ollama, vLLM, LM Studio, OpenRouter…), Anthropic natif, Mistral. Chaque espace apporte sa clé ; l'admin d'instance choisit les fournisseurs autorisés. **Mode 100 % local** possible.
- **Dans l'éditeur** : `/ia` pour écrire, résumer, traduire ou continuer, toujours en suggestion.
- **Questions-réponses sur l'espace** avec citations au niveau du bloc. Pour un petit périmètre (une page, une base), mode « contexte complet ».
- **Propriétés IA** (remplissage automatique : résumé, étiquettes, extraction), exécutées en tâche de fond par un agent d'espace.
- **Agents de fond** déclenchés par une planification, un événement de base ou une @mention, avec journal d'exécution, coût et annulation.
- L'IA intégrée et les agents externes utilisent **exactement les mêmes outils** (§3.3, principe 6).

---

## 7. Auto-hébergement

### 7.1 Installation
```bash
docker run -d --name mementum -p 8080:8080 -v mementum-data:/data ghcr.io/mementumhq/mementum:latest
```
On ouvre `http://serveur:8080` et le premier compte créé devient admin d'instance. C'est tout.

Autres voies :
- **Binaire** : `curl -fsSL https://mementum.dev/install.sh | sh`, puis `mementum serve` (Linux, macOS, Windows, ARM64).
- **docker-compose** d'exemple avec Postgres + Garage (S3) + Caddy, pour les grosses instances.
- Paquets Proxmox (script LXC), Unraid, TrueNAS, Coolify, Helm *(plus tard)*.

### 7.2 Configuration (`mementum.toml` ou variables d'environnement)
```toml
public_url = "https://notes.exemple.fr"
data_dir   = "/data"

[database]            # absent = SQLite dans data_dir
url = "postgres://mementum@db/mementum"

[storage]             # absent = disque local
s3_endpoint = "https://garage.local"
s3_bucket   = "mementum"

[auth]
signup = "invite"     # open | invite | domain
oidc   = [{ name = "Authentik", issuer = "https://auth.exemple.fr", client_id = "mementum" }]

[smtp]                # optionnel : liens magiques, notifications
host = "smtp.exemple.fr"

[https]               # optionnel : certificat Let's Encrypt intégré
acme_email = "admin@exemple.fr"
```
Les secrets passent par des variables d'environnement ou des fichiers (`*_FILE`), jamais en clair dans le toml.

**Déjà configurable au lot 2 de la P1.** Pas encore de fichier `mementum.toml` (lot 7) : quatre réglages, chacun par une variable d'environnement ou par une option de `mementum serve`, l'option l'emportant sur la variable. Une valeur invalide arrête le démarrage.

| Réglage | Variable | Option | Défaut |
|---|---|---|---|
| adresse d'écoute | `MEMENTUM_BIND` | `--bind` | `127.0.0.1:8080`, la boucle locale seulement |
| `data_dir` | `MEMENTUM_DATA_DIR` | `--data-dir` | `./data` (base SQLite `mementum.db`) |
| `auth.signup` | `MEMENTUM_SIGNUP` | `--signup` | `invite` (Ruling R50, à confirmer) ; `open` admis ; `domain` au lot 7 |
| `public_url` | `MEMENTUM_PUBLIC_URL` | `--public-url` | aucune |

- Le premier compte est toujours permis, quel que soit `signup`, et administre l'instance ; le serveur le rappelle dans son journal tant qu'aucun compte n'existe.
- `public_url` (Ruling R51) : en `https`, le cookie de session porte `Secure` ; son origine est admise sur les écritures et sur la synchronisation ; son hôte est admis par le contrôle de l'en-tête `Host` quand le serveur n'écoute que sur la boucle locale, derrière un mandataire ou un tunnel (Ruling R78). L'option `MEMENTUM_ALLOWED_HOSTS` n'est pas faite : le propriétaire l'a écartée. Hors de la boucle locale, le contrôle de `Host` est coupé (Ruling R31) : ce sont les comptes qui protègent.
- Le lien d'une invitation est construit par l'application, sur l'adresse où elle est ouverte, et non sur `public_url`.
- Pas de SMTP : une invitation est un lien à copier, et un mot de passe oublié se règle par `mementum user set-password` (Ruling R47). Pas d'OIDC avant la P3.
- Le niveau des journaux se règle par `RUST_LOG`.

### 7.3 Montée en charge
1. **Un nœud + SQLite** : jusqu'à quelques centaines d'utilisateurs actifs.
2. **Un nœud + Postgres + S3** : plusieurs milliers. On monte en puissance la machine.
3. **Plusieurs nœuds** *(plus tard)* : chaque page ou espace est routé vers un nœud (« un processus par document », comme Figma), avec diffusion inter-nœuds via Postgres LISTEN/NOTIFY ou NATS.

### 7.4 Sauvegardes, mises à jour, migrations
- `mementum backup` (à chaud, cohérent) vers un disque ou S3, planifiable dans l'admin. Compatible Litestream pour la réplication continue de SQLite.
- `mementum restore`, et `mementum migrate --to postgres` pour passer de SQLite à Postgres sans rien perdre.
- Migrations de schéma automatiques au démarrage, avec sauvegarde préalable. Le changelog signale les ruptures, et il n'y en aura pas sans outil de migration.
  - Fait au lot 2 de la P1 (Rulings R67, R72 et R75) : avant toute migration en attente, une copie cohérente de la base est écrite à côté d'elle (`mementum.db.avant-<version>.bak`, puis des noms datés si la base a changé entre deux tentatives). Rien n'est jamais écrasé ni supprimé, et le serveur ne démarre pas s'il ne peut pas l'écrire. Le README dit comment reconnaître la plus récente et la restaurer. `mementum backup` et `mementum restore` restent à faire.

### 7.5 Fédération *(après la 1.0)*
En v1, chaque instance est indépendante. On collabore entre serveurs avec des invités (connexion OIDC d'une instance à l'autre) et des liens publics. L'étape suivante, « Mementum Link », synchroniserait une page partagée entre deux instances (CRDT de serveur à serveur). Les « Spaces » privés d'AT Protocol ne sont qu'en alpha, donc on ne s'y appuie pas pour l'instant.

### 7.6 Mementum Cloud : l'offre hébergée *(décision de Lenny ; modalités = mes propositions)*
Pour ceux qui ne veulent pas héberger : on crée un compte sur `mementum.app` et on a son espace en une minute.

- **Même logiciel, mêmes fonctions.** Le cloud fait tourner exactement le binaire open source publié. Pas de fork, pas de version « Pro ». Le client paie pour ne rien gérer :
  - hébergement, mises à jour, sauvegardes chiffrées hors site, surveillance, HTTPS et domaine personnalisé ;
  - e-mails transactionnels, support, et en option des crédits IA inclus (ou sa propre clé).
- **Aller-retour sans enfermement, dans les deux sens** :
  - un bouton « Passer en auto-hébergement » télécharge l'export `.mementum` complet (ou le fichier SQLite de l'instance) et donne la commande `docker run` pour repartir chez soi ;
  - un auto-hébergeur peut migrer vers le cloud de la même façon ;
  - c'est un argument de vente, la réponse directe aux plaintes contre Notion et contre l'open-core.
- **Architecture proposée : une instance par client.**
  - Chaque organisation cliente a son conteneur `mementum` avec sa base SQLite, répliquée en continu vers un stockage S3 (Litestream) et ses fichiers sur S3.
  - C'est le design existant (binaire unique, SQLite par défaut) réutilisé tel quel : isolation forte entre clients, sauvegarde et restauration par client, export trivial, coût faible pour les petites équipes (une instance inactive est mise en veille et réveillée à la première requête).
  - Les très grosses organisations passent sur une instance dédiée avec Postgres.
  - Écarté pour commencer : un seul gros déploiement multi-locataire, plus efficace à très grande échelle mais plus risqué (une fuite entre clients) et sans intérêt tant que le cloud est petit.
- **Couche « cloud » séparée du cœur** : inscription, facturation (Stripe), choix de l'offre, création et mise en veille des instances, quotas de stockage, tableau de bord.
  - C'est un service à part, `mementum-cloud`, qui pilote les instances par l'API d'administration d'instance. Le cœur ne contient aucun code de facturation et reste identique à la version auto-hébergée.
  - Ce service peut rester privé : c'est un programme distinct qui ne fait que parler à l'API, donc l'AGPL ne l'impose pas. Le publier aussi renforcerait la confiance.
- **Données en Europe** (Lenny est en Belgique) : hébergeur européen (Hetzner, Scaleway, OVH…), RGPD, contrat de sous-traitance (DPA), CGU et politique de confidentialité, pas de traceurs tiers. L'instance d'un client n'est lue par personne sans son accord explicite (accès support journalisé).
- **IA dans le cloud** : par défaut, le client branche sa propre clé (Claude, GPT, Mistral…). En option, des crédits IA inclus via la passerelle, refacturés à prix coûtant plus une marge transparente. Les agents et le MCP fonctionnent à l'identique.
- **À décider plus tard** : les prix (par organisation, par siège, ou paliers de stockage), une offre gratuite limitée en stockage ou non, l'hébergeur précis, la structure juridique pour facturer.

---

## 8. Organisation du code (monorepo)

```
mementum/
├── crates/                    # Rust
│   ├── mementum-server/         # Axum : HTTP, WebSocket, fichiers statiques embarqués
│   ├── mementum-core/           # domaine : pages, bases, membres, opérations, validation
│   ├── mementum-sync/           # docs yrs, journal d'opérations, protocole, compaction
│   ├── mementum-crdt/           # façade CRDT (yrs aujourd'hui, Loro possible)
│   ├── mementum-store/          # sqlx SQLite/Postgres, migrations
│   ├── mementum-acl/            # permissions (tuples + héritage + cache)
│   ├── mementum-query/          # filtres → SQL, tables virtuelles SQL       (→ WASM aussi)
│   ├── mementum-formula/        # langage de formules                        (→ WASM aussi)
│   ├── mementum-md/             # MMD ⇄ arbre de blocs ⇄ Yjs                (→ WASM aussi)
│   ├── mementum-schema/         # registre unique des types de blocs et propriétés
│   ├── mementum-search/         # Tantivy + vecteurs
│   ├── mementum-ai/             # passerelle LLM, agents, exécutions, budgets
│   ├── mementum-mcp/            # serveur MCP
│   └── mementum-cli/            # binaire `mementum`
├── apps/
│   ├── web/                   # React + Vite + BlockNote (PWA)
│   └── desktop/               # Tauri (P5)
├── packages/
│   ├── protocol/              # types générés depuis Rust (ts-rs / specta)
│   ├── sdk-ts/                # SDK TypeScript (Apache-2.0)
│   └── ui/                    # composants partagés
├── spec/mmd/                  # spécification MMD + corpus de tests d'aller-retour
├── evals/agents/              # suite d'évaluation des éditions par agents (§10)
└── docs/
```

**Le point clé : `mementum-schema`.** Il définit **une seule fois** chaque type de bloc et de propriété. Il en dérive les specs BlockNote côté client, le convertisseur MMD, la validation serveur et la doc des outils MCP. Ainsi l'éditeur, le format et l'API ne peuvent pas diverger.

---

## 9. Feuille de route

| Phase | Nom | Contenu | Critère de fin |
|---|---|---|---|
| **P0** | Spike de validation (2–3 semaines, jetable) | BlockNote + Yjs ⇄ serveur yrs ⇄ SQLite. Aller-retour MMD sur 20 types de blocs. Édition d'un agent (`str_replace`) en pair CRDT pendant qu'un humain tape. Base de 100 000 lignes JSON + index sur un Pi 4. Re-benchmark Yjs / Loro sur une page de 10 000 blocs | Les 4 mesures sont chiffrées et les choix du §5.2 confirmés ou corrigés |
| **P1** | « Écrire ensemble » (MVP) | Instance, espaces (plusieurs par compte, membres et invités, pages privées et partagées), comptes locaux + passkeys. Arbre de pages, éditeur avec les blocs de base, temps réel et présence, commentaires, historique simple, corbeille, recherche plein texte. **Import d'un vault Obsidian et des pages Notion** (§4.6), export `.mementum`. **MCP v1** (search, fetch, create_pages, update_page, manage_pages, workspace_info) avec jetons personnels, limitables à un espace ou à des pages. Image Docker | Lenny et 3 personnes l'utilisent une semaine à la place de Notion, et Claude Code édite une page pendant qu'un humain tape, sans perte |
| **P2** | « Bases de données » | Bases et sources, propriétés (relations, rollups, sous-ensemble des formules), vues tableau, kanban, liste, galerie, calendrier, filtres, tris et groupes, vues liées, templates, import CSV, **import des bases Notion**. Outils MCP des bases (query SQL compris), API REST | Une base de 100 000 lignes respecte les budgets du §5.10 sur un VPS à 2 vCPU |
| **P3** | « Organisations » | Permissions fines, groupes, espaces d'équipe, membres restreints, sous-pages plus fermées que leur parent, liens publics, publication web. **Suggestions**, boîte de réception, frise d'historique. OIDC + SAML, journal d'audit, panneau d'admin | Une organisation de 50 personnes avec des invités, migrée depuis Notion |
| **P4** | « Agents » | Passerelle IA, agents d'espace (identité, parrain, politiques, jetons d'intégration), mode suggestion par défaut, annulation par exécution, Q&R avec citations, propriétés IA, déclencheurs. OAuth MCP + CIMD, recherche hybride, contrôle de la règle des deux | La suite d'évals (§10) atteint ≥ 90 % de réussite avec Claude et ≥ 70 % avec un modèle local via Ollama |
| **P5** | « Mementum 1.0 » | Vues frise, graphique, carte, formulaire, fil et tableau de bord. Formules complètes, boutons, automatisations, webhooks, permissions par ligne. Hors ligne complet, appli Tauri, PWA mobile soignée. Passe de performance, Postgres + S3 | Parité avec le cœur de Notion (§4.5) et budgets tenus |
| **Cloud** | « Mementum Cloud » (en parallèle de la P4) | Service `mementum-cloud` : inscription, Stripe, création et mise en veille des instances, sauvegardes S3, domaines personnalisés, tableau de bord, bouton « Passer en auto-hébergement ». Pages légales (CGU, confidentialité, DPA). Bêta fermée dès que la P3 est stable | 10 organisations bêta hébergées pendant un mois sans incident, et une migration cloud → auto-hébergé réussie de bout en bout |
| **+** | Après la 1.0 | Mobile natif, fédération (Mementum Link), bases synchronisées depuis des API externes, notes de réunion (Whisper local), mode présentation, permissions par colonne, SCIM, tableau blanc | — |

**Avancement de la P1** (10 novembre 2026). La phase est découpée en 7 lots : socle et édition en direct ; comptes, espaces, droits et corbeille ; collaboration et blocs riches ; recherche ; agents ; imports et export ; livraison. Le lot 1 ([plan](../superpowers/plans/2026-11-09-p1-lot1-socle-edition.md), [registre](../superpowers/logs/2026-11-09-p1-lot1-ledger.md)) est fait : ses 10 tâches sont closes, et la relecture finale de toute la branche est faite, corrections comprises. Il livre 7 crates (`mementum-schema`, `-md`, `-crdt`, `-store`, `-sync`, `-server`, `-cli`) et le client `apps/web` : l'arbre de pages, l'édition à plusieurs en direct et le format MMD de la P1 (§6.2), sans comptes ni droits. Limites connues laissées par ce lot :
- **R39** : une édition faite sur un bloc pendant qu'un autre client le déplace est perdue (§11) ; à traiter au lot 3 ;
- **R40** : la réécriture complète d'une page de 10 000 blocs bloque l'acteur de la page 2,5 à 4 s selon la machine et sa charge (§5.10) ; remède prévu au lot 5, une écriture MMD par différence ;
- **R41** : `PUT /mmd` refuse une prop hors registre (§6.2, « Limites connues ») ; à lever quand le schéma du client saura la porter ;
- **R43** : l'ouverture d'une page ralentissait avec le nombre de pages affichées dans l'arbre ; **corrigé au lot 2** : l'arbre ne met plus dans le document que ses lignes visibles (§5.10) ;
- **R44** : deux avis RustSec sont ouverts sans mise à jour possible : RUSTSEC-2026-0459 (`libsqlite3-sys` 0.37.0, bloqué par `sqlx` 0.9.0) et RUSTSEC-2026-0215 (`smallstr`, non maintenu, tiré par `yrs` 0.28.1). Le premier ne touche qu'un processus qui ouvre une base écrite par un tiers ; le serveur n'ouvre que la sienne. La CI ne contrôle pas encore les avis (`cargo deny check advisories` à la main) ; à revoir avant toute fonction qui ouvre une base reçue d'ailleurs, et au lot 7.

Le lot 2, « Comptes, espaces, droits et corbeille » ([plan](../superpowers/plans/2026-11-10-p1-lot2-comptes-espaces-droits.md), [registre](../superpowers/logs/2026-11-10-p1-lot2-ledger.md)), est livré le 10 novembre 2026 ; la recette du propriétaire se déroule avec la [checklist](../recette/2026-11-10-p1-lot2-recette.md). Il ajoute la crate `mementum-acl` (qui voit quoi dans un espace) et la migration `0002`, qui remplace organisations et coffres par des espaces. Il livre :
- des comptes locaux : mot de passe haché en Argon2id, session par cookie, 30 jours au plus après la connexion (Ruling R117) ; le premier compte administre l'instance ; inscriptions sur invitation par défaut (Ruling R50, à confirmer) ; mot de passe oublié par `mementum user set-password`, faute de SMTP (Ruling R47) ;
- des espaces façon Notion (demande d'Alex, à confirmer avec Lenny, Ruling R45), plusieurs par compte, avec les rôles propriétaire, admin, membre et invité, et des invitations par lien, dans l'espace ou sur une page (§4.4) ;
- des pages d'espace, partagées et privées, en trois sections calculées ; le partage d'une page et son accès général, par le menu Partager (§4.3, §4.4) ; le déplacement des pages, à la souris et au clavier (§4.2) ;
- les droits vérifiés à chaque appel, synchronisation comprise : une connexion ouverte est fermée dès qu'une écriture change ses droits, et ne revient qu'avec ses nouveaux droits (§5.6). Seule exception, sans risque : des droits qui montent quand la personne accepte une invitation ne ferment rien, et valent à sa prochaine connexion ;
- la corbeille : une page y part avec ses sous-pages et en revient avec elles ; à la corbeille, elle se lit et ne s'écrit plus ; on ne la supprime pour de bon que depuis la corbeille ;
- la migration d'une base du lot 1 sans perte : copie de la base d'abord (§7.4), puis ses pages vont dans « Mon espace », que reprend le premier compte ;
- deux reports du lot 1 : l'arbre fenêtré (R43) et les budgets du navigateur mesurés sur un build de production et un serveur `--release` (R38). Les budgets d'ouverture d'une page (§5.10) tiennent, part du serveur comprise. Ce sont les seuls que ce lot a mesurés sur un build de production ; les mesures sont au registre du lot.

Limites connues laissées par le lot 2 :
- **R76** : dix mots de passe faux bloquent une adresse, même pour son titulaire, pendant 15 minutes au plus (le quart d'heure court depuis le premier échec compté) ; un tiers peut donc l'empêcher de se connecter (ses sessions ouvertes continuent) ;
- **R77** : l'inscription dit si une adresse a un compte ;
- **R84** : sur une instance sur invitation, quelqu'un du dehors peut savoir si une adresse a un compte. Par lots, il lui faut une douzaine de requêtes par adresse (`11 × N + 10 000` pour N adresses) ; pour une seule adresse, plus de 10 000. Les limites de débit par adresse IP du lot 7 (Ruling R54) lèveront R76 et R84 ; elles freineront R77 sans le lever : là où l'inscription est ouverte, `409 email_taken` reste un indice en une requête ;
- **R109** : la présence n'est pas encore vérifiée en entier par le serveur. Personne ne touche plus au nom ni au curseur de quelqu'un qui est là (Ruling R101). Mais pendant la coupure d'une personne, quelqu'un qui peut modifier la page peut la priver de nom et de curseur chez les autres, tant qu'il reste là, ou jusqu'à ce que la page se vide (un simple lecteur ne le peut plus, Ruling R118) ; et après une reconnexion, la présence d'une personne inactive peut mettre jusqu'à 30 s à revenir. Ni le contenu ni les droits ne sont en jeu (§5.6). La présence vérifiée par le serveur est prévue au lot 3 (Ruling R52) ;
- **R97** : la barre latérale ne suit pas en direct les pages créées, renommées ou déplacées par d'autres ; elle est relue quand l'onglet revient au premier plan, au plus une fois toutes les cinq secondes. Le direct viendra avec le journal d'opérations (canal `space:<id>`, §5.4), au lot 3 ;
- **R86** : un invité en accès complet sur une page peut y inviter d'autres personnes (à confirmer, §4.4) ;
- **R117** : dans un navigateur, une session dure au plus 30 jours après la connexion. Le serveur prolonge la session en base à l'usage, mais ne renvoie pas le cookie, qui s'éteint 30 jours après avoir été posé. Le renvoi du cookie est reporté ;
- **R121** : un compte autorisé, même en lecture seule, peut encore surcharger le serveur (connexions de synchro sans plafond par compte ni par page, file d'une connexion qui ne lit pas). Ces plafonds vont au lot 7, avec les autres abus d'un compte autorisé (Ruling R54). D'ici là, n'inviter que des personnes de confiance ;
- `mementum user set-password` tourne dans un autre processus que le serveur : il ferme les sessions du compte, pas ses connexions de synchro déjà ouvertes, qui restent en lecture et en écriture jusqu'à leur prochaine reconnexion (Rulings R73 et R125) : pour chasser quelqu'un, redémarrer le serveur après la commande ; il ne lève pas non plus le frein des connexions, gardé en mémoire ;
- **R46** : pas de passkeys au lot 2. Le §4.5 les place en P3, le tableau ci-dessus en P1 : la contradiction reste à trancher ;
- **R52** : hors du lot, suppression d'un espace ou d'un compte, création d'espaces réservée par l'admin d'instance, inscription par domaine e-mail, vidage automatique de la corbeille ;
- **R53** : la migration `0002` porte tout le lot, et a changé une fois en cours de route. Une base du lot 1 est migrée à sa première ouverture, après copie (§7.4). Une base ouverte par un binaire intermédiaire du lot 2 peut refuser de démarrer : repartir alors de la copie d'avant migration.

---

## 10. Tests et qualité
- **Aller-retour MMD** : tests de propriétés (proptest) arbre → MMD → arbre sur des arbres aléatoires, plus un corpus de pages réelles. **Bloquant pour chaque version.**
- **Fidélité des imports** : un corpus de vaults Obsidian réels (liens, intégrations, callouts, frontmatter, pièces jointes, liens cassés) et d'exports Notion (zip et API, avec bases liées). On compare page par page avec l'original et on vérifie que le rapport d'import liste bien tout ce qui n'a pas été converti.
- **Convergence CRDT** : fuzzing de sessions concurrentes humains + agents + coupures réseau, avec vérification que tous les clients convergent vers le même état.
- **Suite d'évals agents** (`evals/agents/`) : 150 à 200 tâches d'édition réalistes (corriger, insérer, restructurer, remplir une base…), lancées sur Claude, GPT et 2 à 3 modèles Ollama. Elle compare `str_replace`, les opérations par ancre et le remplacement de bloc, et mesure la réussite, les tokens et les dégâts collatéraux. Elle **fixe le format MMD v1** avant la P4, car aucun benchmark public n'existe sur l'édition de documents riches.
- **Tests différentiels** du moteur de requêtes : SQLite et Postgres doivent donner les mêmes résultats.
- **Vecteurs de test des formules**, calés sur le comportement documenté de Notion.
- **Matrice de permissions** : tests générés (sujet × action × objet × héritage), plus un corpus d'**injections de prompt** rejoué contre les agents intégrés.
- **E2E Playwright** multi-clients : deux navigateurs et un agent sur la même page.
- **Performance en CI** sur un runner ARM de classe Pi, avec échec si un budget du §5.10 est dépassé.

---

## 11. Risques et parades

| Risque | Parade |
|---|---|
| Périmètre énorme (Notion = des centaines de personnes) | Phases livrables, P1 utile seule, IA et bases ensuite. On ne court pas après les fonctions annexes (Mail, Calendar) |
| BlockNote encore en 0.x (API qui bouge) | `mementum-schema` comme couche d'isolation ; repli possible sur Tiptap nu |
| **BlockNote passe à Yjs 14** (défaut en 0.57) alors que yrs 0.28 ne lit pas le texte des nœuds Yjs 14 *(trouvé en P0)* | Rester sur le chemin Yjs 13 (`@blocknote/core/yjs`) ; suivre le support Yjs 14 de yrs ; façade `mementum-crdt` |
| **Déplacement concurrent d'un bloc = bloc dupliqué et éditions perdues** avec Yjs *(confirmé en P0 : 25 doublons sur 25 ; perte des éditions relevée à la relecture ; **perte toujours ouverte après le lot 1 de la P1**)* | Doublons : passe de déduplication du serveur (lot 1). Éditions perdues : risque ouvert, test `edit_during_concurrent_move_survives` ignoré. Le report par le serveur depuis l'original gardé par `skip_gc` a été essayé et écarté au lot 1 : yrs ne laisse relire ni l'id ni les enfants d'un bloc supprimé, sauf à rematérialiser la page (ce n'est qu'un coût, sur un chemin rare) ; surtout, l'original mêle ses anciennes suppressions à celles du déplacement (un diff vers la copie ressuscite du texte effacé), et `skip_gc` garde toute page réécrite, pour toujours : +3,1 Mo et environ +80 ms de chargement par réécriture d'une page de 10 000 blocs, 300 ms dépassés après 3 réécritures. Ce coût est permanent et se paie à chaque chargement, alors que celui du ramasse-miettes est passager : il se paie une fois, à l'écriture, puis une compaction l'efface des chargements suivants (Ruling R40, §5.10). Deux pistes à comparer au lot 3 (Ruling R39) : (a) déplacements interceptés côté client et faits par le serveur (opération dédiée sur l'état fusionné) ; (b) le serveur enregistre la correspondance original → copie à la réception du déplacement d'un client, quand l'original est encore vivant et que la mise à jour dit quelles suppressions appartiennent au déplacement. Dans les deux cas : correspondance persistée, pour reporter sur la copie les éditions arrivées après le déplacement, fusion à trois voies pour l'ordre « édition puis déplacement », et aucune double application après un rechargement. Déplacements de pages faits par le serveur |
| Pages géantes lentes à ouvrir (10 000 blocs ≈ 1,3 s en P0 ; le coût est la liaison y-prosemirror, pas Yjs, qui charge en 34 ms) | Virtualisation de l'affichage ou découpage des pages géantes, sinon budget revu pour elles ; le cache local (§5.9) n'aide que pour une page déjà vue |
| Bases JSON sur SQLite trop lentes à 100 000 lignes sur un Pi | Budgets tenus sur PC en P0 ; **mesure ARM à faire** (risque : filtre + tri) ; rollups matérialisés ; table typée par source en repli |
| **Fidélité MMD ⇄ Yjs** *(rouvert à la relecture de la P0)* | Sans perte pour le sous-ensemble modélisé (20 types, 12 800+ cas proptest MMD ⇄ arbre). Pertes silencieuses hors modèle (§6.2, « Limites connues ») : `mementum-schema` modélise toutes les props ou porte un sac de props opaque, syntaxe d'enfants pour tout bloc, `<br/>`, nœuds inconnus transportés tels quels ; proptest qui traverse aussi la couche Yjs. **Limite du lot 1 de la P1** (Ruling R41) : le sac de props est lu et rendu, mais refusé à l'écriture par `PUT /mmd` (422), car y-prosemirror retire du document partagé tout attribut que le schéma du client ne déclare pas dès qu'un navigateur modifie le bloc ; à lever quand un porteur durable existera (attribut déclaré au schéma du client) |
| **Client plus ancien qui efface les blocs d'une version plus récente** (y-prosemirror supprime du document partagé tout nœud ou marque inconnus) *(relevé à la relecture de la P0)* | Versionnage du schéma ; un client dont le schéma est plus ancien que celui de la page passe en lecture seule |
| **Racine de page créée par un client avant la première synchro** (rendu local d'abord, §5.9) | Racine déterministe (identifiant client et horloge fixes) ou édition bloquée jusqu'à la synchro ; e2e « taper avant la synchro » |
| Erreur « RangeError » de y-prosemirror après une fusion distante, qui peut effacer un changement distant *(à confirmer)* | Reproduire en P1, suivre les versions de y-prosemirror, surveiller le message dans les e2e |
| ~~Choix CRDT~~ | Levé en P0 : Yjs (règle de décision amendée à la relecture, `spike/RESULTS.md`, notes T7) |
| Moteur de formules complexe | Sous-ensemble en P2, complet en P5, vecteurs de test |
| Sécurité IA (injection, exfiltration) | Contenu en attente inerte, règle des deux, permissions côté serveur, agents sans accès par défaut |
| Abandon (risque n°1 des projets open source) | Pile courte, une seule langue serveur, doc soignée, DCO, gouvernance ouverte dès que la communauté existe |

---

## 12. Décisions prises et part laissée au constructeur

### Choix de Lenny
- **Nom : Mementum** (memento + momentum). Lenny voulait « Memento », mais **Memento Database** (MementoDB Inc.) est une app de bases de données et de notes à environ 1,4 M d'installations : même catégorie. On a donc choisi un nom proche et libre.
  - **Libres au 09/11/2026** :
    - npm `mementum` et le scope `@mementum` ;
    - crates `mementum` ;
    - domaines `mementum.app` et `mementum.dev` ;
    - organisation GitHub `mementumhq` et domaine `mementumhq.com`.
  - **Déjà pris** : `mementum.com`, `mementum.org` et le compte GitHub `mementum`.
  - **Homonymes, hors de notre catégorie** : un mémo vocal pour Mac (2013) et un programme santé iOS d'entraînement de la mémoire (Cogniss, 2025). Risque faible.
- **Direction visuelle : A « Papier »** (§4.3).
- **Mementum autonome, sans miroir en fichiers** : on entre par l'**import Notion et Obsidian** dès la P1 (§4.6), on sort par l'export.
- **Serveur MCP dès la première version** (P1), complété jusqu'en P4.
- **Licence AGPL-3.0** pour le serveur et l'application web, avec **Apache-2.0** pour le SDK et la spec MMD, **DCO sans CLA**, aucune fonction payante (§3.1).
- **Modèle économique** : auto-hébergement gratuit avec toutes les fonctionnalités, et **Mementum Cloud**, une offre hébergée payante pour ceux qui ne veulent pas gérer de serveur (§7.6).

### Demande d'Alex (2026-11-09)
- **Espaces façon Notion**, à la place des organisations et des coffres (§4.1, §4.4). Sa demande : se faire plusieurs espaces (« Perso », « Pro »), demander à Claude Code de verser du contenu dans l'un d'eux, inviter des gens dans un espace ou sur une page de l'espace, ouvrir des accès d'API sur un espace ou sur des pages, « vraiment comme Notion ».
  - Les **coffres façon Obsidian** étaient une idée de Lenny, validée sur le principe. Il en reste les conventions d'Obsidian (liens `[[…]]`, rétroliens, vue graphe, dossiers, frontmatter, callouts, tags), le modèle d'espace « LLM wiki » et l'absence de miroir en fichiers.
  - **À confirmer avec Lenny** (§13).
  - **Choix de rédaction** faits pour traduire la demande, à relire :
    - l'espace remplace aussi l'organisation, pas seulement le coffre ;
    - les trois sections « Espace », « Partagé », « Privé », déduites des accès ;
    - le premier espace créé à l'inscription, et la création d'espaces que l'admin d'instance peut réserver ;
    - l'admin d'instance ne lit pas, par l'application, les pages d'un espace dont il n'est pas membre (§5.6) ;
    - la répartition par phases (§4.5) : invités sur une page et jetons personnels limités à un espace ou à des pages dès la P1 ; groupes, espaces d'équipe et sous-pages plus fermées en P3 ; jetons d'intégration en P4.

### Mes choix (à relire)
1. **Serveur Rust en binaire unique**, SQLite par défaut et Postgres en option.
2. **Mementum Cloud** : une instance par client (SQLite + Litestream + S3), couche de facturation séparée du cœur, données en Europe.
4. **Architecture hybride** : Yjs/yrs pour le texte des pages, journal d'opérations dont le serveur fait autorité pour le reste.
5. Éditeur **BlockNote**, client **React + Vite**, cache **SQLite WASM/OPFS**.
6. Lignes de base en **JSON + index d'expression + table d'arêtes**, avec repli prévu.
7. **Tantivy** pour la recherche, plutôt que FTS5 ou tsvector.
8. **Format MMD** aligné sur le Markdown de Notion, ancres `^bN` façon Obsidian.
9. **14 outils MCP**, mode **suggestion par défaut** pour les agents, contenu IA en attente inerte.
10. **Pas de fédération en v1.**

### Laissé au constructeur
- La bibliothèque d'index vectoriel, le rendu de la grille (DOM virtualisé ou canvas), le design system de l'interface.
- La grammaire exacte du langage de formules (objectif : compatibilité avec les formules Notion), les détails du protocole binaire.
- La politique de compaction et de rétention des instantanés CRDT.

---

## 13. Questions ouvertes pour Lenny
- **Espaces façon Notion à la place des coffres façon Obsidian** (demande d'Alex du 2026-11-09, §4.4 et §12) : à confirmer. Le lot 2 de la P1, livré le 2026-11-10, est bâti sur ce modèle (Ruling R45).

Aucune autre question bloquante pour commencer. À trancher avant d'ouvrir Mementum Cloud (§7.6) : les prix, une éventuelle offre gratuite, l'hébergeur et la structure juridique pour facturer.
