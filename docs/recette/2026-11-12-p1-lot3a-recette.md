# Recette du lot 3, partie A : collaboration

Pour le propriétaire de l'instance (Alex), ou Lenny. Écrit le 16 novembre 2026.

Tu vas essayer l'application toi-même, ce document sous les yeux. Compte une heure environ.

Tout ce qu'un test automatique peut vérifier l'est déjà. Cette liste porte sur ce qu'un test ne juge pas : ce que tu vois quand vous êtes deux sur un espace, le confort, le dessin. Chaque case dit ce que tu fais, puis ce que tu dois voir. Coche si c'est bon. Sinon, note le numéro (« D4 ne va pas »), et une phrase sur ce que tu as vu à la place, dans la ligne « Remarques : » de la section.

Fais les sections dans l'ordre : chacune part de l'état laissé par la précédente.

## Avant de commencer

**Ce qu'il te faut.**

- Un navigateur récent : Chrome, Edge ou Firefox.
- Une seconde fenêtre, en navigation privée, pour jouer une autre personne. Dans la suite, « fenêtre 1 » est la tienne, « fenêtre 2 » celle de Camille. Mets-les côte à côte : presque tout se juge en regardant les deux à la fois.
- Une adresse e-mail inventée pour Camille, par exemple `camille@exemple.fr`. Aucun e-mail n'est envoyé.

**Une instance neuve, pas tes données.** La partie B du lot 3 est encore en cours, et peut changer sa migration de base (`0004`) : une base ouverte par cette version pourrait ensuite refuser de démarrer avec la suivante. Fais donc cette recette sur un dossier de données à part. À la racine du dépôt, dans deux terminaux :

```
corepack pnpm install
cargo run -p mementum-cli -- serve --data-dir data/recette-lot3 --version-every-secs 10
```

```
corepack pnpm -C apps/web dev
```

Puis ouvre `http://localhost:5173`. Avec `--version-every-secs 10`, une version de page est coupée à l'écriture qui suit de dix secondes la première écriture pas encore versionnée, au lieu de dix minutes : c'est ce qui rend la section H faisable en quelques minutes. La recette tourne sous Vite, en développement (`pnpm dev`) : c'est ce qui permet la section J. Si `cargo` répond « Accès refusé », un autre serveur Mementum tourne déjà depuis ce dossier : arrête-le d'abord.

Si le port 5173 ou 8080 est déjà pris, choisis-en d'autres. Sous PowerShell :

```
cargo run -p mementum-cli -- serve --data-dir data/recette-lot3 --version-every-secs 10 --bind 127.0.0.1:8081
```

```
$env:MEMENTUM_E2E_WEB_PORT = "5174"; $env:MEMENTUM_E2E_API_PORT = "8081"; corepack pnpm -C apps/web dev
```

Puis ouvre `http://localhost:5174`. Le port du serveur sert aussi en K3.

**Ce que tu verras aussi.** La version que tu lances contient déjà une part de la partie B (favoris, récents, fil d'Ariane dans la barre du haut, menu « / » et ses blocs riches, équations). Elle a sa propre recette, [`2026-11-12-p1-lot3b-recette.md`](2026-11-12-p1-lot3b-recette.md) : ne la juge pas ici.

## Ce que cette partie contient, et ne contient pas

**Dedans :**

- la barre latérale qui suit en direct ce que font les autres ;
- les avatars des personnes qui ont la page ouverte ;
- les commentaires, sur la page ou sur un bloc, avec réponses, réactions et fils résolus ;
- l'historique des versions d'une page, son aperçu, la restauration ;
- le déplacement d'un bloc pendant que quelqu'un d'autre écrit ;
- un onglet qui ne fait plus rien sous un autre compte quand un autre onglet du navigateur s'y connecte.

**Pas encore :** les notifications (une mention dans un commentaire ne prévient personne) ; la comparaison de deux versions ; la recherche ; l'envoi d'e-mails ; les bases de données. Ne signale pas ces absences comme des défauts.

## A. Préparer l'espace (4 minutes)

- [ ] **A1.** Fenêtre 1 : crée le premier compte (« Bienvenue dans Mementum »). Tu dois arriver dans ton espace, vide.
- [ ] **A2.** Fenêtre 1 : invite `camille@exemple.fr` comme « Membre », par les réglages de l'espace, comme au lot 2. Fenêtre 2 : colle le lien, crée le compte de Camille. Camille doit arriver dans ton espace.
- [ ] **A3.** Fenêtre 1 : crée une page « Essai » dans la section « Espace ». Écris-y trois paragraphes, un par ligne : `Un`, `Deux`, `Trois`. Fenêtre 2 : la page « Essai » doit apparaître dans la barre latérale de Camille sans qu'elle touche à rien (voir B1).
- [ ] **A4.** Fenêtre 1 : sur « Essai », clique « Partager ». Sous « Tous les membres de l'espace », règle le niveau des membres à « Peut modifier ». Camille étant le seul autre membre, **c'est ainsi que tu changeras son niveau dans toute la suite** (« passe les membres à … ») ; toi, propriétaire, tu gardes l'accès complet. Sans ce réglage, un membre a l'accès complet sur une page d'équipe.

Remarques :

## B. La barre latérale à deux (4 minutes)

Fenêtre 1 et fenêtre 2 côte à côte, chacune sur l'espace.

- [ ] **B1.** Fenêtre 2 : crée une page « Idées » dans « Espace ». Fenêtre 1 : elle doit apparaître dans la barre latérale en moins d'une seconde, sans clic ni rechargement.
- [ ] **B2.** Fenêtre 2 : renomme « Idées » en « Idées de Camille », dans le titre de la page, puis Entrée (ou clique ailleurs : le titre ne s'enregistre qu'à ce moment). Fenêtre 1 : le nouveau titre doit arriver dans la barre latérale moins d'une seconde après.
- [ ] **B3.** Fenêtre 2 : glisse « Idées de Camille » sous « Essai », pour en faire une sous-page. Fenêtre 1 : elle doit passer sous « Essai » aussitôt, et rien d'autre ne doit bouger dans l'arbre.
- [ ] **B4.** Fenêtre 2 : mets « Idées de Camille » à la corbeille. Fenêtre 1 : elle doit quitter la barre latérale aussitôt. Fenêtre 2 : restaure-la depuis la corbeille. Fenêtre 1 : elle doit revenir à sa place.
- [ ] **B5.** Fenêtre 1 : renomme l'espace dans ses réglages. Fenêtre 2 : le nouveau nom doit arriver en haut de la barre latérale de Camille sans rechargement.
- [ ] **B6.** Fenêtre 1 : ouvre « Essai », clique dans son titre, puis ailleurs, sans rien taper. Fenêtre 2 : renomme « Essai » en « Essai 2 », puis remets « Essai » (Entrée après chaque titre). Fenêtre 1 : le titre affiché doit suivre, et ton clic de tout à l'heure ne doit rien avoir renvoyé (le titre reste celui de Camille).
- [ ] **B7.** Pendant toutes ces étapes, la barre latérale de la fenêtre où tu ne fais rien ne doit jamais clignoter, se vider, ni faire sauter la ligne sous ton pointeur.

Remarques :

## C. Qui regarde la page (3 minutes)

- [ ] **C1.** Fenêtre 1 : ouvre « Essai ». Fenêtre 2 : ouvre « Essai ». Fenêtre 1 : dans la barre du haut, à droite, une pastille ronde doit apparaître, avec l'initiale de Camille sur une couleur. Survole-la : son nom s'affiche. Ta propre pastille n'y est pas.
- [ ] **C2.** Fenêtre 2 : regarde la barre du haut. Ta pastille doit y être, avec ton initiale, et pas celle de Camille.
- [ ] **C3.** Fenêtre 2 : clique dans le texte de « Deux ». Fenêtre 1 : le curseur de Camille doit s'afficher dans « Deux », à sa couleur. Son nom, celui de son compte, s'affiche à côté un instant après chaque mouvement du curseur, puis s'efface.
- [ ] **C4.** Fenêtre 1 : dans « Partager », passe les membres à « Peut lire ». Survole la pastille de Camille. Son nom doit être suivi de « (lecture) ». Remets les membres à « Peut modifier », et clique « Rouvrir la page » dans la fenêtre 2.
- [ ] **C5.** Fenêtre 2 : quitte « Essai » (ouvre une autre page). Fenêtre 1 : sa pastille doit disparaître en quelques secondes. Fenêtre 2 : rouvre « Essai ». La pastille doit revenir.
- [ ] **C6.** Fenêtre 1 : ouvre « Essai » dans un second onglet. Aucun des deux onglets ne doit montrer ta pastille : un compte ne se voit pas lui-même. Ferme ensuite ce second onglet.

Remarques :

## D. Écrire un commentaire, le retrouver (6 minutes)

Fenêtre 1 et fenêtre 2 sur « Essai ».

- [ ] **D1.** Fenêtre 1 : en haut de la page, clique « Commentaires ». Un panneau s'ouvre à droite, titré « Commentaires », avec une zone pour commenter la page et « Aucun fil ouvert. ».
- [ ] **D2.** Fenêtre 1 : écris `Bonne page` dans la zone de la page, et envoie. Le fil doit apparaître dans le panneau, avec ton nom et « à l'instant ». Le bouton « Commentaires » doit maintenant porter le nombre 1.
- [ ] **D3.** Fenêtre 2 : sans recharger, clique « Commentaires ». Le fil de la fenêtre 1 doit y être. Laisse le panneau ouvert.
- [ ] **D4.** Fenêtre 1 : survole « Deux ». Une poignée apparaît à sa gauche. Clique-la : dans le menu, choisis « Commenter ». Le panneau doit montrer en haut le début du texte du bloc (« Sur le bloc : Deux »), et une zone de saisie qui a le focus. Écris `À reformuler` et envoie.
- [ ] **D5.** Fenêtre 1 : une pastille doit apparaître dans la marge droite, à la hauteur de « Deux ». Fenêtre 2 : la même pastille doit apparaître, et le fil arriver dans le panneau de Camille, sans rechargement.
- [ ] **D6.** Fenêtre 1 : ferme le panneau (Échap). Le focus doit revenir où il était. Clique la pastille de « Deux » : le panneau doit se rouvrir sur ce fil.
- [ ] **D7.** Fenêtre 1 : clique dans « Trois », puis tape Ctrl+Alt+M (Cmd+Option+M sur Mac). Le panneau doit s'ouvrir sur un commentaire neuf du bloc « Trois ». Si ton clavier est en AZERTY, c'est la touche où est écrit M qui compte, pas sa position. Avec AltGr, tes caractères habituels (@, #, €) doivent rester ceux qu'ils sont.
- [ ] **D8.** Tape un début de texte dans ce commentaire, sans l'envoyer. Appuie sur Échap : tu sors du champ, rien n'est effacé. Échap encore : le panneau se ferme. Rouvre-le : ton texte doit être là. Envoie-le.
- [ ] **D9.** Les fils doivent se ranger dans l'ordre des blocs de la page (« Deux » avant « Trois »), puis les fils de la page. Dans un fil de bloc, clique « Aller au bloc » : la page doit défiler jusqu'au bloc, mis en évidence un instant.
- [ ] **D10.** Fenêtre 1 : ajoute une trentaine de lignes à la page, pour qu'elle défile. Fais défiler : le panneau doit rester à sa place, à droite, et les pastilles suivre leurs blocs.

Remarques :

## E. Répondre, réagir, résoudre, supprimer (5 minutes)

- [ ] **E1.** Fenêtre 2 : dans le fil « À reformuler », clique « Répondre », écris `D'accord` et envoie. Fenêtre 1 : la réponse doit arriver dans le fil, sous le premier message, avec le nom de Camille.
- [ ] **E2.** Fenêtre 1 : sur le message de Camille, ajoute une réaction 👍. Fenêtre 2 : la réaction doit arriver, avec un compte de 1. Camille ajoute la même : le compte passe à 2. Elle la retire : il revient à 1.
- [ ] **E3.** Fenêtre 1 : sur ton propre message, choisis « Modifier », change le texte, enregistre. Le message doit dire « modifié ». Sur le message de Camille, tu ne dois pas voir « Modifier ».
- [ ] **E4.** Fenêtre 1 : clique « Résoudre » sur le fil « À reformuler ». Il doit quitter les fils ouverts, la pastille de « Deux » disparaître, et « Fils résolus (1) » apparaître en bas du panneau. Clique-le : le fil revient, avec « Rouvrir ». Rouvre-le : il revient parmi les fils ouverts, et la pastille aussi.
- [ ] **E5.** Fenêtre 1 : supprime le bloc « Trois » (sélectionne son texte, efface-le, puis supprime la ligne). Son fil doit rester lisible, rangé sous « Bloc supprimé ». On peut encore y répondre.
- [ ] **E6.** Fenêtre 1 (accès complet) : sur le fil du bloc supprimé, clique « Supprimer le fil ». Une confirmation doit demander « Supprimer ce fil ? », nommer qui l'a ouvert et le début de son premier message, et dire qu'il part pour tout le monde. Confirme. Le fil doit partir chez toi et dans la fenêtre 2, et le focus aller au fil voisin.
- [ ] **E7.** Fenêtre 2 (Camille, « Peut modifier » depuis A4) : elle ne doit pas voir « Supprimer le fil ». Elle peut supprimer son propre message (le bouton « Supprimer » sous lui) après une confirmation.

Remarques :

## F. Commenter sans modifier, lire sans commenter (4 minutes)

- [ ] **F1.** Fenêtre 1 : dans « Partager », passe les membres à « Peut commenter ». Fenêtre 2 : la page passe en lecture seule (bandeau). Le panneau des commentaires reste, et Camille peut toujours écrire dans les fils.
- [ ] **F2.** Fenêtre 2 : survole un bloc qui n'a pas de fil ouvert. La poignée n'apparaît plus ; dans la marge de droite, une petite bulle « + » doit suivre le bloc survolé (son infobulle : « Commenter ce bloc » ; elle ne vient pas sur un bloc qui a déjà un fil ouvert, qui a sa pastille). Clique-la : le panneau s'ouvre sur un commentaire neuf de ce bloc. Écris `Vu` et envoie.
- [ ] **F3.** Fenêtre 2 : clique dans le texte d'un bloc (la page est en lecture seule, mais on peut y poser le curseur), puis Ctrl+Alt+M. Le commentaire neuf doit viser ce bloc.
- [ ] **F4.** Fenêtre 2 : Camille peut résoudre le fil qu'elle a ouvert, pas celui que tu as ouvert (le bouton « Résoudre » n'y est pas).
- [ ] **F5.** Fenêtre 1 : passe les membres à « Peut lire ». Fenêtre 2 : les fils restent lisibles, mais aucune zone de saisie ni aucun bouton d'action ne doit rester (ni répondre, ni réagir, ni résoudre).
- [ ] **F6.** Fenêtre 1 : remets les membres à « Peut modifier ». Fenêtre 2 : clique « Rouvrir la page ».

Remarques :

## G. Un texte qu'on ne peut plus envoyer (3 minutes)

- [ ] **G1.** Fenêtre 2 : dans un fil, clique « Répondre » et tape une phrase, sans l'envoyer.
- [ ] **G2.** Fenêtre 1 : passe les membres à « Peut lire ». Fenêtre 2 : la zone de saisie disparaît, mais pas le texte : il doit être montré en haut du panneau, dans un encart, avec « Tu ne peux plus écrire ici. », le texte (sélectionnable, pour le copier) et un bouton pour l'écarter.
- [ ] **G3.** Fenêtre 1 : remets les membres à « Peut modifier ». Fenêtre 2 : après « Rouvrir la page », la phrase doit retrouver sa zone, dans son fil, sans être partie toute seule et sans voler le focus.
- [ ] **G4.** Fenêtre 2 : envoie-la. Elle doit arriver une fois, dans la fenêtre 1 aussi.

Remarques :

## H. L'historique et son aperçu (5 minutes)

L'instance coupe une version de page dix secondes après la première écriture non versionnée, juste avant l'écriture suivante (réglage de la section « Avant de commencer »).

- [ ] **H1.** Fenêtre 1 : dans « Essai », ajoute une ligne `Version une`. Attends quinze secondes, puis ajoute `Version deux`. Attends encore quinze secondes, puis ajoute `Version trois`.
- [ ] **H2.** Fenêtre 1 : en haut à droite, ouvre le menu « … », puis « Historique ». Une fenêtre doit s'ouvrir : à gauche, « Version actuelle » puis les versions, la plus récente d'abord, chacune avec sa date, « il y a… » et l'avatar et le nom de qui l'a modifiée (le tien : Camille n'a encore rien écrit dans la page).
- [ ] **H3.** Clique une version plus ancienne. À droite, la page doit s'afficher telle qu'elle était à ce moment (sans « Version trois », par exemple), en lecture seule, avec la même mise en forme que dans l'éditeur. Pendant ce temps, la page elle-même, derrière la fenêtre, ne doit pas changer.
- [ ] **H4.** Au clavier : les flèches parcourent la liste sans rien choisir, Entrée choisit, Tab ne sort pas de la fenêtre, Échap la ferme, et le focus revient au bouton « … ».
- [ ] **H5.** Fenêtre 2 : Camille, avec « Peut modifier », ouvre aussi « Historique ». Elle doit voir la même liste. Fenêtre 1 : tape une ligne, attends quinze secondes, tapes-en une autre. Fenêtre 2 : la nouvelle version doit apparaître dans sa liste, sans rouvrir la fenêtre.
- [ ] **H6.** Fenêtre 1 : passe les membres à « Peut lire ». Fenêtre 2 : la fenêtre « Historique » reste ouverte, et dit le refus du serveur tel quel, sans « Réessayer ». Ferme-la : le menu « … » ne propose plus « Historique ». Remets les membres à « Peut modifier », puis « Rouvrir la page ».

Remarques :

## I. Restaurer (4 minutes)

- [ ] **I1.** Fenêtre 1 : dans « Historique », choisis la version qui a `Version une` sans `Version deux`. Clique « Restaurer cette version ». Une confirmation doit dire « Le contenu actuel sera remplacé. Il restera dans l'historique. », et, puisque Camille a la page ouverte, ajouter que ce qu'elle tape pendant la restauration peut être perdu.
- [ ] **I2.** Confirme. Le bouton dit « Restauration… » un instant, puis la fenêtre se ferme, et la page montre le contenu restauré (« Version restaurée. » n'est dit qu'aux lecteurs d'écran). Fenêtre 2 : la page de Camille doit montrer le contenu restauré, sans rechargement.
- [ ] **I3.** Fenêtre 1 : rouvre « Historique ». En tête de liste, une version marquée « avant une restauration » doit garder ce que tu viens de remplacer. Choisis-la : `Version trois` doit y être.
- [ ] **I4.** Fenêtre 1 : si tu avais des fils sur des blocs restaurés, ils doivent avoir gardé leur pastille : la restauration garde l'identifiant des blocs.
- [ ] **I5.** Fenêtre 1 : mets « Essai » à la corbeille, puis ouvre-la depuis la corbeille. L'historique se lit encore, mais sans « Restaurer cette version » ; choisis une version : une phrase dit qu'aucune version ne se restaure à la corbeille. Restaure ensuite la page depuis la corbeille.

Remarques :

## J. Déplacer un bloc pendant que l'autre écrit (8 minutes)

Fenêtre 1 et fenêtre 2 sur « Essai ». La page doit commencer par trois lignes, une par bloc : `Un`, `Deux`, `Trois`. Après la restauration de la section I, `Trois` n'y est plus (supprimé en E5) : ajoute-le sous `Deux`, et récris ce qui manque. Pour déplacer un bloc : survole-le, prends la poignée à sa gauche et glisse-la ; ou place le curseur dans le bloc et tape Ctrl+Maj+↑ ou Ctrl+Maj+↓.

- [ ] **J1. Une frappe à côté.** Fenêtre 2 : Camille tape sans s'arrêter à la fin de « Un ». Pendant ce temps, fenêtre 1 : glisse « Trois » tout en haut. Tout ce que tape Camille doit rester dans « Un », à sa place, chez elle et chez toi, et son curseur ne doit pas sauter dans un autre bloc.
- [ ] **J2. Le curseur suit son bloc.** Fenêtre 2 : Camille place son curseur au milieu de « Deux », sans taper. Fenêtre 1 : déplace « Deux » d'une place (Ctrl+Maj+↑). Fenêtre 2 : Camille tape trois lettres. Elles doivent entrer dans « Deux », là où était son curseur.
- [ ] **J3. Deux blocs, deux personnes.** Fenêtre 1 : glisse « Un » en bas. En même temps, fenêtre 2 : glisse « Deux » en haut. Les deux pages doivent finir identiques, chaque bloc une seule fois, aucun texte perdu.
- [ ] **J4. La frappe arrive avant le déplacement (gardée).** Pour couper une fenêtre du serveur, le réglage « Hors connexion » des outils du navigateur ne suffit pas : sous Chrome et Edge, il ne coupe pas la connexion d'édition. Passe par la console. Fenêtre 1 : ouvre les outils du navigateur (F12), onglet « Console », tape `__mementum.provider.disconnect()` puis Entrée (cet objet n'existe qu'en développement, sous `pnpm dev`). Après une ou deux secondes, la barre du haut doit dire « Hors ligne : tes modifications partiront au retour du réseau ».
- [ ] **J5.** Fenêtre 2 (en ligne) : Camille tape ` de Camille` à la fin de « Deux ». Fenêtre 1 (coupée, qui ne voit pas sa frappe) : glisse « Deux » sous le dernier bloc. Puis, dans la console de la fenêtre 1, tape `__mementum.provider.connect()` et Entrée. Les deux fenêtres doivent montrer « Deux de Camille » une seule fois, à la place où tu l'as glissé.
- [ ] **J6. La frappe arrive après le déplacement (limite connue).** Recommence dans l'autre sens : dans la console de la fenêtre 2 (F12 dans la fenêtre de Camille), `__mementum.provider.disconnect()` ; Camille tape ` encore` à la fin de « Un » ; fenêtre 1, en ligne, glisse « Un » ailleurs ; puis, dans la console de la fenêtre 2, `__mementum.provider.connect()`. Ce que tu dois voir : « Un » à sa nouvelle place, **sans** ` encore` : la frappe arrivée au serveur après le déplacement est perdue. C'est la limite connue de ce lot (R222), pas un défaut. Note seulement si tu vois autre chose (un bloc en double, un texte dans un autre bloc, une page différente dans les deux fenêtres).
- [ ] **J7. Annuler un déplacement.** Fenêtre 1 : déplace un bloc, puis Ctrl+Z. Le bloc doit revenir à sa place, et rien d'autre ne doit changer.
- [ ] **J8. Un bloc avec un fil.** Fenêtre 1 : commente « Deux », puis déplace-le. Sa pastille doit le suivre, et le fil rester attaché à lui (« Aller au bloc » y mène).

Remarques :

## K. Une session reprise sous un autre compte, dans un autre onglet (3 minutes)

Le cookie de session est commun aux onglets d'un même navigateur. Cette section vérifie que rien de ce qu'un onglet envoie ne part sous le compte qu'un autre onglet vient de connecter.

- [ ] **K1.** Fenêtre 1 : garde « Essai » ouverte dans l'onglet A. Ouvre un onglet B dans la même fenêtre, sur l'application. Onglet B : déconnecte-toi (nom de l'espace, puis « Se déconnecter »). Onglet A : la fenêtre « Ta session a pris fin » doit s'ouvrir par-dessus la page, sans la fermer.
- [ ] **K2.** Onglet B : connecte-toi avec le compte de Camille. Reviens sur l'onglet A, sans rien y taper : l'onglet doit se recharger de lui-même et montrer l'espace sous le nom de Camille. Rien de ce que tu avais ouvert sous ton nom ne doit avoir changé ou être parti sous le sien.
- [ ] **K3.** (Pour qui veut voir le refus du serveur.) Dans un terminal (sous Windows PowerShell 5.1, écris `curl.exe` au lieu de `curl`, et mets le port de ton serveur s'il n'est pas 8080), avec l'identifiant d'un compte quelconque (par exemple `00000000-0000-7000-8000-000000000000`) et le cookie `mementum_session` de la fenêtre (outils du navigateur, onglet « Application », « Cookies ») : `curl -i -H "Cookie: mementum_session=<la valeur>" -H "X-Mementum-Account: 00000000-0000-7000-8000-000000000000" http://127.0.0.1:8080/api/v1/me`. La réponse doit être `409`, avec le code `account_changed` et le message « tu t'es connecté sous un autre compte dans un autre onglet ; rien n'a été fait ». Sans l'en-tête `X-Mementum-Account`, la même commande doit répondre `200`.
- [ ] **K4.** Onglet B : déconnecte Camille, puis reconnecte-toi sous ton compte. Ferme l'onglet B.

Remarques :

## L. Le dessin : direction A « Papier » (5 minutes)

Le dessin doit ressembler à Notion sans le copier : mêmes emplacements, tons de papier, un seul vert. Regarde chaque écran d'un œil critique, et note ce qui s'en écarte.

- [ ] **L1. Barre du haut.** De gauche à droite : le fil d'Ariane de la page, puis, à droite, les avatars des présents, « Commentaires », « Partager » et « … ». Les avatars sont de petites pastilles rondes (24 pixels), bordées de blanc, qui se chevauchent un peu ; au-delà de cinq personnes, « +N ». Le bouton « Commentaires » porte le nombre de fils ouverts dans une petite pastille, et rien quand il n'y en a pas. La barre garde la même hauteur avec ou sans avatars.
- [ ] **L2. Panneau des commentaires.** Une colonne blanche à droite de la page, d'environ 320 pixels, séparée par un trait fin, qui suit le défilement. Les fils sont des cartes sobres : nom en gras, « il y a… » en gris, texte noir, actions discrètes en texte. Les réactions sont de petites pastilles avec leur nombre. Le texte d'un message garde ses retours à la ligne, et un nom ou un texte écrit de droite à gauche (arabe, hébreu) ne bouscule pas ce qui l'entoure.
- [ ] **L3. Fenêtre étroite.** Réduis la fenêtre sous 1 100 pixels de large : le panneau des commentaires passe par-dessus la page au lieu de la pousser, et se ferme toujours par Échap.
- [ ] **L4. Pastilles de la marge.** De petites pastilles blanches, bordées d'un trait fin, à droite du texte, alignées sur leur bloc, avec l'icône d'une bulle, et le nombre de fils en gris à partir de deux ; elles ne recouvrent jamais le texte, et foncent un peu au survol.
- [ ] **L5. Fenêtre « Historique ».** Une fenêtre centrée, sur un voile : la liste à gauche, l'aperçu à droite, « Restaurer cette version » en bas. La version choisie est marquée d'un fond plus soutenu. « avant une restauration » est une étiquette discrète. L'aperçu a la même typographie que la page.
- [ ] **L6. Confirmations.** « Supprimer ce fil ? », « Supprimer ce message ? » et « Restaurer cette version ? » ressemblent aux confirmations du lot 2 (« Retirer l'accès… ») : même fenêtre, même place des boutons, « Annuler » à côté.
- [ ] **L7. Clavier.** Dans le panneau et dans la fenêtre « Historique », appuie plusieurs fois sur Tab : ce qui a le focus porte le contour vert, bien visible, et le focus ne se perd jamais (après une action, il reste dans le panneau, sur le bouton suivant, le fil, ou le titre).
- [ ] **L8. Les mots.** Tous les textes sont en français, tutoient, et aucun ne montre un code technique (sauf K3, dans le terminal).

Remarques :

## M. Les cas de bord (4 minutes)

- [ ] **M1. Barre latérale après une coupure du serveur.** Fenêtre 1 : arrête le serveur (Ctrl+C dans son terminal), attends dix secondes, relance-le. Fenêtre 2 : crée une page. Fenêtre 1 : elle doit apparaître dans la barre latérale en une dizaine de secondes au plus, sans rechargement.
- [ ] **M2. Arrêt du serveur et historique.** Fenêtre 1 : tape une ligne dans « Essai », puis arrête le serveur aussitôt (Ctrl+C) et relance-le. Ouvre « Historique » : la ligne doit être dans « Version actuelle », et une version récente doit exister (le serveur garde une dernière version des pages ouvertes en s'arrêtant).
- [ ] **M3. Beaucoup de fils.** Ouvre une vingtaine de fils sur des blocs différents d'une longue page. L'ouverture de la page et la frappe doivent rester fluides, et les pastilles suivre leurs blocs pendant que tu tapes au-dessus.
- [ ] **M4. Rechargement pendant un brouillon.** Tape un commentaire sans l'envoyer, puis recharge la page (F5). Le brouillon est perdu : c'est voulu (il ne vit qu'en mémoire), mais dis-nous si c'est gênant.

Remarques :

## Limites connues de cette partie

- Ce qui est tapé dans un bloc pendant qu'une autre personne le déplace est gardé seulement si la frappe arrive au serveur avant le déplacement. Une frappe qui arrive après est perdue (J6, R222). Même dans le bon ordre, rien n'est reporté dans un bloc qui contient une mention, une formule ou un saut de ligne, dans un tableau, quand le type du bloc a changé en même temps, quand la personne qui déplace a effacé du texte du bloc, quand la place de la frappe ne se lit pas à coup sûr, ou au-delà de 64 sous-blocs ou d'environ 64 Kio à reporter d'un coup ; et, rarement, le texte reporté peut tomber un mot plus loin (R520).
- Annuler un déplacement retire aussi ce que les autres ont écrit dans le bloc depuis.
- Deux personnes qui déplacent le même bloc au même instant le retrouvent à une seule des deux places ; ses fils de commentaires peuvent se détacher chez la seconde (R376).
- Restaurer une version remplace tout le contenu de la page : ce que quelqu'un tape au même instant, sur le contenu remplacé, est perdu (R188). La confirmation le dit quand d'autres ont la page ouverte.
- L'historique se lit à partir de « Peut modifier » : quelqu'un qui ne fait que lire une page ne voit pas son historique (R203).
- Une page garde ses cent dernières versions, dont vingt « avant une restauration » au plus ; les plus anciennes partent.
- Les commentaires ont des plafonds : par page, 2 000 fils, 10 000 messages, 4 Mio de texte et 20 000 réactions ; 500 messages par fil ; 32 emojis différents par message ; et une part par personne. Le message du serveur le dit, et qui a l'accès complet peut supprimer des fils pour faire de la place.
- Une mention dans un commentaire ne prévient personne : la boîte de réception viendra plus tard.
- Un commentaire pas encore envoyé ne survit pas au rechargement de l'onglet (M4).
- La liste des présents montre à chacun l'identifiant du compte des autres présents, qui dit sa date de création (rien ne l'affiche, mais un outil le lit).
- Pendant qu'une personne est coupée du réseau, quelqu'un qui peut modifier la page peut, dans un cas rare (la page restée vide plus de trente secondes entre-temps), cacher son nom et son curseur à son retour. Le contenu n'est jamais en jeu (R109).
- Une personne invitée, même en lecture seule, peut encore surcharger le serveur si elle le veut : n'invite que des gens de confiance ; les plafonds viennent au lot 7 (R121).

## Comment rendre le résultat

Donne la liste des numéros qui ne vont pas, dans la conversation où le lot a été lancé. Pour chacun : une phrase qui dit ce que tu as vu à la place, et si possible une capture d'écran. Les lignes « Remarques : » sont pour tout le reste : une gêne, une idée, un mot qui sonne faux. Chaque écart sera repris au registre du lot 3, avec la décision qu'il appelle.

Si tout est coché, dis-le aussi : c'est ce qui clôt la partie A du lot 3.
