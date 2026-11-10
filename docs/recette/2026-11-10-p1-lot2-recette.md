# Recette du lot 2 : comptes, espaces, droits et corbeille

Pour Alex, propriétaire de l'instance. Écrit le 10 novembre 2026.

Tu vas essayer l'application toi-même, ce document sous les yeux. Compte 45 minutes environ.

Tout ce qu'un test automatique peut vérifier l'est déjà. Cette liste porte sur ce qu'un test ne juge pas : le dessin, le confort, les parcours vus de tes yeux. Chaque case dit ce que tu fais, puis ce que tu dois voir. Coche si c'est bon. Sinon, note le numéro (« C4 ne va pas »).

Fais les sections dans l'ordre : chacune part de l'état laissé par la précédente.

## Avant de commencer

**Ce qu'il te faut.**

- L'adresse de ton instance (ton serveur Mementum), dans un navigateur récent : Chrome, Edge ou Firefox.
- Une seconde fenêtre, en navigation privée, pour jouer une autre personne. Un second navigateur va aussi. Dans la suite, « fenêtre 1 » est la tienne, « fenêtre 2 » celle de l'autre personne.
- Deux adresses e-mail inventées, par exemple `camille@exemple.fr` et `dana@exemple.fr`. Aucun e-mail n'est envoyé : elles n'ont pas besoin d'exister.

**Cas 1 : ton instance, relancée sur la nouvelle version, avec tes pages du lot 1.**

Arrête d'abord ton serveur du lot 1. Puis relance-le depuis ce dépôt, sur le même dossier de données, dans deux terminaux. Ce sont les commandes du README (« Lancer Mementum ») :

```
corepack pnpm install
cargo run -p mementum-cli -- serve --data-dir <ton dossier de données>
```

```
corepack pnpm -C apps/web dev
```

Remplace `<ton dossier de données>` par le dossier où est ton fichier `mementum.db`. Sans `--data-dir`, le serveur prend `./data`. Si ton instance répondait sur d'autres ports que 8080 et 5173, garde-les : la variante est dans le cas 2.

- Au premier démarrage, le serveur met ta base à jour. Juste avant, il en écrit une copie à côté d'elle, dans le dossier des données : `mementum.db.avant-0002.bak`. Si quelque chose tourne mal, le README dit comment revenir en arrière (« Copies avant migration »).
- Le serveur écrit dans son journal qu'aucun compte n'existe encore. C'est normal : le lot 1 n'avait pas de comptes.
- Tes pages t'attendent dans un espace nommé « Mon espace ». Le premier compte créé en devient propriétaire.

Crée ce premier compte toi-même, depuis cette machine, avant de rouvrir l'instance à qui que ce soit (tunnel coupé). Tant qu'aucun compte n'existe, la première personne qui s'inscrit reçoit toutes tes pages et devient administratrice, et le serveur dit à tout visiteur qu'aucun compte n'existe encore.

**Cas 2 : une instance neuve.** À la racine du dépôt, dans deux terminaux :

```
corepack pnpm install
cargo run -p mementum-cli -- serve --data-dir data/recette
```

```
corepack pnpm -C apps/web dev
```

Puis ouvre `http://localhost:5173`. Le dossier `data/recette` est créé au besoin, et ne touche à aucune autre donnée.

Si le port 5173 ou 8080 est déjà pris, choisis-en d'autres. Sous PowerShell :

```
cargo run -p mementum-cli -- serve --data-dir data/recette --bind 127.0.0.1:8081
```

```
$env:MEMENTUM_E2E_WEB_PORT = "5174"; $env:MEMENTUM_E2E_API_PORT = "8081"; corepack pnpm -C apps/web dev
```

Puis ouvre `http://localhost:5174`. Si `cargo` répond « Accès refusé », un autre serveur Mementum tourne déjà depuis ce dossier : arrête-le d'abord.

**Le premier compte.** Tant qu'aucun compte n'existe, l'application ouvre sur « Bienvenue dans Mementum ». Tu y donnes un nom, une adresse e-mail et un mot de passe de 10 caractères au moins, puis tu cliques « Créer le compte administrateur ». Ce premier compte administre l'instance.

**Deux choses à savoir pendant l'essai.**

- La barre latérale ne suit pas tout à fait en direct ce que font les autres. Elle se met à jour seule en dix secondes au plus, ou tout de suite quand tu reviens sur la fenêtre : clique dedans. Si rien ne bouge après une vingtaine de secondes, recharge la page (F5). C'est une limite connue, pas un défaut.
- Ne rate pas dix fois de suite le mot de passe d'une même adresse : elle serait bloquée un quart d'heure. C'est aussi une limite connue.

## Ce que ce lot contient, et ne contient pas

**Dedans :**

- des comptes, avec une adresse e-mail et un mot de passe ;
- des espaces, comme les espaces de travail de Notion, plusieurs par compte ;
- des membres (invités dans tout l'espace) et des invités (invités sur une page) ;
- des pages privées et des pages partagées, rangées en trois sections ;
- des invitations par lien ;
- le partage d'une page, avec quatre niveaux, dont la lecture seule ;
- la corbeille ;
- le déplacement des pages, à la souris et par un menu.

**Pas encore, c'est prévu pour plus tard :** les bases de données et leurs vues ; les commentaires (le niveau « Peut commenter » existe, mais vaut « Peut lire ») ; la recherche ; l'envoi d'e-mails (un lien d'invitation se copie et s'envoie à la main) ; la connexion avec Google ; les jetons d'API et le serveur MCP ; l'application mobile ; le thème sombre.

Ne signale pas ces absences comme des défauts.

## A. Comptes (4 minutes)

Fenêtre 1.

- [ ] **A1.** Crée le premier compte, comme dit plus haut. Tu dois arriver dans l'application, déjà connecté, sans passer par un écran de connexion.
- [ ] **A2.** Regarde le haut de la barre latérale. Avec tes pages du lot 1 : l'espace s'appelle « Mon espace », et toutes tes pages sont dans la section « Espace ». Sur une instance neuve : l'espace s'appelle « Espace de » suivi de ton nom, et il est vide.
- [ ] **A3.** Avec tes pages du lot 1 seulement : ouvre trois de tes pages. Leur contenu doit être intact.
- [ ] **A4.** Clique le nom de l'espace, en haut à gauche. Un menu s'ouvre. En bas, tu dois voir ton nom, ton adresse et la mention « Admin de l'instance ».
- [ ] **A5.** Dans ce menu, clique « Se déconnecter ». Tu dois voir l'écran « Se connecter ».
- [ ] **A6.** Tape ton adresse et un mot de passe faux, puis Entrée. Tu dois voir, en rouge sous les champs, « Adresse e-mail ou mot de passe invalide ». Tu restes sur l'écran.
- [ ] **A7.** Tape le bon mot de passe, puis Entrée. Tu dois retrouver ton espace et tes pages.
- [ ] **A8.** Clique le nom de l'espace, puis « Changer le mot de passe ». Donne l'actuel et un nouveau, puis valide. Tu dois lire « Mot de passe changé ». Clique « Fermer ».
- [ ] **A9.** Déconnecte-toi, puis reconnecte-toi avec le nouveau mot de passe. Tu dois entrer.

Remarques :

## B. Espaces (3 minutes)

Fenêtre 1.

- [ ] **B1.** Clique le nom de l'espace, puis « Créer un espace ». Nomme-le « Perso » et clique « Créer l'espace ». Tu dois arriver dans « Perso », vide : ses sections disent « Aucune page pour l'instant. ».
- [ ] **B2.** Clique le nom de l'espace. Tu dois voir tes deux espaces, chacun avec ton rôle, « Propriétaire », sous son nom, et une coche au bout de la ligne de celui qui est ouvert.
- [ ] **B3.** Clique ton premier espace dans ce menu. Tu dois y retrouver ses pages. Aucune page d'un espace ne doit apparaître dans l'autre.
- [ ] **B4.** Clique le nom de l'espace, puis « Réglages de l'espace ». Tu dois voir ton rôle, « Propriétaire », puis quatre parties : « Nom », « Membres », « Invitations » et « Quitter l'espace ».
- [ ] **B5.** Change le nom de l'espace, puis clique « Enregistrer ». Tu dois lire « Nom enregistré. », et le nouveau nom doit s'afficher en haut de la barre latérale.
- [ ] **B6.** Passe dans « Perso », ouvre ses réglages, clique « Quitter l'espace », puis confirme. Tu en es le seul propriétaire : le serveur doit refuser, avec un message qui te dit d'en nommer un autre d'abord. Tu restes dans l'espace. Reviens ensuite dans ton premier espace.

Remarques :

## C. Inviter quelqu'un dans l'espace, par un lien (5 minutes)

- [ ] **C1.** Fenêtre 1 : ouvre les réglages de ton premier espace (son nom, puis « Réglages de l'espace »). Dans la partie « Invitations », tape `camille@exemple.fr`, garde le rôle « Membre », clique « Inviter ». Tu dois voir un cadre qui dit « Ce lien ne sera plus affiché : copie-le maintenant. », avec le lien.
- [ ] **C2.** Clique « Copier le lien ». Le bouton doit dire « Lien copié ».
- [ ] **C3.** Regarde sous « En attente ». L'invitation doit y être, avec l'adresse, « Membre » et sa date de fin, dans 7 jours.
- [ ] **C4.** Fenêtre 2 : colle le lien dans la barre d'adresse. Tu dois lire ton nom, suivi de « t'invite dans l'espace » et du nom de l'espace, et voir deux boutons : « Créer mon compte » et « J'ai déjà un compte ».
- [ ] **C5.** Fenêtre 2 : regarde la barre d'adresse. Elle doit finir par `/invite`. La longue suite de lettres du lien n'y est plus.
- [ ] **C6.** Fenêtre 2 : clique « Créer mon compte ». L'adresse est déjà remplie. Donne le nom « Camille » et un mot de passe, puis valide. Camille doit arriver dans ton espace : le nom de ton espace est en haut de sa barre latérale, au-dessus des sections « Espace » et « Privé ».
- [ ] **C7.** Fenêtre 1 : recharge les réglages de l'espace. « Membres » doit compter 2 personnes, et l'invitation ne doit plus être « En attente ».
- [ ] **C8.** Fenêtre 1 : dans la liste des membres, passe Camille de « Membre » à « Admin ». Fenêtre 2 : clique le nom de l'espace. Dans le menu, sous ce nom, tu dois lire « Admin ».
- [ ] **C9.** Fenêtre 1 : remets Camille « Membre ». Fenêtre 2 : ferme le menu, puis rouvre-le. Tu dois lire « Membre ».
- [ ] **C10.** Ouvre un nouvel onglet dans la fenêtre 2 et colle encore le même lien. Tu dois lire « Cette invitation n'est plus valable » : un lien ne sert qu'une fois. Ferme ensuite cet onglet.

Remarques :

## D. Les trois sections de la barre latérale (3 minutes)

La barre latérale range les pages en trois sections : « Espace » (ce que voient tous les membres), « Partagé » (ce qui est partagé avec des personnes précises) et « Privé » (ce que tu es seul à voir). « Partagé » n'apparaît que s'il y a quelque chose dedans.

- [ ] **D1.** Fenêtre 1 : survole le titre « Privé ». Un « + » apparaît à sa droite. Clique-le. Une page doit se créer dans « Privé », le curseur dans son titre. Tape « Notes privées », puis Entrée.
- [ ] **D2.** Fenêtre 2 (Camille) : clique dans la fenêtre. « Notes privées » ne doit apparaître nulle part.
- [ ] **D3.** Fenêtre 1 : clique le « + » du titre « Espace ». Tape « Page d'équipe », puis Entrée. La page doit être dans la section « Espace ».
- [ ] **D4.** Fenêtre 2 : clique dans la fenêtre. « Page d'équipe » doit apparaître dans la section « Espace » de Camille.
- [ ] **D5.** Fenêtre 2 : ouvre « Page d'équipe » et tape une ligne. Camille doit pouvoir écrire.

Remarques :

## E. Pages : créer, renommer, imbriquer, déplacer (5 minutes)

Fenêtre 1. Les boutons « … » et « + » d'une ligne n'apparaissent qu'au survol.

Dans cette section, ne déplace jamais « Notes privées » : elle doit rester tout en haut de « Privé », sans page parente, pour la section F.

- [ ] **E1.** Clique l'icône du crayon, en haut de la barre latérale (« Nouvelle page »). Une page doit se créer dans « Privé », le curseur dans son titre.
- [ ] **E2.** Tape « Idées », puis Entrée. Le titre doit changer aussi dans la barre latérale et dans la barre du haut.

Crée de la même façon deux autres pages : « Courses », puis « Lectures ». « Privé » a maintenant quatre pages, dans cet ordre : « Notes privées », « Idées », « Courses », « Lectures ».

- [ ] **E3.** Survole la ligne de « Page d'équipe », clique son « + ». Une sous-page doit se créer sous elle, décalée vers la droite. Tape « Sous-page », puis Entrée.
- [ ] **E4.** Glisse « Lectures » sur le milieu de la ligne « Idées ». La ligne « Idées » doit s'éclairer. Relâche : « Lectures » doit devenir sa sous-page.
- [ ] **E5.** Glisse « Lectures » vers le bord haut de la ligne « Courses ». Un trait vert doit apparaître au-dessus de « Courses ». Relâche : « Lectures » doit sortir de « Idées », et se ranger entre « Idées » et « Courses », au même niveau qu'elles.
- [ ] **E6.** Clique le « … » de « Courses », la dernière page de « Privé ». Dans le menu, « Déplacer vers le bas » doit être grisé : elle est déjà en bas.
- [ ] **E7.** Dans le même menu, clique « Déplacer vers le haut ». « Courses » doit monter d'un cran, au-dessus de « Lectures ».
- [ ] **E8.** Clique le « … » de « Lectures », puis « Imbriquer dans la page précédente ». « Lectures » doit devenir une sous-page de « Courses ».
- [ ] **E9.** Clique le « … » de « Sous-page », puis « Sortir de la page parente ». Elle doit arriver à la racine de la section « Espace », juste après « Page d'équipe ».
- [ ] **E10.** Fenêtre 2 : clique dans la fenêtre. Camille doit toujours voir « Sous-page », dans « Espace » : une page sortie à la racine reste une page d'équipe.
- [ ] **E11.** Fenêtre 1 : essaie de glisser « Idées » vers la section « Espace ». Rien ne doit bouger. Une page change de section par le menu Partager, pas à la souris.

« Privé » se termine ainsi : « Notes privées », « Idées », puis « Courses », avec « Lectures » dedans.

Remarques :

## F. Partager une page (8 minutes)

Fenêtre 1 : ouvre « Notes privées ». Elle doit être à la racine de « Privé », sans page parente. Si elle a été rangée sous une autre page, clique son « … », puis « Sortir de la page parente ».

- [ ] **F1.** Clique « Partager », en haut à droite. Un panneau doit s'ouvrir sous le bouton, le curseur dans le champ « Adresse e-mail ». Tu dois t'y voir, marqué « (toi) », en « Accès complet ».
- [ ] **F2.** Tape `dana@exemple.fr`, choisis « Peut lire » dans la liste à côté, clique « Inviter ». Tu dois lire « Copie ce lien maintenant : il ne sera plus affiché une fois la page fermée. », et voir le lien, avec un bouton « Copier le lien d'invitation ». Copie-le.
- [ ] **F3.** Clique en dehors du panneau, puis rouvre « Partager ». Le lien de l'invitation doit être encore là : il reste affiché tant que l'invitation attend et que la page est ouverte.
- [ ] **F4.** Fenêtre 2 : déconnecte Camille (le nom de l'espace, puis « Se déconnecter »), puis colle le lien. Tu dois lire ton nom, suivi de « t'invite sur la page « Notes privées » », et le nom de l'espace où elle est.
- [ ] **F5.** Fenêtre 2 : clique « Créer mon compte », donne le nom « Dana » et un mot de passe. Dana doit arriver directement sur « Notes privées ».
- [ ] **F6.** Fenêtre 2 : regarde la barre latérale de Dana. Elle ne doit avoir qu'une section, « Partagé », avec cette seule page.
- [ ] **F7.** Fenêtre 2 : regarde la page. Un bandeau doit dire « Tu peux lire cette page, pas la modifier. ». Dana ne peut taper ni dans le titre, ni dans le texte.
- [ ] **F8.** Fenêtre 2 : cherche l'icône du crayon, en haut de la barre latérale, et le bouton « Partager ». Dana ne doit avoir ni l'une ni l'autre : une invitée ne crée pas de page à la racine, et ne partage qu'avec l'accès complet.
- [ ] **F9.** Fenêtre 1 : clique dans la page, puis regarde la barre latérale. « Notes privées » doit être passée de « Privé » à « Partagé ».
- [ ] **F10.** Fenêtre 1 : rouvre « Partager ». Dana est maintenant dans la liste des personnes. Sur sa ligne, passe de « Peut lire » à « Peut modifier ». Fenêtre 2 : un bandeau doit dire « Tu peux maintenant modifier cette page : rouvre-la. », avec un bouton « Rouvrir la page ». Clique-le : Dana doit pouvoir écrire.
- [ ] **F11.** Fenêtre 1 : passe Dana en « Accès complet ». Fenêtre 2 : en une ou deux secondes, Dana doit avoir le bouton « Partager ».
- [ ] **F12.** Fenêtre 2 : clique ce bouton. Dans le panneau de Dana, l'accès général se lit sans se régler : « Seuls les membres de l'espace règlent l'accès général. ». Ferme le panneau (Échap).
- [ ] **F13.** Fenêtre 1 : sous « Accès général », coche « Tous les membres de l'espace ». Une liste de niveaux doit apparaître à côté, sur « Peut lire ». Dans la barre latérale, la page doit passer dans « Espace ».
- [ ] **F14.** Fenêtre 1 : recoche « Seulement les invités ». Une fenêtre doit demander « Fermer cette page aux membres ? », avec un bouton rouge « Fermer aux membres ». Clique-le : la page doit revenir dans « Partagé ».
- [ ] **F15.** Fenêtre 1 : clique le « + » de « Page d'équipe », tape « Compte rendu », puis Entrée. Sur cette sous-page, clique « Partager ». « Tous les membres de l'espace » doit être coché et marqué « hérité », et « Seulement les invités » grisé : une page parente ouvre déjà cette page aux membres.
- [ ] **F16.** Dans ce panneau, ouvre la liste des niveaux, à côté de « hérité ». Seul « Accès complet » doit se choisir : les niveaux plus bas sont grisés.
- [ ] **F17.** En bas du panneau, clique « Copier le lien ». Le bouton doit dire « Lien copié ». Appuie sur Échap : le panneau doit se fermer.
- [ ] **F18.** Fenêtre 1 : clique le « + » du titre « Espace », tape « Page reprise », puis Entrée. Clique « Partager », coche « Seulement les invités », puis confirme par « Fermer aux membres ». Tu dois rester sur la page, nommé « (toi) » en « Accès complet » dans le panneau, et la page doit passer de « Espace » à « Privé » dans ta barre latérale : une page d'équipe peut redevenir privée.

Remarques :

## G. Écrire à deux, en direct (2 minutes)

Fenêtre 1 : ouvre « Notes privées ». Fenêtre 2 : Dana y est déjà, avec l'accès complet. Mets les deux fenêtres côte à côte.

- [ ] **G1.** Tape une phrase dans la fenêtre 1. Elle doit apparaître dans la fenêtre 2 au fil de la frappe, sans retard visible.
- [ ] **G2.** Pendant que tu tapes dans la fenêtre 1, regarde la fenêtre 2. Tu dois y voir ton curseur, en couleur, avec ton nom à côté.
- [ ] **G3.** Tape dans les deux fenêtres, à deux endroits de la page, en alternant vite. Aucune lettre ne doit se perdre, et les deux fenêtres doivent montrer le même texte.

Remarques :

## H. Corbeille (4 minutes)

La fenêtre 2 reste sur « Notes privées », avec Dana.

- [ ] **H1.** Fenêtre 1 : crée une page avec le crayon, et nomme-la « À jeter ». Clique son « + » pour lui faire une sous-page. Clique ensuite le « … » de « À jeter », puis « Mettre à la corbeille ». Les deux lignes doivent quitter la barre latérale.
- [ ] **H2.** Clique « Corbeille », en bas de la barre latérale. « À jeter » doit y être, avec la date de sa mise à la corbeille, et deux boutons : « Restaurer » et « Supprimer définitivement ».
- [ ] **H3.** Clique le titre « À jeter » dans la corbeille. La page doit s'ouvrir avec un bandeau « Cette page est à la corbeille. ». Tu peux la lire, pas y écrire.
- [ ] **H4.** Clique « Restaurer » dans le bandeau. La page et sa sous-page doivent revenir dans « Privé », à leur place.
- [ ] **H5.** Remets « À jeter » à la corbeille par son « … », ouvre la corbeille, clique « Supprimer définitivement ». Une fenêtre doit demander confirmation, avec un bouton rouge. Confirme : la ligne doit disparaître, et la corbeille doit dire « La corbeille est vide. ».
- [ ] **H6.** Fenêtre 1 : dans « Partagé », clique le « … » de « Notes privées », puis « Mettre à la corbeille ». Fenêtre 2 : en une ou deux secondes, le bandeau « Cette page est à la corbeille. » doit apparaître, et Dana ne doit plus pouvoir écrire.
- [ ] **H7.** Fenêtre 1 : ouvre la corbeille et clique « Restaurer » sur « Notes privées ». Fenêtre 2 : le bandeau doit partir, et Dana doit pouvoir écrire de nouveau.

Remarques :

## I. Perdre un droit pendant qu'on a la page ouverte (3 minutes)

Fenêtre 1 : ouvre « Notes privées ». Fenêtre 2 : Dana l'a toujours ouverte, en « Accès complet ».

- [ ] **I1.** Fenêtre 1 : clique « Partager », et passe Dana de « Accès complet » à « Peut lire ». Fenêtre 2 : le bandeau « Tu peux lire cette page, pas la modifier. » doit apparaître aussitôt, et ce que tape Dana ne doit plus entrer.
- [ ] **I2.** Fenêtre 1 : pendant ce changement, regarde la barre du haut. Elle ne doit pas afficher « Hors ligne », même un instant.
- [ ] **I3.** Fenêtre 1 : sur la ligne de Dana, clique « Retirer ». Une fenêtre doit demander « Retirer l'accès de Dana ? », avec un bouton rouge « Retirer ». Confirme : Dana doit quitter la liste.
- [ ] **I4.** Fenêtre 2 : sans rien toucher, tu dois lire « Tu n'as plus accès à cette page ». Le texte de la page ne doit plus être à l'écran.
- [ ] **I5.** Fenêtre 2 : recharge la page. Tu dois lire « Page introuvable » : pour Dana, cette page est maintenant comme une page qui n'existe pas.

Remarques :

## J. Le dessin : direction A « Papier » (4 minutes)

Le dessin doit ressembler à Notion sans le copier : mêmes emplacements, tons de papier, un seul vert. Regarde chaque écran d'un œil critique, et note ce qui s'en écarte.

- [ ] **J1. Connexion.** Fenêtre 1 : déconnecte-toi. Tu dois voir une colonne étroite, centrée, sur fond blanc : le carré vert « M » et « Mementum », un titre en gras, une phrase grise dessous, deux champs à bordure fine, un bouton vert plein sur toute la largeur, et une ligne en pied sur l'inscription. Rien d'autre à l'écran. Reconnecte-toi ensuite.
- [ ] **J2. Barre latérale.** Fond gris chaud très clair, séparé de la page par un trait fin. Les titres de section sont petits et gris. Les lignes sont serrées et régulières. La page ouverte a un fond un peu plus foncé. « Corbeille » reste collée en bas, même quand la liste défile.
- [ ] **J3. Page.** Une colonne blanche centrée, large de 760 pixels au plus. Le titre est grand et gras. Il est aligné, à gauche, sur le texte qui le suit.
- [ ] **J4. Barre du haut.** Elle est fine : le titre de la page à gauche, en gris, « Partager » à droite. Ouvre la corbeille, qui n'a pas ce bouton, puis reviens sur une page : la barre doit garder la même hauteur, et le contenu ne doit pas sauter.
- [ ] **J5. Menu Partager.** Un panneau blanc sous le bouton, aligné sur son bord droit, coins arrondis, ombre douce. « Inviter » est le seul bouton vert plein, avec « Copier le lien d'invitation » quand un lien d'invitation est affiché. « Retirer » et « Révoquer » sont discrets, en simple texte. Le choix coché de l'accès général est sur fond gris clair, avec une pastille verte. Aucun texte ne déborde ni ne passe à la ligne pour un seul mot.
- [ ] **J6. Corbeille et réglages.** Même colonne que la page, mêmes titres. Les parties sont séparées par un trait fin. « Retirer », « Révoquer » et « Supprimer définitivement » sont discrets dans les listes, et ne rougissent qu'au survol. Au repos, le rouge est gardé pour « Quitter l'espace », pour les messages d'erreur et pour les boutons des fenêtres de confirmation.
- [ ] **J7. Écrans d'erreur.** Dans la fenêtre 1, change le dernier caractère de l'adresse d'une page, puis Entrée. Tu dois lire « Page introuvable », avec une phrase simple, en français. Aucun code technique, aucun mot d'anglais.
- [ ] **J8. Survol et clavier.** Survole les lignes, les boutons, les entrées de menu : chacun doit réagir par un léger changement de fond. Appuie plusieurs fois sur Tab : ce qui a le focus doit porter un contour vert, bien visible.
- [ ] **J9. Fenêtre étroite.** Réduis la fenêtre à la moitié de l'écran. La barre latérale garde sa largeur, le texte de la page se resserre, et le menu Partager reste en entier dans la fenêtre. Il n'y a pas encore de mise en page pour téléphone : en dessous de cette largeur, ne signale rien.

Remarques :

## K. Les cas de bord (5 minutes)

- [ ] **K1. Réseau coupé.** Fenêtre 1 : ouvre « Idées ». Ouvre les outils du navigateur (F12), onglet « Réseau », et choisis « Hors connexion » (« Offline »). Tape quelques mots. Après une ou deux secondes, la barre du haut doit dire « Hors ligne : tes modifications partiront au retour du réseau ». Tu peux encore écrire. Si la mention ne vient pas, ton navigateur ne coupe pas ce type de connexion : arrête plutôt le serveur (Ctrl+C dans son terminal).
- [ ] **K2. Réseau rétabli.** Remets le réglage d'origine dans le même onglet (ou relance le serveur). La mention doit partir. Recharge la page : ce que tu as tapé hors ligne doit être là.
- [ ] **K3. Rechargement en pleine frappe.** Tape une phrase et appuie sur F5 dans la seconde. Après le rechargement, la phrase doit être là.
- [ ] **K4. Une page sans y avoir droit.** Fenêtre 1 : copie l'adresse de « Idées ». Fenêtre 2 : déconnecte Dana (elle n'a plus d'espace : clique « Mementum », en haut à gauche, puis « Se déconnecter »), connecte-toi avec le compte de Camille, puis colle cette adresse. Tu dois lire « Page introuvable », exactement comme pour l'adresse inventée de J7. Rien ne doit dire que la page existe.
- [ ] **K5. Très longue page.** Fenêtre 1 : colle un très long texte dans « Idées » (plusieurs centaines de lignes). La frappe et le défilement doivent rester fluides.
- [ ] **K6. Titre très long.** Donne à « Idées » un titre de trois lignes. Dans la page, il passe à la ligne. Dans la barre latérale, il est coupé par des points de suspension, sur une seule ligne, sans passer sous les boutons « … » et « + ».
- [ ] **K7. Bouton Précédent.** Ouvre trois pages l'une après l'autre, puis clique deux fois Précédent. Tu dois revenir à la première, et la ligne de la page ouverte doit suivre dans la barre latérale.
- [ ] **K8. Précédent après la déconnexion.** Déconnecte-toi, puis clique Précédent. Tu ne dois pas revoir tes pages : l'écran « Se connecter » doit revenir.
- [ ] **K9. Deux onglets du même compte.** Reconnecte-toi, puis ouvre la même page dans deux onglets de la fenêtre 1. Tape dans l'un : le texte doit apparaître dans l'autre.
- [ ] **K10. Déconnexion dans un onglet.** Déconnecte-toi dans l'un des deux. Dans l'autre, une fenêtre « Ta session a pris fin » doit s'ouvrir par-dessus la page, sans la fermer. Reconnecte-toi là, avec le même compte : la page doit être restée, avec son texte.

Remarques :

## Limites connues de ce lot

Elles sont voulues, ou reportées à un lot suivant. Ne les signale pas comme des découvertes.

- Dix mots de passe faux pour une même adresse la bloquent, un quart d'heure au plus, même avec le bon mot de passe. Quelqu'un peut donc empêcher une personne de se connecter. Ses fenêtres déjà ouvertes continuent (R76).
- L'écran d'inscription dit si une adresse a déjà un compte (R77).
- Même quand on ne s'inscrit que sur invitation, quelqu'un du dehors peut savoir si une adresse a un compte, en une douzaine d'essais par adresse s'il en sonde beaucoup à la fois (R84).
- Il faut se reconnecter 30 jours après sa dernière connexion, même si l'on ouvre l'application tous les jours (R117).
- Une invitée à qui tu donnes l'accès complet à une page peut y inviter d'autres personnes, comme dans Notion (R86).
- Une page sortie à la racine garde son accès d'équipe, mais les personnes nommées sur son ancienne page parente la perdent (R94).
- La barre latérale ne suit pas tout à fait en direct ce que font les autres : elle se met à jour seule en dix secondes au plus, ou tout de suite quand tu reviens sur la fenêtre (R97, R128).
- Quelqu'un qui peut modifier une page peut, pendant qu'une autre personne est coupée du réseau, prendre sa place dans l'affichage des curseurs : à son retour, elle écrit normalement, mais son nom et son curseur ne se voient plus chez les autres tant qu'il reste là. Le contenu n'est pas en jeu. Quelqu'un qui ne fait que lire la page ne le peut pas (R109, R118).
- Par une manœuvre du même genre, quelqu'un qui peut modifier la page peut la laisser invisible même après être parti lui-même, jusqu'à ce que tout le monde ait quitté la page. Là encore, seuls le nom et le curseur sont touchés (R109, R118).
- Une personne invitée, même en lecture seule, peut surcharger le serveur si elle le veut. Pour l'instant, n'invite que des gens de confiance : les plafonds viennent au lot 7 (R121).
- Si tu recharges huit fois un onglet, un autre onglet du même compte, ouvert sur la même page, perd sa connexion un instant, et revient seul (R109).
- Après une coupure du réseau, le nom et le curseur d'une personne qui ne tape pas peuvent mettre jusqu'à 30 secondes à revenir chez les autres. Dès qu'elle tape ou déplace son curseur deux fois, ils reviennent (reporté au lot suivant).
- Un lien d'invitation donne l'accès à qui le détient : l'adresse e-mail n'est pas vérifiée, et rien n'est envoyé (R47).
- Un niveau qui monte ne s'applique pas tout seul : il faut cliquer « Rouvrir la page ». C'est voulu, pour ne pas changer l'éditeur sous les yeux de qui lit (R111).
- Sous une page ouverte aux membres, « Seulement les invités » est grisé : une sous-page ne peut pas être plus fermée que sa page parente dans ce lot (R93, R110).
- Après un `mementum user set-password` sur le serveur, une page déjà ouverte dans un navigateur reste connectée jusqu'à sa prochaine reconnexion. Pour chasser quelqu'un, il faut redémarrer le serveur après la commande (R125).
- Si un autre compte se connecte dans le même navigateur, l'onglet resté ouvert sous le premier se recharge : tout de suite si c'est dans la fenêtre « Ta session a pris fin », à ton retour sur l'onglet si c'est dans un autre onglet. C'est voulu : le texte tapé par l'un ne doit pas partir sous le nom de l'autre (R115, R126).
- Un simple membre peut fermer aux autres membres une page d'équipe, après la confirmation « Fermer cette page aux membres ? ». Elle disparaît alors pour tous les autres, propriétaire et admins compris, et lui reste (R122, R127).
- Deux onglets du même navigateur ne se parlent plus directement : tout passe par le serveur. Hors ligne, ce que tu tapes dans l'un n'apparaît dans l'autre qu'au retour du réseau.
- Dans le menu Partager, rien ne dit si une personne est membre ou invitée. La confirmation de « Retirer » parle donc d'une nouvelle invitation pour tout le monde (reporté au lot suivant).
- La corbeille est une liste à plat. Pour restaurer une sous-page partie avec sa page parente, c'est la page parente qu'il faut restaurer, et le message le dit (connu, à améliorer plus tard).
- Le premier compte porte la mention « Admin de l'instance », mais elle ne lui donne encore aucun pouvoir dans l'application. Lui non plus ne voit pas les pages privées des autres (R49).
- Un mot de passe oublié se règle sur le serveur, par une commande : il n'y a pas d'e-mail de secours (R47).
- On ne peut pas encore supprimer un espace ni un compte, et la corbeille ne se vide pas toute seule (R52).
- Le texte d'attente d'une page vide vouvoie (« Entrez du texte… »), alors que le reste de l'application tutoie. Il vient de l'éditeur, depuis le lot 1.

## Comment rendre le résultat

Donne la liste des numéros qui ne vont pas, dans la conversation où le lot a été lancé. Pour chacun : une phrase qui dit ce que tu as vu à la place, et si possible une capture d'écran. Les lignes « Remarques : » sont pour tout le reste : une gêne, une idée, un mot qui sonne faux.

Si tout est coché, dis-le aussi : c'est ce qui clôt le lot.
