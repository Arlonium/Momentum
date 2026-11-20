# Recette du lot 4 : la recherche

Pour le propriétaire de l'instance (Alex), ou Lenny. Écrit le 20 novembre 2026, pendant la passe complète de fin de lot : si elle trouve un défaut qui touche ce que tu vois, la correction groupée peut changer une case. Le registre du lot le dira.

Tu vas essayer l'application toi-même, ce document sous les yeux. Compte une heure environ, trois quarts d'heure si tu sautes les étapes marquées « facultatif ».

Tout ce qu'un test automatique peut vérifier l'est déjà, dont les droits, de bout en bout, dans un navigateur, à plusieurs comptes. Cette liste porte sur ce qu'un test ne juge pas : si le bon résultat vient en tête, si l'on comprend ce que l'écran dit, le confort au clavier, ce qu'on attend et combien de temps. Chaque case dit ce que tu fais, puis ce que tu dois voir. Coche si c'est bon. Sinon, note le numéro (« B4 ne va pas ») et une phrase sur ce que tu as vu à la place, dans la ligne « Remarques : » de la section.

Fais les sections dans l'ordre : chacune part de l'état laissé par la précédente.

## D'abord : les choix de produit pris sans toi

Le 20 novembre, Lenny a répondu « oui » pour pousser au fil du travail (L12), et, pour le reste, « tu es autonome ». Les dix-neuf décisions du plan sont donc **retenues par défaut** : la recommandation du plan, adaptée aux mesures là où il le fallait. Elles sont en service dans la version que tu essaies. Pour chacune : ce qui a été choisi, ce que ça fait, comment y revenir, et l'étape où tu la vois. Dis, dans ta réponse, lesquelles tu gardes et lesquelles tu veux autrement. Le détail (la question, les raisons, le coût) est dans le [plan](../superpowers/plans/2026-11-19-p1-lot4-recherche.md), « Décisions L1 à L19 », et au [registre](../superpowers/logs/2026-11-19-p1-lot4-ledger.md) (R933).

**Les dix-neuf décisions du plan** (retenues par défaut le 2026-11-20, sauf L1, retenue sur mesures, et L12, ta réponse) :

1. **L1, le moteur : Tantivy 0.27.0** (R933, retenu sur les mesures de la Task S0). Il est 1,5 à 7 fois plus rapide qu'un FTS5 de SQLite bien écrit, et tient « la note ne lit rien des autres » par ses fonctions publiques ; FTS5 l'aurait demandé en C. Il coûte une dépendance lourde (64 crates neuves, R970), un second stockage du texte à tenir d'accord avec la base, et 40 s de compilation en release sur 24 cœurs (S0). Y revenir : passer à FTS5 refait le cœur du lot (les tâches S3, S4c et S7). Rien ne le montre à l'écran.
2. **L2, où l'on cherche : dans l'espace affiché**, comme Notion. Chercher dans tous ses espaces demanderait de calculer les droits de chaque espace à chaque frappe. Y revenir : une recherche par espace, réunie par le client, ou un calcul de droits de plus par espace (section E, E6).
3. **L3, ce qui est cherché** : les titres, le texte des blocs, les légendes et les noms de fichiers, l'adresse d'un signet, le LaTeX d'une formule ; **pas** les commentaires, ni le contenu des fichiers (PDF), ni l'historique, ni les dossiers. Y revenir : les commentaires seraient le premier ajout (ils ont leurs droits à eux) (section B).
4. **L4, la corbeille : jamais dans les résultats, mais gardée dans l'index**, pour qu'une page restaurée se retrouve aussitôt. Son texte reste donc dans l'index jusqu'à la suppression définitive. Y revenir : la retirer de l'index à la mise à la corbeille et la réindexer à la restauration (étape E4).
5. **L5, la forme : une fenêtre de recherche**, ouverte par « Rechercher » dans la barre latérale et par Ctrl+K (Cmd+K sur Mac), sauf quand du texte est sélectionné dans une page que tu peux modifier : Ctrl+K y reste « créer un lien », comme dans Notion. Pas de palette d'actions ni de questions à l'IA (plus tard, dans la même fenêtre). Y revenir : changer de touche est d'une ligne (étapes B1 et B2).
6. **L6, la tolérance : sans casse ni accents, tous les mots voulus, le dernier complété** (« ornith » trouve « ornithorynque ») ; **pas de racinisation** (« pages » ne trouve pas « page ») ni de tolérance aux fautes. Une racinisation française fausserait les textes anglais d'un même espace ; l'essai de S0 n'est qu'une indication (dix paires françaises réunies sur vingt, six paires anglaises à tort sur trente). Y revenir : une racinisation est une fonction du moteur, à mesurer d'abord (étapes B5 et B6).
7. **L7, ni filtre ni tri à l'écran** : tri par pertinence seul ; les filtres (type, date, auteur) viennent au lot 5 avec l'outil `search` des agents.
8. **L8, les rétroliens : « Pages qui mènent ici », sous le texte de la page**, absente quand il n'y en a pas ; y comptent une mention de page, un lien vers une page, un lien du texte vers une page de l'instance ; seules les pages que tu vois, du même espace. Autre place possible : un compteur dans la barre du haut qui ouvre un panneau (section D).
9. **L9, le délai d'indexation : 2 s après la dernière frappe, 15 s au plus pendant une frappe continue** ; un texte effacé reste trouvable ce temps-là, par qui lit la page. C'est le prix de ne rien mettre sur le chemin d'une frappe. Y revenir : un délai plus court se règle (`MEMENTUM_INDEX_AFTER_MS`), au prix de plus d'indexations (section F).
10. **L10, les budgets de la recherche**, adoptés au §5.10 de la conception tels que mesurés à la Task X2 (R1163, R1165, R1173) : la réponse du serveur sous 100 ms au p95, temps plancher compris, sur un espace de 10 000 pages et 100 000 blocs ; de la dernière touche au résultat affiché sous 400 ms au p95 (224 ms mesurés, 299 ms sous un processeur ralenti de moitié : les 300 ms que proposait le plan ne tenaient pas avec une marge) ; un mot tapé trouvable en moins de 5 s. Ils ne valent pas sur un million de blocs pour des mots que toutes les pages portent (choix 23, plus bas).
11. **L11, `[[Page#Titre]]` (lien vers un titre d'une page) : dans aucun lot pour l'instant.** Il demande un choix de schéma, à faire avec le lot 5 ou le lot 6 (l'import d'Obsidian en a besoin).
12. **L12, pousser : oui, au fil du travail, pour tous les lots** (ta réponse du 20 novembre, R934). Rappelé pour mémoire.
13. **L13, une instance qui a déjà des pages est indexée seule, en fond, au premier démarrage**, et la recherche dit qu'elle est incomplète tant que ce n'est pas fini. L'autre choix : une commande à lancer par l'exploitant (étape G6, facultative).
14. **L14, un titre mentionné devient trouvable** : une mention ou un lien de page copie le titre de la page dans le document (R251, lot 3) ; on trouve donc la page qui porte la mention en cherchant le titre de la page mentionnée, même sans voir celle-ci. Rien de neuf n'est montré (la mention se lit déjà), mais c'est plus facile à trouver. Si tu défais R251, la recherche suit sans autre travail (étape E3).
15. **L15, cent pages au plus par recherche**, vingt par réponse : au-delà, il faut préciser la recherche, et rien ne dit combien il en reste (étape B9, facultative).
16. **L16, une recherche fait 200 caractères et 8 mots au plus**, sans opérateur (ni guillemets pour une expression exacte, ni « sauf ») ; au-delà, un refus qui le dit. L'expression exacte serait le premier ajout (étape B8).
17. **L17, le texte d'un bloc que cette version ne connaît pas (`unknown`) n'est pas cherché.**
18. **L18, le texte d'une page est gardé trois fois** : dans le document, dans la projection de la base (`blocks_index`) et dans l'index plein texte (pour l'extrait). Ensemble, de l'ordre de quatre fois le texte des pages sur le disque (R931). Ne plus garder le texte dans l'index rendrait 23 % de sa place (mesuré en S3), au prix d'une lecture de plus par résultat (étape G7, facultative).
19. **L19, un index pour toute l'instance, et un temps de réponse plancher de 50 ms par recherche** (R932, arrêté à 50 ms en X2, R1166). Sans plancher, le temps d'une recherche dirait si un mot est courant dans les pages que tu ne vois pas. Toute recherche du serveur coûte donc 50 ms au moins ; les titres de l'arbre, eux, paraissent sans attendre. Le plancher ne couvre pas tout : choix 23.

**Pris en route, pendant l'exécution et les relectures** (une ligne chacun) :

20. **Les rétroliens ont un temps de réponse plancher de 150 ms** (R1022, monté de 75 à 150 ms en X2, R1167), pour la même raison : leur lecture ne doit rien dire des pages qui mènent ici et que tu ne vois pas. Aucun rendu ne les attend (ils sont lus après l'ouverture de la page). Y revenir : baisser le plancher demande de baisser aussi la borne du parcours (choix 31).
21. **Une recherche calculée et une en attente par compte ; quatre-vingts recherches calculées par seconde pour toute l'instance** (R910, R1054 : quatre calculs de front, chacun tenu jusqu'au plancher). Une troisième recherche du même compte prend la place de celle qui attendait, refusée (`429`). La fenêtre n'envoie qu'une recherche à la fois, 150 ms après ta dernière frappe, et jette d'elle-même une réponse en retard : ce refus ne se voit qu'avec deux onglets du même compte qui cherchent ensemble, et il est alors dit tel que le serveur le dit (R1085). Y revenir : plus de places, au prix d'un temps qui se lit (choix 23).
22. **Mémoire « au repos » jugée sur la mémoire que le serveur tient en propre** (R924) : les fichiers de l'index, que le système projette en mémoire et reprend sous pression, sont comptés à part. Un exploitant qui lit le RSS voit donc plus (étape G7). Et un tampon d'écriture de l'index de 50 Mo (R925).
23. **Au-delà de ce que couvre le plancher, la durée se lit** (limite L-b, R1166) : sur le poste de mesure, 50 ms couvrent environ 250 000 entrées cachées qui portent huit mots cherchés et 1,3 million qui en portent un ; au million de blocs portant tous les mots cherchés, une recherche de huit mots prend 270 ms environ. Monter le plancher à 100 ms doublerait ces seuils pour 50 ms de plus sur chaque recherche de chacun. Les budgets sont donc posés à 100 000 blocs (R1163).
24. **Une page trouvée par son titre s'ouvre en haut**, même quand le serveur l'a aussi trouvée par un bloc : sa ligne vient de l'arbre, sans extrait ni bloc (R1080, R1109). Y revenir : une hauteur de ligne fixe, qui laisserait la ligne du serveur remplacer celle de l'arbre sans rien déplacer (étape C4).
25. **Les lignes de la fenêtre ne bougent plus une fois montrées** (R1080 à R1082) : les titres de l'arbre sont pris une fois par recherche, les résultats du serveur s'ajoutent dessous, tout ce qui se dit va sous la liste, et la fenêtre est tenue par son bord haut. Conséquence : une page renommée, mise à la corbeille ou dont l'accès est retiré pendant que la fenêtre est ouverte garde sa ligne jusqu'à ta prochaine frappe (étape E5).
26. **Dès que ce que tu tapes change, les résultats d'avant ne sont plus montrés** (R1082) : la liste repart des titres de l'arbre, puis des résultats de la nouvelle recherche.
27. **Ce que tu as tapé est gardé d'une ouverture à l'autre, par espace** (R1086), le temps que l'application reste ouverte : fermée par Échap, la fenêtre se rouvre sur ta dernière recherche, tout sélectionné ; ouvrir un résultat la vide (étapes B1 et C3).
28. **Recliquer un résultat ramène au bloc**, même si l'adresse est déjà la sienne (R1101 ; le plan disait « ne fait rien »), sans ajouter d'entrée à l'historique du navigateur ; le curseur revient au début du bloc. Y revenir : une ligne de `navigate` (étape C3).
29. **Le bloc trouvé est amené au milieu de la fenêtre, et prend le focus dans le geste** (R1108, R1107) ; une arrivée tardive (une page ouverte par son adresse, qui se charge pendant que tu tapes dans le titre) montre le bloc sans te prendre le focus (R1114). Un bloc dans un repli fermé n'ouvre pas le repli (étape C5, facultative).
30. **Un rétrolien dont la page n'est plus dans ton arbre n'est pas montré** (R1117) ; tenu sous le pointeur ou le focus, il reste à sa place sous « Page indisponible », jusqu'à ce que tu en sortes (étape E7).
31. **Rétroliens : deux cents pages au plus, sans total ; le parcours s'arrête à 10 000 pages qui mènent ici** (R1167 ; 20 000 avant X2), sans le dire : au-delà, des rétroliens que tu vois peuvent manquer (limite L-j). Le plancher de 150 ms et cette borne sont arrêtés ensemble.
32. **Rien ne pousse un rétrolien neuf** (R1116) : la section est lue après l'ouverture de la page, au retour sur l'onglet (cinq secondes au moins après la lecture d'avant), au retour de la connexion ; jamais pendant que tu tapes. Une page déjà ouverte ne voit un lien posé ailleurs qu'à ton retour sur son onglet (étape D3).
33. **Le fil d'Ariane d'un résultat ne nomme que les pages que tu vois**, les 32 plus proches au plus (R903, R1044) : un invité sur une sous-page voit un fil qui commence à elle.
34. **Un lien du texte écrit avec l'adresse entière de l'instance (`https://…/p/…`) ne compte comme rétrolien que si le serveur connaît son adresse publique** (`--public-url`, R998) ; et changer d'adresse publique ne réindexe rien de soi-même : il faut `mementum reindex` (étape G2).
35. **Ctrl+K ouvre la recherche de partout, champs compris** (R1087) : dans un champ de texte, il perd le sens que le navigateur lui donne ; sur Mac, Ctrl+K l'ouvre aussi, comme Cmd+K. Tapé pendant qu'un menu « / », `@` ou `[[` est ouvert, il ferme ce menu, qui ne se rouvre pas au retour ; le texte tapé reste (R1144).
36. **Les titres trouvés ne sont pas surlignés**, seuls les mots de l'extrait le sont ; un extrait peut commencer ou finir au milieu d'un caractère composé (un emoji à plusieurs parties) (R969, limites écrites de la Task C1).

## Avant de commencer

**Ce qu'il te faut.**

- Un navigateur récent : Chrome, Edge ou Firefox.
- Une seconde fenêtre, en navigation privée, pour jouer une autre personne. Dans la suite, « fenêtre 1 » est la tienne, « fenêtre 2 » celle de Camille, puis, à la section E, celle de Dom. Mets-les côte à côte.
- Deux adresses e-mail inventées, `camille@exemple.fr` et `dom@exemple.fr`. Aucun e-mail n'est envoyé.
- Une montre, ou l'horloge de ton poste, pour la section F.

**Une instance neuve, pas tes données.** La migration de ce lot (`0005`, l'index de recherche) peut encore changer jusqu'à la clôture du lot : elle a déjà changé trois fois en route, et une base ouverte par cette version pourrait refuser de démarrer avec la suivante. Fais donc cette recette sur un dossier de données à part, **jamais sur `data/instance`**. À la racine du dépôt, dans deux terminaux :

```
corepack pnpm install
cargo run -p mementum-cli -- serve --data-dir data/recette-lot4 --public-url http://localhost:5173
```

```
corepack pnpm -C apps/web dev
```

Puis ouvre `http://localhost:5173` dans les deux fenêtres. `--public-url` dit au serveur l'adresse sous laquelle tu l'ouvres : c'est ce qui fait compter comme rétrolien un lien collé avec l'adresse entière d'une page (étape D4). Si `cargo` répond « Accès refusé », un autre serveur Mementum tourne déjà depuis ce dossier : arrête-le d'abord.

Si le port 5173 ou 8080 est déjà pris, choisis-en d'autres, et donne la même adresse à `--public-url`. Sous PowerShell :

```
cargo run -p mementum-cli -- serve --data-dir data/recette-lot4 --bind 127.0.0.1:8081 --public-url http://localhost:5174
```

```
$env:MEMENTUM_E2E_WEB_PORT = "5174"; $env:MEMENTUM_E2E_API_PORT = "8081"; corepack pnpm -C apps/web dev
```

Puis ouvre `http://localhost:5174`.

**Les mots témoins.** Les étapes cherchent des mots que rien d'autre ne porte (« ornithorynque », « quetzal », « albatros »…). Tape-les tels quels : un mot rare rend la recherche facile à juger.

## Ce que ce lot contient, et ne contient pas

**Dedans :**

- la fenêtre de recherche d'un espace : « Rechercher » dans la barre latérale, Ctrl+K ;
- les titres de l'arbre tout de suite, puis les résultats du serveur, avec un extrait et ses mots surlignés, et le fil d'Ariane ;
- l'arrivée sur le bloc trouvé, pour qui modifie comme pour qui lit ;
- « Pages qui mènent ici », sous le texte d'une page ;
- l'indexation en fond, la commande `mementum reindex`, et l'index qui se reconstruit seul.

**Pas encore** (ne le signale pas comme un défaut) : chercher dans les commentaires, les fichiers, la corbeille ou l'historique ; les filtres ; tous les espaces à la fois ; les fautes de frappe ; l'expression exacte entre guillemets ; la palette de commandes (actions, IA) ; la vue graphe ; `[[Page#Titre]]` ; les mots surlignés dans la page ouverte ; un compteur de résultats.

**Déjà connu, d'avant ce lot** : Échap tapé dans le texte d'une page, sans menu ouvert, fait perdre le focus à l'éditeur, et la frappe suivante ne va nulle part (R1143) ; il est noté pour la fin du lot. Clique dans le texte pour reprendre.

## A. Préparer l'espace (8 minutes)

- [ ] **A1. Les comptes.** Fenêtre 1 : crée ton compte, le premier de l'instance. Invite `camille@exemple.fr` comme « Membre », par les réglages de l'espace. Fenêtre 2 : colle le lien, crée le compte de Camille. Elle doit arriver dans ton espace.
- [ ] **A2. Les pages.** Fenêtre 1, dans la section « Espace » : crée une page « Essai », et écris dans son texte la ligne « Le mot ornithorynque est dans le texte. ». Crée une page « Résumé de l'été », et écris « Une élève a écrit ceci. ». Crée une page « Cible », vide. Dans la section « Privé », crée une page « Secret », et écris « Le quetzal est un oiseau. ».
- [ ] **A3. Une longue page.** Crée une page « Longue ». Écris cinq lignes de texte quelconque, puis Ctrl+A et Ctrl+D (dupliquer), cinq fois de suite : la page doit compter 160 lignes environ. Va tout en bas et écris, sur une ligne à part, « Ici vit un albatros. ».
- [ ] **A4.** Attends trois secondes : c'est le temps que l'index rattrape ce que tu viens d'écrire.

Remarques :

## B. Chercher (12 minutes)

Fenêtre 1, sur « Essai ».

- [ ] **B1. Ouvrir.** Sous le sélecteur d'espace, la barre latérale doit montrer « Rechercher », avec son raccourci écrit (Ctrl K, ou ⌘ K sur Mac). Clique-la : une fenêtre « Rechercher » s'ouvre, le curseur dans son champ (« Un titre, ou quelques mots d'une page »), et montre « Récents », tes dernières pages. Échap la ferme, et le focus revient sur « Rechercher ». Puis, le curseur dans le texte de « Essai » sans rien sélectionner, Ctrl+K : la même fenêtre s'ouvre. Échap : le curseur doit revenir **là où il était** dans le texte ; tape une lettre, elle doit s'écrire à cet endroit, pas en tête de la page. Efface-la.
- [ ] **B2. Ctrl+K avec du texte sélectionné.** Sélectionne un mot du texte de « Essai », attends que la barre de mise en forme paraisse, puis Ctrl+K : c'est la barre de lien de l'éditeur qui doit s'ouvrir, pas la recherche. Échap.
- [ ] **B3. Titres d'abord, puis le texte.** Ouvre la recherche et tape « cib » : la ligne « Cible » doit paraître aussitôt (elle vient de l'arbre déjà chargé). Efface, et tape « ornithorynque » : un instant plus tard (moins d'une demi-seconde), « Essai » doit paraître, avec un extrait où « ornithorynque » est surligné. Rien de ce qui était déjà montré ne doit bouger sous le pointeur quand une ligne s'ajoute.
- [ ] **B4. Au clavier seul.** Flèche bas, flèche haut : la ligne active change, sans que la liste saute. Entrée ouvre la page de la ligne active. Le bas de la fenêtre rappelle les touches (↑ ↓ choisir, Entrée ouvrir, Ctrl Entrée nouvel onglet, Échap fermer).
- [ ] **B5. Le dernier mot complété.** Cherche « ornith » : « Essai » doit être trouvée (le dernier mot tapé se complète). Cherche « ornith texte » : trouvée aussi (tous les mots, dans le même bloc). Cherche « ornithorynques » (un « s » de plus, à la fin) : rien (pas de racinisation, choix 6). Cherche « o » seul : la lettre seule n'est pas complétée, et « Essai » ne vient pas.
- [ ] **B6. Accents et casse.** Cherche « RESUME » : « Résumé de l'été » doit venir. Cherche « eleve » : la même page, trouvée par son texte, « élève » surligné. Cherche « été » : la même page, par son titre.
- [ ] **B7. Le résultat en tête.** Fais cinq recherches réelles, sur des pages à toi (un mot du titre, un mot du milieu d'un long texte, deux mots d'un même paragraphe). Le bon résultat est-il en tête ? Note celles où il ne l'est pas. Une page trouvée par son titre doit passer devant une page qui ne porte le mot que dans son texte.
- [ ] **B8. Les bornes.** Tape une recherche de neuf mots différents (par exemple « un deux trois quatre cinq six sept huit neuf ») : la fenêtre doit dire, en rouge, « Ta recherche a trop de mots : 8 au plus. ». Colle une longue phrase de plus de 200 caractères dans le champ : il doit s'arrêter à 200.
- [ ] **B9. Plus de vingt pages (facultatif).** Crée une page « Colibri » dont le texte porte « colibri », puis duplique-la (menu « … » de sa ligne, « Dupliquer ») jusqu'à avoir vingt-cinq pages. Cherche « colibri » : vingt lignes au plus, puis « Afficher plus de résultats » en bas, hors de la liste qui défile. Clique-le : les suivantes s'ajoutent dessous, et le bouton ne doit pas descendre sous ton pointeur. Supprime ensuite ces copies (corbeille, puis « Supprimer définitivement »), ou laisse-les.
- [ ] **B10. L'indexation en cours.** Dans « Essai », écris sur une nouvelle ligne « pangolin », puis, **tout de suite**, Ctrl+K et cherche « pangolin » : la fenêtre doit dire « Aucun résultat. » et « L'indexation est en cours : des pages peuvent manquer. ». Attends trois secondes, efface une lettre et retape-la : « Essai » doit venir, et la phrase sur l'indexation doit partir. Ces deux phrases se comprennent-elles ?
- [ ] **B11. Ce qui est cherché, et pas (facultatif).** Ajoute un commentaire « marmotte » sur « Essai » : chercher « marmotte » ne doit rien trouver (les commentaires ne sont pas cherchés, choix 3). Pose une équation `x^2 + lemniscate` (menu « / », « Équation ») : chercher « lemniscate » doit trouver la page.

Remarques :

## C. Arriver sur le bloc (8 minutes)

- [ ] **C1. Une page qu'on ouvre.** Ouvre « Essai ». Cherche « albatros », Entrée : « Longue » doit s'ouvrir **à son dernier bloc**, amené au milieu de la fenêtre, le curseur dedans ; tape « ! » : le point d'exclamation doit s'écrire dans ce bloc. Efface-le. L'adresse finit par `#b=` suivi de l'identifiant du bloc. La page ne doit pas sauter ensuite.
- [ ] **C2. Retour arrière.** Bouton « Précédent » du navigateur : tu dois revenir sur « Essai ». « Suivant » : de nouveau « Longue », à son dernier bloc.
- [ ] **C3. Une page déjà ouverte.** Sur « Longue », remonte tout en haut (Ctrl+Début). Ctrl+K : le champ est vide (ouvrir un résultat l'a vidé, choix 27). Tape « albatros », Entrée : la page doit redescendre jusqu'au bloc, sans se recharger (choix 28). Remonte, recommence une fois : même effet. Puis Ctrl+K, tape « ornith », et Échap ; Ctrl+K de nouveau : la fenêtre se rouvre sur « ornith », tout sélectionné. Échap. « Précédent » ne doit pas te faire repasser par chaque geste vers le même bloc.
- [ ] **C4. Une page trouvée par son titre.** Cherche « longue », Entrée : « Longue » s'ouvre **en haut** (sa ligne vient de l'arbre, sans bloc, choix 24).
- [ ] **C5. Nouvel onglet, repli (facultatif).** Cherche « albatros », Ctrl+Entrée : un nouvel onglet doit s'ouvrir sur « Longue », à son dernier bloc. Dans « Essai », pose un bloc repliable (menu « / », « Liste repliable »), écris « dauphin » dans un de ses sous-blocs, replie-le, attends trois secondes, puis cherche « dauphin » et ouvre le résultat : la page s'ouvre, mais le repli reste fermé (limite connue, choix 29).
- [ ] **C6. Un lecteur.** Ouvre « Partager » sur « Longue », et règle « Tous les membres de l'espace » à « Peut lire ». Fenêtre 2 : Camille cherche « albatros » et ouvre le résultat. « Longue » doit s'ouvrir à son dernier bloc, chez elle aussi, en lecture seule. Remets ensuite « Peut modifier ».

Remarques :

## D. Les rétroliens (8 minutes)

- [ ] **D1. Une mention.** Fenêtre 1, dans « Essai » : tape `@`, puis « cib », et choisis la page « Cible ». Attends trois secondes, puis ouvre « Cible » : sous son texte, une section « Pages qui mènent ici » doit nommer « Essai », avec un extrait du bloc qui porte la mention. Elle ne doit pas gêner la lecture de la page.
- [ ] **D2. Un lien vers une page.** Dans « Longue », sur une ligne vide, menu « / », « Lien vers une page », choisis « Cible ». Attends trois secondes, ouvre « Cible » : la section doit nommer « Essai » et « Longue ». Clique « Longue » dans la section : elle doit s'ouvrir **au bloc du lien**.
- [ ] **D3. Rien n'est poussé.** Fenêtre 2 : Camille ouvre « Cible », et la laisse ouverte. Fenêtre 1 : dans « Résumé de l'été », mentionne « Cible ». Attends trois secondes. Fenêtre 2 : la section de Camille ne change pas tant qu'elle reste sur l'onglet (choix 32). Clique dans une autre application, attends cinq secondes, reviens sur la fenêtre 2 : « Résumé de l'été » doit paraître dans la section.
- [ ] **D4. Un lien du texte (facultatif).** Fenêtre 1 : ouvre « Cible » et copie son adresse dans la barre du navigateur. Dans « Secret », écris « voir là », sélectionne « là », Ctrl+K (la barre de lien), colle l'adresse, Entrée. Attends trois secondes, ouvre « Cible » : « Secret » doit paraître dans la section. Ce lien ne compte que parce que le serveur connaît son adresse publique (`--public-url`, plus haut, choix 34).
- [ ] **D5. Le nom et la place.** « Pages qui mènent ici », sous le texte : est-ce le bon nom, la bonne place (choix 8) ?
- [ ] **D6. Un lien retiré.** Dans « Essai », efface la mention de « Cible ». Attends trois secondes, puis rouvre « Cible » : « Essai » ne doit plus être nommée.

Remarques :

## E. À deux : ce qu'on ne voit pas ne se trouve pas (12 minutes)

Fenêtre 2 : Camille, membre.

- [ ] **E1. Une page privée.** Fenêtre 2 : Camille cherche « quetzal » : « Aucun résultat. ». Fenêtre 1 : toi, tu trouves « Secret ».
- [ ] **E2. Un invité.** Fenêtre 1 : crée dans « Espace » une page « Pour Dom », écris « Le mot capybara est ici. », puis une sous-page « Note de Dom » où tu écris « Le mot capybara encore. » ; mentionne « Pour Dom » dans « Note de Dom » et dans « Secret ». Invite `dom@exemple.fr` sur « Pour Dom » seulement (« Partager », « Peut lire »). Fenêtre 2 : déconnecte Camille (menu du compte), colle le lien de l'invitation, crée le compte de Dom. Attends trois secondes. Dom cherche « ornithorynque » : rien. Il cherche « capybara » : « Pour Dom » et « Note de Dom », et le fil d'Ariane de « Note de Dom » ne nomme que « Pour Dom ».
- [ ] **E3. Les rétroliens d'un invité.** Dom ouvre « Pour Dom » : la section doit nommer « Note de Dom », et **pas** « Secret ». Dom cherche « Pour Dom » : il trouve « Note de Dom » (la mention en copie le titre, choix 14), mais jamais « Secret ».
- [ ] **E4. La corbeille.** Fenêtre 1 : mets « Note de Dom » à la corbeille. Fenêtre 2 : Dom cherche « capybara » : seule « Pour Dom » vient. Fenêtre 1 : restaure-la. Dom cherche de nouveau : « Note de Dom » revient aussitôt, sans attendre l'index (choix 4).
- [ ] **E5. Un accès retiré, fenêtre ouverte.** Fenêtre 2 : Dom cherche « capybara », et laisse la fenêtre de recherche ouverte sur ses deux résultats. Fenêtre 1 : dans « Partager » de « Pour Dom », retire l'accès de Dom. Fenêtre 2 : les deux lignes restent affichées (choix 25). Dom clique « Pour Dom » : la page doit dire « Tu n'as plus accès à cette page », et la fenêtre se fermer. Il rouvre la recherche et retape une lettre : plus aucun résultat. Fenêtre 1 : rends-lui l'accès.
- [ ] **E6. Deux espaces.** Fenêtre 1 : crée un espace « Pro » (sélecteur d'espace, « Créer un espace »), et dedans une page « Plan » où tu écris « Le pélican vole. ». Attends trois secondes. Cherche « pélican » dans « Pro » : trouvé. Reviens dans ton premier espace et cherche « pélican » : rien.
- [ ] **E7. Une source qui part (facultatif).** Fenêtre 2 : Dom ouvre « Pour Dom », la section nomme « Note de Dom ». Pose ton pointeur sur cette ligne et laisse-le. Fenêtre 1 : mets « Note de Dom » à la corbeille. Fenêtre 2 : quand la barre latérale a perdu « Note de Dom » (quelques secondes), la ligne de la section doit rester à sa place, sous « Page indisponible » ; sors le pointeur de la section : la ligne doit partir. Fenêtre 1 : restaure « Note de Dom ».

Remarques :

## F. Les délais (5 minutes)

Fenêtre 1, sur « Essai ».

- [ ] **F1. Un mot tapé.** Écris « wapiti », puis arrête de taper et regarde l'heure. Cherche « wapiti » toutes les secondes (efface une lettre et retape-la) : « Essai » doit venir entre deux et trois secondes après ta dernière frappe, quinze au plus.
- [ ] **F2. Une frappe continue (facultatif).** Écris sans t'arrêter pendant vingt secondes (n'importe quoi) dans « Longue », en commençant par « okapi ». Fenêtre 2 (connecte-toi en Camille, ou toi dans une autre fenêtre) : cherche « okapi » pendant que tu tapes encore : « Longue » doit venir avant que quinze secondes soient passées depuis « okapi ».
- [ ] **F3. Un mot effacé.** Efface « wapiti », puis cherche-le aussitôt : il peut être encore trouvé, et lisible dans l'extrait, pendant deux à trois secondes ; après, plus rien. C'est la limite L-a : un texte retiré d'une page juste avant d'y inviter quelqu'un reste trouvable par lui ce temps-là. Ce délai te convient-il (choix 9) ?

Remarques :

## G. L'exploitation (15 minutes)

Commandes dans le terminal du serveur (le premier). Ici, `mementum` se lance par `cargo run -p mementum-cli --` ; si `cargo` répond « Accès refusé » pendant que le serveur tourne, lance plutôt le binaire déjà compilé, `target\debug\mementum.exe` sous Windows, `target/debug/mementum` ailleurs.

- [ ] **G1. Refusée pendant que le serveur tourne.** Serveur lancé, dans un troisième terminal : `cargo run -p mementum-cli -- reindex --data-dir data/recette-lot4`. La commande doit refuser, en français : « l'index de recherche est déjà ouvert par un autre processus (un serveur qui tourne, ou une réindexation en cours) ; arrête-le d'abord, puis relance la commande : la réindexation ne tourne que serveur arrêté. Rien n'a été modifié, ni la base ni l'index ». La recherche marche toujours dans la fenêtre 1.
- [ ] **G2. Tout refaire, serveur arrêté.** Arrête le serveur (Ctrl-C). Lance `cargo run -p mementum-cli -- reindex --data-dir data/recette-lot4`. Elle doit finir en quelques secondes par une ligne « réindexation finie (sans adresse publique) : … pages relues depuis leur document et projetées, … entrées écrites dans l'index de recherche (… validations) », puis avertir : « attention : aucune adresse publique (ni --public-url, ni MEMENTUM_PUBLIC_URL) : un lien écrit avec l'adresse entière de l'instance (https://…/p/…) ne comptera pas comme rétrolien de sa page ; si le serveur a une adresse publique, relance la commande avec --public-url ». Relance-la avec `--public-url http://localhost:5173` : la ligne doit dire « (adresse publique : http://localhost:5173) », sans avertissement. Relance le serveur comme au début : la recherche trouve tout, et « Cible » nomme toujours « Secret » (étape D4).
- [ ] **G3. L'index seul.** Serveur arrêté : `cargo run -p mementum-cli -- reindex --text-only --data-dir data/recette-lot4`. La ligne doit dire « index de recherche refait depuis les tables, sans relire aucun document : … ». `--text-only` avec `--public-url` doit être refusé (l'adresse ne sert qu'à relire les documents).
- [ ] **G4. Le dossier de l'index supprimé.** Serveur arrêté, supprime le dossier `data/recette-lot4/search`. Relance le serveur : il doit démarrer aussitôt, et son journal dire « l'index de recherche n'est pas celui de cette base (base restaurée, dossier d'une autre instance, index perdu ou d'une autre forme) : il est reconstruit, en fond ». Quelques secondes plus tard, tout se retrouve ; si tu cherches tout de suite, la fenêtre peut dire que l'indexation est en cours.
- [ ] **G5. Sauvegarder (à lire).** Le README, « Index de recherche » : le dossier `search/` se refait de lui-même, et une sauvegarde porte toujours sur le dossier des données entier, serveur arrêté. Est-ce clair ?
- [ ] **G6. Une instance d'avant ce lot (facultatif, une vingtaine de minutes, surtout de compilation).** Dans un terminal, à côté du dépôt : `git worktree add ..\mementum-lot3 98e75f0` (le dernier commit du lot 3, sans le lot 4), puis, dans `..\mementum-lot3` : `corepack pnpm install`, `cargo run -p mementum-cli -- serve --data-dir data/avant-lot4` et, dans un autre terminal, `corepack pnpm -C apps/web dev` (arrête d'abord le serveur et le Vite de cette recette). Crée un compte et trois pages avec du texte. Arrête ce serveur et ce Vite. Copie `..\mementum-lot3\data\avant-lot4` dans `data\avant-lot4` du dépôt, puis relance le serveur de ce lot et Vite sur ce dossier (`--data-dir data/avant-lot4`). Le serveur doit écrire `data/avant-lot4/mementum.db.avant-0005.bak` (la copie d'avant migration) et démarrer aussitôt ; la recherche doit trouver tes trois pages après quelques secondes (en disant d'abord, peut-être trop vite pour le voir, que l'indexation est en cours). Retire ensuite l'atelier : `git worktree remove ..\mementum-lot3` (son dossier `target` pèse plusieurs gigaoctets).
- [ ] **G7. La place et la mémoire (facultatif).** Regarde la taille de `data/recette-lot4/search` à côté de `mementum.db` : pour un espace aussi petit, quelques Mo. Pour un grand espace, l'index et la projection pèsent ensemble de l'ordre de quatre fois le texte des pages (choix 18).

Remarques :

## À la fin

Arrête le serveur et Vite. Le dossier `data/recette-lot4` peut être supprimé. Envoie, avec tes remarques : les choix que tu gardes et ceux que tu veux autrement, et les numéros des cases qui ne vont pas.
