# Mementum — Document de conception

> **Statut** : proposition. Nom (**Mementum**) et direction visuelle (**A « Papier »**) choisis par Lenny ; le reste est à valider
> **Date** : 2026-11-09 (d'abord « Vivace », puis « Memento », écarté parce que Memento Database existe déjà ; finalement « Mementum »)
> **Rédigé par** : Claude Code, à partir de 4 recherches (fonctionnalités Notion 2026, alternatives open source, architecture temps réel, accès des agents IA). La synthèse est dans [`reports/Conception alternative Notion open source.md`](../../../reports/Conception%20alternative%20Notion%20open%20source.md) et les notes sourcées dans [`research_notes/`](../../../research_notes/Conception%20alternative%20Notion%20open%20source/).

---

## 0. En une minute

**Mementum** est un espace de travail open source et auto-hébergeable, au niveau de Notion : pages en blocs, bases de données relationnelles, vues, équipes et permissions, édition à plusieurs en direct. Le contenu est rangé dans des **coffres** à la manière d'Obsidian, chacun privé ou partagé. Mementum est **conçu dès le départ pour que les agents IA lisent et écrivent les pages aussi proprement que les humains**.

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
Aucune alternative ne réunit les quatre critères suivants : **vraie base relationnelle dans les pages**, **temps réel**, **organisations et permissions**, **auto-hébergement léger sous licence libre sans plafond**.

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
- Parité avec le **cœur** de Notion : pages et blocs, bases avec les 24 types de propriétés, vues, relations, rollups et formules, coffres partagés (équivalent des teamspaces), commentaires, historique, suggestions.
- **Temps réel** partout : contenu des pages, arborescence, lignes de base, présence.
- **Performance mesurée** contre des budgets chiffrés (§5.10), vérifiés en CI, y compris sur machine ARM type Raspberry Pi.
- **Plusieurs organisations sur un même serveur** : n'importe qui monte son instance et y héberge sa famille, son asso ou son entreprise.
- **Agents IA de première classe** : format canonique, MCP intégré, identité, permissions, journal, mode suggestion, annulation par exécution.
- **IA indépendante du fournisseur** : Claude, GPT, Mistral… ou 100 % local via Ollama. Rien n'est obligatoire.
- **Autonomie et portabilité** : **import complet depuis Notion et depuis un vault Obsidian dès la P1** (§4.6), plus Markdown et CSV. Export complet sans perte et réimportable (format `.mementum`). Mementum se suffit à lui-même : on y entre par l'import, on en sort par l'export.

### Non-objectifs (v1)
- Notion Mail et un vrai Calendar (Notion a d'ailleurs fermé Mail le 22/09/2026).
- Apps mobiles natives : PWA d'abord.
- Fédération entre serveurs : chaque instance est indépendante en v1 (§7.5).
- Miroir des coffres en fichiers `.md` synchronisés en continu avec Obsidian ou git. **Décision de Lenny** : Mementum est autonome, on passe d'un outil à l'autre par l'import et l'export, pas par une synchro permanente.
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
└── Organisation (ex. « Famille », « Asso », « Entreprise »)  ← plusieurs par instance
    ├── Membres, groupes, invités, agents
    └── Coffres (§4.4) : « Mon coffre » privé de chaque membre + coffres partagés
        └── Dossiers et pages (arbre)
            ├── Blocs (texte, titres, listes, toggles, callouts, code…)
            ├── Sous-pages
            └── Bases de données (pleine page ou en ligne)
                ├── Sources de données (schéma + lignes ; une base peut en avoir plusieurs)
                ├── Vues (tableau, kanban, calendrier…)
                └── Lignes = pages (propriétés + contenu)
```
Le **coffre** remplace à la fois l'espace d'équipe (teamspace) et l'espace privé de Notion : c'est l'unité de rangement et de partage.

- **Admin d'instance** : gère les inscriptions (ouvertes, sur invitation, par domaine e-mail), les quotas, les fournisseurs IA autorisés, les sauvegardes et le SMTP.
- **Rôles d'organisation** : Propriétaire, Admin, Membre, Membre restreint, Invité. Les agents forment une catégorie à part (§6.7).
- **Niveaux de permission sur une page** : Accès complet · Peut modifier · Peut modifier le contenu (bases) · Peut créer (bases) · Peut commenter · Peut lire.
- **Accès général** : seulement les invités · toute l'organisation · toute personne ayant le lien (avec expiration et mot de passe en option) · publié sur le web.

### 4.2 Écrans principaux
- **Barre latérale** : sélecteur d'organisation, recherche, Accueil, Boîte de réception, **Agents** (exécutions en cours, suggestions en attente), **Vue graphe**, Favoris, puis la section **Coffres** (chaque coffre avec son niveau de partage : Privé, Organisation, ou nombre de membres) et son arbre de pages, Modèles, Corbeille, Réglages.
- **Page** : couverture, icône, titre, propriétés si c'est une ligne de base, éditeur en blocs, rétroliens, commentaires dans la marge, avatars des présents (agents compris, avec un badge 🤖).
- **Base de données** : onglets de vues, filtres, tris et groupes, recherche interne, ouverture des lignes en panneau latéral, au centre ou en pleine page.
- **Palette de commandes** (Ctrl+K) : navigation, actions, et questions à l'IA avec citations.
- **Revue des suggestions** : comparaison par exécution d'agent, avec accepter ou refuser bloc par bloc ou en bloc.
- **Historique** : frise des versions avec attribution humain ou agent, et restauration en un clic.
- **Réglages** : organisation (membres, groupes, sécurité, IA, agents, journal d'audit) et instance (admin).

### 4.3 Direction visuelle retenue : A « Papier » *(choix de Lenny)*
Maquettes : canvas interactif https://claude.ai/artifact/5VPJk8gPwX1cMdwufLHTXw (artboard « A · Papier »). Le principe est de **ne pas dépayser quelqu'un qui vient de Notion** : mêmes emplacements, mêmes gestes, identité propre.
- **Mise en page** :
  - barre latérale claire (fond `#F6F5F1`, bordure `#E8E6E1`, éléments de 30–32 px) ;
  - page blanche centrée (≈ 760 px max), bandeau de couverture et icône de page en tuile, titre en 38 px gras ;
  - propriétés en liste « libellé / valeur » sous le titre.
- **Typographie et couleurs** :
  - **Inter** partout ;
  - texte `#2F2E2B`, texte secondaire `#66655F` (contraste ≥ 4,5:1) ;
  - **accent vert `#23704F`**, réglable par organisation ;
  - pastilles de statut en couleurs douces.
- **Barre du haut** : fil d'Ariane, « Modifié il y a… », avatars des présents (l'agent a une pastille ✦ à la couleur d'accent), bouton **Partager**, menu « … ».
- **Menu Partager** (déroulant en haut à droite, comme Notion), de haut en bas :
  1. onglets Partager / Publier ;
  2. bascule **« Tout le coffre » / « Cette page seulement »** ;
  3. champ d'invitation (personnes, groupes, agents) ;
  4. liste des accès avec leur niveau (l'agent porte un badge « Agent » et le nom de son parrain) ;
  5. **Accès général** en 3 choix : seulement les invités / toute l'organisation / toute personne ayant le lien ;
  6. « Copier le lien ».
- **Collaboration visible** : curseur nommé de chaque personne. Une suggestion d'agent apparaît surlignée dans le texte, avec une petite barre « Scribe (agent) propose un ajout · Accepter · Refuser ». Après décision : « Suggestion acceptée · Annuler ».
- **Bases** : en-tête avec onglets de vues (Tableau, Kanban, Calendrier) et bouton « Nouveau » à la couleur d'accent ; tableau sobre à lignes fines.
- **Thème sombre** : dérivé du même système, sans changer la mise en page. Les directions B (« Atelier ») et C (« Nuit ») restent des sources d'idées : panneau d'activité des agents, barre IA flottante Ctrl+K.

### 4.4 Coffres façon Obsidian *(idée de Lenny)*
- **Chaque membre a « Mon coffre »**, privé par défaut. Une organisation crée autant de coffres partagés qu'elle veut (« Wiki interne », « Projet X », « Direction »…).
- **Partage d'un coffre** :
  - avec **toute l'organisation**, avec des **groupes** ou avec **certaines personnes** (et des agents) ;
  - niveaux : Accès complet · Peut modifier · Peut commenter · Peut lire, et pour les agents `suggérer` ;
  - toutes les pages du coffre héritent de ces accès ;
  - une page peut avoir une **exception** (plus ouverte ou plus fermée) via « Cette page seulement ».
- **L'expérience Obsidian** :
  - liens `[[Page]]` et `[[Page#Titre]]`, **rétroliens**, **vue graphe** du coffre ;
  - dossiers ;
  - frontmatter YAML = propriétés de la page ;
  - callouts `> [!tip]`, références de bloc `^b4`, tags `#tag`.

  Le format MMD (§6.2) est déjà du Markdown compatible avec ces conventions.
- **Modèle de coffre « LLM wiki »**, prêt à l'emploi, inspiré du vault de Lenny :
  - `raw/` pour les sources jamais modifiées ;
  - pages tenues par l'IA ;
  - `index`, `log` (journal) et page de schéma (`CLAUDE.md`).

  Un agent d'espace s'en occupe : ingestion, réponses aux questions, vérification. Il est en mode suggestion par défaut.
- **Pas de miroir en fichiers** (décision de Lenny) : un coffre vit dans Mementum. On arrive avec l'import d'un vault Obsidian (§4.6) et on repart avec l'export. Les agents comme Claude Code passent par le MCP (§6.5) ou la CLI, pas par les fichiers.

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
| Organisations, coffres privés et partagés (organisation entière ou personnes), `[[liens]]` et rétroliens, comptes locaux | P1 |
| Partage de coffre par groupe, exceptions par page, invités, membres restreints, vue graphe | P3 |
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

### 4.6 Importer depuis Notion et Obsidian *(exigence de Lenny)*
Mementum est autonome : il n'y a **pas de synchro permanente** avec un autre outil. On importe une fois, proprement, puis on travaille dans Mementum.

**Chaque import :**
- crée un **nouveau coffre** (ou remplit un coffre choisi) ;
- produit un **rapport d'import** qui liste ce qui a été converti, approximé ou ignoré, avec un lien vers chaque page concernée ;
- est **rejouable** : réimporter met à jour les pages déjà importées (l'identifiant d'origine est gardé) au lieu de créer des doublons, ce qui permet une bascule progressive ;
- tourne en tâche de fond avec une barre de progression. On le lance depuis l'interface (glisser-déposer d'un zip ou d'un dossier) ou depuis la CLI : `mementum import obsidian <dossier>`, `mementum import notion <zip>` ou `mementum import notion --token`.

**Vault Obsidian** (dossier ou zip) :

| Dans Obsidian | Dans Mementum |
|---|---|
| Dossiers | Dossiers du coffre |
| Notes `.md` | Pages (converties en MMD) |
| Frontmatter YAML | Propriétés de la page (type deviné : date, nombre, liste, case à cocher, texte) |
| `[[Note]]`, `[[Note\|alias]]`, `[[Note#Titre]]`, `[[Note#^bloc]]` | Liens vers la page, le titre ou le bloc, résolus comme Obsidian : par nom unique, sinon par chemin |
| `![[image.png]]`, `![[Note]]` | Image ou fichier ; une note intégrée devient un bloc synchronisé (un simple lien en cas de boucle) |
| Callouts `> [!tip]`, `> [!warning]`… | Callouts (le type donne l'icône et la couleur) |
| `#tags` (dans le texte ou le frontmatter) | Propriété « Tags » en multi-sélection, plus la recherche |
| Références de bloc `^id` | Ancres de bloc conservées |
| Pièces jointes | Fichiers du coffre (dédoublonnés) |
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
- teamspaces → coffres partagés. Les accès sont recopiés quand les personnes existent dans l'organisation ; sinon, des invitations sont proposées ;
- pages privées → « Mon coffre » de la personne qui importe ;
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
│ ├─ Sync : docs Yjs (yrs) par page + journal d'opérations par organisation  │
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
| Contenu des pages | **Yjs** (client) + **yrs** (serveur, feature `sync`), derrière une façade `mementum-crdt`. **Confirmé par la P0** (`spike/RESULTS.md`). Côté BlockNote, on passe par le chemin Yjs 13 (`@blocknote/core/yjs`). Le serveur écrit la racine d'une page neuve (sinon deux clients sur une page vide créent deux racines et l'une est perdue) | Standard de fait (Outline, Docmost, Plane, La Suite Docs, AFFiNE…), liaison éditeur mûre. P0 : Yjs 3 à 16 fois plus rapide que Loro dès qu'on lit toute la page. Passer à Yjs 14 quand yrs le comprendra (yrs 0.28 ne lit pas le texte des nœuds Yjs 14) | Loro (arbre déplaçable sans doublons, mais plus lent en lecture complète et en fusion ; plan B via la façade), Automerge |
| Structure et bases | **Journal d'opérations dont le serveur fait autorité** (façon Linear/Figma) : version monotone par organisation, application optimiste côté client puis rebase, dernier écrivain gagnant par propriété | Permissions, requêtes serveur sur 100 000 lignes, formules : impossible proprement en tout-CRDT. Aucun moteur existant n'accepte SQLite par défaut (Zero, Electric : Postgres uniquement ; PowerSync : FSL) | Zero, ElectricSQL, PowerSync, Triplit (à l'arrêt), tout-CRDT façon Anytype |
| Éditeur | **BlockNote** (cœur MPL-2.0) sur ProseMirror/Tiptap | Expérience Notion prête (menu `/`, poignées, imbrication) ; choisi par La Suite Docs (État français) | Tiptap nu (tout à construire), Lexical, BlockSuite (trop couplé à AFFiNE) |
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
orgs(id, slug, name, icon, settings_json)
members(org_id, user_id, role, status)                               -- owner|admin|member|restricted|guest
groups(id, org_id, name) · group_members(group_id, user_id)
agents(id, org_id, name, avatar, sponsor_user_id, policy_json)       -- §6.7
vaults(id, org_id, name, icon, color, owner_id, kind, settings_json)   -- personal|shared (§4.4)

nodes(id, org_id, vault_id, parent_id, kind, title, icon, cover,
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
ops_log(org_id, lsn, actor_id, run_id?, op_json, created_at)          -- tout changement structurel
agent_runs(id, agent_id, on_behalf_of?, trigger, model, status, cost, started_at, ended_at)
audit_log(id, org_id, actor_id, action, object, details_json, created_at)
files(hash, size, mime, storage_key) · attachments(node_id, file_hash)
api_tokens(id, user_or_agent_id, scopes, expires_at, hash)
automations(id, database_id, trigger_json, actions_json) · webhooks(id, org_id, url, events, secret_hash)
jobs(id, kind, payload_json, status, run_at, attempts)                -- file de tâches intégrée
```

Identifiants : **UUIDv7** (triables par date). Côté utilisateur et agent, on montre des alias courts (`pg_7Hq2Lm`), traduits par le serveur.

### 5.4 Synchronisation temps réel (hybride)
Le client ouvre **une seule WebSocket**, multiplexée, encodée en MessagePack :

| Canal | Contenu | Mécanisme |
|---|---|---|
| `doc:<page>` | Contenu riche de la page | Synchro Yjs (étapes 1 et 2), puis mises à jour incrémentales. *Awareness* pour les curseurs et la sélection |
| `org:<id>` | Arbre des pages, métadonnées, lignes de base, commentaires, notifications | **Pull** : « donne-moi tout depuis la version N », filtré par permissions. **Push** : diffusion en direct des nouvelles opérations |
| `mutate` | Écritures structurelles | Le client envoie `{client_mutation_id, name, args}`. Le serveur valide (permissions, schéma), applique, attribue un numéro de version et répond. L'opération est idempotente |
| `query:<view>` | Fenêtre d'une vue de base (lignes 0 à 200…) | Requête serveur, abonnement aux changements de la fenêtre |

- **Hors ligne** : les mutations restent en file dans SQLite (OPFS) et sont rejouées au retour. Pour les propriétés de base, le dernier écrivain gagne, propriété par propriété. Le texte fusionne via Yjs.
- **Chargement paresseux** : l'arbre se charge niveau par niveau ; une page est d'abord rendue depuis le cache, puis synchronisée ; les lignes de base arrivent par fenêtres.
- **Compaction** : un instantané Yjs est pris toutes les N mises à jour, puis le journal est purgé. Les instantanés d'historique sont conservés (§4.5).

### 5.5 Bases de données : stockage, requêtes, formules
- **Stockage** *(mon choix, confirmé par la P0 sur PC)* : `rows.props_json` (JSON typé selon le schéma), avec des **index d'expression** créés automatiquement sur les propriétés filtrées ou triées le plus souvent (SQLite JSON1 / Postgres JSONB + GIN). Les relations vont dans une table d'arêtes.
  - Mesures P0 sur 100 000 lignes (PC) : fenêtre triée 2,5 ms, filtre ET/OU + tri 127 ms, groupes 13 ms.
  - Les requêtes doivent reprendre **l'expression exacte de l'index** (un `CAST` faisait passer les groupes de 13 à 707 ms).
  - La table SQL typée par source ne gagne qu'environ 20 % : alternative abandonnée.
  - **Mesures ARM encore à faire.**
- **Rollups et formules : toujours matérialisés** (obligatoire depuis la P0). Un rollup calculé à la lecture prend 1,9 s sur 100 000 lignes en JSON et 0,6 s en table typée. Les valeurs vivent dans `computed_json` et sont recalculées par le graphe de dépendances décrit plus bas.
- **Moteur de requêtes** (`mementum-query`) : filtre (même JSON que l'interface) → SQL paramétré. Tris, groupes et pagination par curseur côté serveur. Pour une base petite et en cache, le même moteur tourne en WASM dans le client et répond hors ligne.
- **Formules** (`mementum-formula`) : analyseur, typage, puis évaluation compilée en bytecode. Le même code Rust tourne sur le serveur et en WASM dans le client, pour des résultats identiques partout. Les valeurs calculées sont stockées dans `computed_json` et invalidées par graphe de dépendances (propriété → formule → rollup → ligne liée).
- **SQL en lecture seule pour les agents** : chaque source de données est exposée comme une table virtuelle (`SELECT … FROM "Tâches"`), avec les permissions appliquées (lignes et colonnes filtrées). Notion propose la même chose avec du SQLite en lecture seule.

### 5.6 Permissions
- **Sujets** : utilisateur, groupe, invité, agent, lien public, « toute l'organisation ».
- **Objets** : organisation → coffre → dossier/page/base → source → ligne (→ colonne, plus tard).
- **Héritage** : une sous-page hérite de son parent, sauf surcharge. Le droit le plus large l'emporte. Les règles par ligne sont évaluées dans la requête SQL (clause ajoutée).
- **Vérification** : `check(sujet, action, objet)` en Rust, avec un cache invalidé par les opérations ACL. Toute diffusion WebSocket est filtrée : personne ne reçoit une opération sur un objet qu'il ne peut pas lire.
- **Agents** : droits effectifs = leurs droits ∩ droits de l'humain pour qui ils agissent (s'il y en a un) ∩ portée du jeton (§6.7).

### 5.7 Recherche
- Tantivy indexe les titres et les blocs (`blocks_index`), avec le fil d'Ariane de chaque page. Les résultats sont filtrés par permissions au moment de la requête.
- **Recherche hybride** (P4) : BM25 + vecteurs (embeddings par bloc, avec le contexte du fil d'Ariane), fusion RRF, puis rerank optionnel. Les citations pointent sur un **bloc précis**.
- Les deux index sont reconstructibles : `mementum reindex`.

### 5.8 Fichiers
- Stockage adressé par contenu (hash SHA-256), donc dédoublonné. Miniatures générées en tâche de fond.
- URL signées à durée limitée. Quotas par organisation. Antivirus ClamAV en option.

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

<database id="db_Taches" view="Tableau" />

Voir <mention type="page" id="pg_Spec01">Spécification</mention>. ^b8
```

- **Balises** : `<callout>`, `<toggle>`, `<columns>/<column ratio>`, `<tabs>/<tab>`, `<page>`, `<database>`, `<mention>`, `<embed>`, `<synced ref>`, `<file>`, `<span color>`, `<comment>` (fils ancrés, sur demande).
- **Échappatoire sans perte** : `<block type="x" id="…" data="…"/>` pour tout bloc sans équivalent (bloc d'une extension, par exemple). L'agent doit le laisser tel quel ; le supprimer exige `allow_deleting_content`.
- **Ancres optionnelles** : à la lecture, avec `anchors: "block"`, chaque bloc reçoit un alias court local à la page (`^b4`, la syntaxe des références de bloc d'Obsidian). Sans ancres, le Markdown reste propre et économe en tokens.
- **`revision`** : une poignée opaque, dérivée du vecteur d'état Yjs, renvoyée à chaque lecture. Les pages très longues sont paginées (`truncated: true`, `<unloaded id="…"/>`).
- La **spécification MMD** est publiée sous Apache-2.0 et exposée aux agents comme ressource MCP (`mementum://spec/mmd`), pas dans les descriptions d'outils.
- **Règles de format établies en P0** (convertisseur `spike/mmd`, aller-retour validé sur 12 800+ cas aléatoires) :
  - **Marques** : en Markdown (`**`, `*`, `~~`, `` ` ``) quand le texte marqué commence et finit par une lettre ou un chiffre et qu'il est entouré de texte sans marque ; sinon en balises (`<strong>`, `<em>`, `<s>`, `<code>`). `<u>` et `<span color>` sont toujours en balises.
  - **Espaces en début et fin de contenu** écrites `&#32;` (Markdown les supprimerait). Un paragraphe vide s'écrit `<p/>`.
  - **Ancres** : ` ^bN` en fin de ligne pour les blocs de texte, `^bN` sur la ligne suivante pour le code, les équations, les séparateurs, les images et les tableaux, attribut `id="bN"` pour les balises, attribut `anchor="bN"` pour `<page>` et `<database>` (leur `id` est déjà pris).
  - **Indentation** : les enfants d'un élément numéroté sont indentés de la largeur du marqueur (« 1. » = 3 espaces).
  - **Maths en ligne** `$…$` : LaTeX à accolades équilibrées.
  - **Échappement** : un « ! » juste avant un lien est échappé (sinon il serait relu comme une image).

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
  - Les remplacements se font aux positions UTF-16.
  - Un agent qui édite pendant qu'un humain tape : 0 perte, visible en 7 à 15 ms.
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
- **Auth** : OAuth 2.1 (métadonnées RFC 9728, enregistrement des clients par CIMD, audience RFC 8707, portées progressives). **Jetons personnels** pour les modèles locaux et la CLI. Pont stdio pour les clients qui ne savent pas faire de HTTP.
- **14 outils** :

| # | Outil | Rôle |
|---|---|---|
| 1 | `search` | Recherche hybride. Filtres : coffre, type, date, auteur. Renvoie titres, fils d'Ariane, extraits ancrés |
| 2 | `fetch` | Page, base, ligne ou bloc par ID ou URL → MMD + propriétés + `revision` (ancres en option, pagination) |
| 3 | `create_pages` | Création en lot sous un parent, contenu MMD, propriétés |
| 4 | `update_page` | Opérations du §6.3, `mode: suggest \| direct \| branch` |
| 5 | `manage_pages` | Déplacer, dupliquer, mettre à la corbeille, restaurer. La corbeille demande une confirmation humaine |
| 6 | `query_database` | Lignes via filtre JSON, via une vue enregistrée ou via **SQL en lecture seule** ; curseur, `limit` ≤ 100 |
| 7 | `upsert_rows` | Création ou mise à jour de lignes en lot, avec résultat de validation par ligne |
| 8 | `get_database` | Schéma typé, vues, nombre de lignes, options des sélections |
| 9 | `update_database_schema` | Ajouter, renommer ou supprimer des propriétés et des options (portée plus élevée) |
| 10 | `manage_views` | Créer et modifier des vues (filtres, tris, groupes) |
| 11 | `comments` | Lister, créer, résoudre ; ancrés sur un bloc |
| 12 | `suggestions` | Lister, accepter ou refuser (par défaut, seul un humain accepte) |
| 13 | `workspace_info` | Qui suis-je, membres, coffres ; correspondance nom ↔ ID |
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
- **Niveaux d'accès des agents** : `lire < suggérer < modifier < complet`, accordés sur un coffre, un sous-arbre de pages ou une base.
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
- **Passerelle multi-fournisseurs** : API compatibles OpenAI (Ollama, vLLM, LM Studio, OpenRouter…), Anthropic natif, Mistral. Chaque organisation apporte sa clé ; l'admin d'instance choisit les fournisseurs autorisés. **Mode 100 % local** possible.
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

### 7.3 Montée en charge
1. **Un nœud + SQLite** : jusqu'à quelques centaines d'utilisateurs actifs.
2. **Un nœud + Postgres + S3** : plusieurs milliers. On monte en puissance la machine.
3. **Plusieurs nœuds** *(plus tard)* : chaque page ou organisation est routée vers un nœud (« un processus par document », comme Figma), avec diffusion inter-nœuds via Postgres LISTEN/NOTIFY ou NATS.

### 7.4 Sauvegardes, mises à jour, migrations
- `mementum backup` (à chaud, cohérent) vers un disque ou S3, planifiable dans l'admin. Compatible Litestream pour la réplication continue de SQLite.
- `mementum restore`, et `mementum migrate --to postgres` pour passer de SQLite à Postgres sans rien perdre.
- Migrations de schéma automatiques au démarrage, avec sauvegarde préalable. Le changelog signale les ruptures, et il n'y en aura pas sans outil de migration.

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
| **P1** | « Écrire ensemble » (MVP) | Instance, organisations, coffres privés et partagés, comptes locaux + passkeys. Arbre de pages, éditeur avec les blocs de base, temps réel et présence, commentaires, historique simple, corbeille, recherche plein texte. **Import d'un vault Obsidian et des pages Notion** (§4.6), export `.mementum`. **MCP v1** (search, fetch, create_pages, update_page, manage_pages, workspace_info) avec jetons personnels. Image Docker | Lenny et 3 personnes l'utilisent une semaine à la place de Notion, et Claude Code édite une page pendant qu'un humain tape, sans perte |
| **P2** | « Bases de données » | Bases et sources, propriétés (relations, rollups, sous-ensemble des formules), vues tableau, kanban, liste, galerie, calendrier, filtres, tris et groupes, vues liées, templates, import CSV, **import des bases Notion**. Outils MCP des bases (query SQL compris), API REST | Une base de 100 000 lignes respecte les budgets du §5.10 sur un VPS à 2 vCPU |
| **P3** | « Organisations » | Permissions fines, groupes, invités, exceptions de partage par page, liens publics, publication web. **Suggestions**, boîte de réception, frise d'historique. OIDC + SAML, journal d'audit, panneau d'admin | Une organisation de 50 personnes avec des invités, migrée depuis Notion |
| **P4** | « Agents » | Passerelle IA, agents d'espace (identité, parrain, politiques), mode suggestion par défaut, annulation par exécution, Q&R avec citations, propriétés IA, déclencheurs. OAuth MCP + CIMD, recherche hybride, contrôle de la règle des deux | La suite d'évals (§10) atteint ≥ 90 % de réussite avec Claude et ≥ 70 % avec un modèle local via Ollama |
| **P5** | « Mementum 1.0 » | Vues frise, graphique, carte, formulaire, fil et tableau de bord. Formules complètes, boutons, automatisations, webhooks, permissions par ligne. Hors ligne complet, appli Tauri, PWA mobile soignée. Passe de performance, Postgres + S3 | Parité avec le cœur de Notion (§4.5) et budgets tenus |
| **Cloud** | « Mementum Cloud » (en parallèle de la P4) | Service `mementum-cloud` : inscription, Stripe, création et mise en veille des instances, sauvegardes S3, domaines personnalisés, tableau de bord, bouton « Passer en auto-hébergement ». Pages légales (CGU, confidentialité, DPA). Bêta fermée dès que la P3 est stable | 10 organisations bêta hébergées pendant un mois sans incident, et une migration cloud → auto-hébergé réussie de bout en bout |
| **+** | Après la 1.0 | Mobile natif, fédération (Mementum Link), bases synchronisées depuis des API externes, notes de réunion (Whisper local), mode présentation, permissions par colonne, SCIM, tableau blanc | — |

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
| **Déplacement concurrent d'un bloc = bloc dupliqué** avec Yjs *(confirmé en P0 : 25 sur 25)* | Passe serveur après fusion : un même id de `blockContainer` en double → une seule copie gardée (P1) ; déplacements de pages faits par le serveur |
| Pages géantes lentes à ouvrir (10 000 blocs ≈ 1,3 s en P0, dont 0,9 s de synchro) | Cache local SQLite WASM (§5.9) et chargement paresseux des blocs, dès la P1 |
| Bases JSON sur SQLite trop lentes à 100 000 lignes sur un Pi | Budgets tenus sur PC en P0 ; **mesure ARM à faire** (risque : filtre + tri) ; rollups matérialisés |
| ~~Fidélité MMD ⇄ Yjs~~ | Levé en P0 : aller-retour sans perte sur 20 types, 12 800+ cas proptest |
| ~~Choix CRDT~~ | Levé en P0 : Yjs |
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
- **Coffres façon Obsidian**, partageables avec l'organisation, des groupes ou des personnes (§4.4). Idée de Lenny, validée sur le principe.
- **Mementum autonome, sans miroir en fichiers** : on entre par l'**import Notion et Obsidian** dès la P1 (§4.6), on sort par l'export.
- **Serveur MCP dès la première version** (P1), complété jusqu'en P4.
- **Licence AGPL-3.0** pour le serveur et l'application web, avec **Apache-2.0** pour le SDK et la spec MMD, **DCO sans CLA**, aucune fonction payante (§3.1).
- **Modèle économique** : auto-hébergement gratuit avec toutes les fonctionnalités, et **Mementum Cloud**, une offre hébergée payante pour ceux qui ne veulent pas gérer de serveur (§7.6).

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
Aucune question bloquante pour commencer. À trancher avant d'ouvrir Mementum Cloud (§7.6) : les prix, une éventuelle offre gratuite, l'hébergeur et la structure juridique pour facturer.
