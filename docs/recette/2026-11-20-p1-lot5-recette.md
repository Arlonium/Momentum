# Recette du lot 5 : les agents

Pour le propriétaire du projet (Lenny), ou Alex. Écrit le 21 novembre 2026, quand le code du lot est sur `main` (`aad795e`), **avant la passe complète de fin de lot**, et mis à jour après elle, par la correction groupée (registre, R1664 à R1689) : la fenêtre de création d'un jeton dit maintenant le risque d'une page piégée et propose la portée la plus étroite d'abord (choix 24, étape B2), un jeton « Lecture » ne permet plus la corbeille (choix 44), et « Réglages de l'espace » prend le focus en arrivant (A1). Le [registre](../superpowers/logs/2026-11-20-p1-lot5-ledger.md) dit le reste.

Tu vas brancher ton propre Claude Code sur une instance d'essai, lui faire verser un compte rendu dans un espace « Pro », taper dans la page pendant qu'il la complète, puis retrouver ce qu'il a fait dans l'historique et dans l'activité de son jeton. Compte **environ une heure et demie** sans les étapes marquées « facultatif », **près de deux heures** avec elles, dont une bonne part à attendre les réponses de Claude Code. Il fera quelques dizaines d'appels de modèle, comptés sur ton abonnement ou ta clé.

Tout ce qu'un test automatique peut vérifier l'est déjà : les droits d'un jeton par chaque route et chaque outil, la frappe qui ne se perd pas pendant vingt écritures d'agent (dans Chromium), le protocole MCP dans ses deux révisions, la révocation entre deux appels. Cette liste porte sur ce qu'un test ne juge pas : si l'écran des jetons se comprend sans notice, si les commandes à coller marchent dans **ton** shell, ce que Claude Code fait vraiment, ce qu'on voit et ce qu'on perd quand on tape pendant qu'il écrit, si l'activité et l'historique disent ce qu'on y cherche. Chaque case dit ce que tu fais, puis ce que tu dois voir. Coche si c'est bon. Sinon, note le numéro (« D4 ne va pas ») et une phrase sur ce que tu as vu à la place, dans la ligne « Remarques : » de la section.

Fais les sections dans l'ordre : chacune part de l'état laissé par la précédente.

**Qui peut jouer quoi.** Les sections A, B, E, F (côté écran) et G peuvent être jouées par un script de navigateur, comme le coordinateur l'a fait pour le lot 4. **La section C (`claude mcp add` dans tes shells) ne peut être jouée que par toi ou Alex, sur ta propre configuration de Claude Code** : un agent n'écrit pas dans la configuration de Claude Code de quelqu'un d'autre (Ruling R1258). La section D demande un vrai Claude Code ; le coordinateur peut la jouer avec un Claude Code lancé à part (`claude -p … --mcp-config <fichier> --strict-mcp-config`, R1258), en gardant les outils de ressources de Claude Code (`ListMcpResourcesTool`, `ReadMcpResourceTool`, dans `--allowedTools`) : `--tools ""` les retire aussi, et le modèle ne lirait plus la spécification `mementum://spec/mmd` (R1514, R1673) ; mais ce que tu y juges (est-ce gênant ? acceptable ?) reste à toi.

## D'abord : les choix de produit pris sans toi

Le 20 novembre, Lenny a dit « tu es autonome », et de pousser sur `main` au fil du travail. Les dix-huit décisions du plan ont donc été **retenues par défaut** : la recommandation du plan, adaptée aux mesures là où il le fallait ; L13, L16 et L17 ont été tranchées par le coordinateur à la relecture du plan, L18 ajoutée à la relecture de la Task M1. D'autres choix ont été pris en route. Tous sont en service dans la version que tu essaies. Pour chacun : ce qui a été choisi, ce que ça fait, comment y revenir, et l'étape où tu le vois. Dis, dans ta réponse, lesquels tu gardes et lesquels tu veux autrement. Le détail (la question, les raisons, le coût) est dans le [plan](../superpowers/plans/2026-11-20-p1-lot5-agents.md), « Décisions L1 à L18 », et au registre.

**Les dix-huit décisions du plan** (retenues par défaut le 2026-11-20) :

1. **L1, l'échéance d'un jeton : 90 jours par défaut**, au choix 7 jours, 30 jours, 1 an ou aucune. Un jeton est à refaire tous les trois mois. Y revenir : une constante de l'écran (étape B2).
2. **L2, le mot de passe redemandé pour créer un jeton** : une session volée ne se fait pas un jeton durable. Ses essais ont leur propre frein (dix faux en quinze minutes ferment la création, pas la connexion, R1366), qu'un changement de mot de passe réussi remet à zéro (R1377). Un compte sans mot de passe (passkeys, lot 7) demandera une autre preuve (étape B5).
3. **L3, changer de mot de passe révoque tous les jetons du compte**, et la fenêtre le dit avant et après (« 1 jeton d'API révoqué… ») ; la commande `mementum user set-password` aussi. Claude Code est à rebrancher après. Y revenir : une ligne (étape G3).
4. **L4, la corbeille par un agent : sur une option du jeton, décochée par défaut**, avec « Modification » seulement (choix 44). La « confirmation humaine » de la conception est donnée d'avance par toi, en cochant l'option, pas à chaque geste. Depuis la relecture de la Task S5, l'option couvre aussi la sortie de la corbeille (choix 20) (étape F3).
5. **L5, un seul mode d'écriture, `direct`** : ce qu'un agent écrit est écrit tout de suite, et l'historique le garde ; pas de suggestions (phase 3). Le défaut de `mode` deviendra `suggest` avec elles : la description de l'outil le dit (section D).
6. **L6, `replace_page` refusé à un agent tant qu'une connexion qui peut écrire est ouverte sur la page** (un onglet qui peut la modifier) (`409 page_in_use` : le message lui dit de passer par les opérations ciblées). `PUT /mmd`, que seule une session appelle, ne refuse pas. Y revenir : une ligne (étape D6).
7. **L7, les médias du web refusés à un agent, sans option** : il ne verse pas de page qui montre une image, une vidéo ou un son d'une adresse web, même à sa création ; une page qu'il duplique garde ceux de l'original, qu'il lisait déjà. Un lien ou un signet passent. Y revenir : une option du jeton, une tâche (étape F2).
8. **L8, un journal d'activité par jeton**, pas le journal d'audit immuable de la conception (phase 3) : il ne garde pas le contenu, et en perd les deux dernières secondes à un arrêt brutal (étape E2).
9. **L9, les filtres de la recherche d'un agent : auteur et date de modification** ; « type » viendra avec les bases (phase 2) (étape D7, facultative).
10. **L10, un admin d'espace ne voit pas les jetons que ses membres ouvrent sur l'espace** ; il voit « par un agent » dans l'historique. Les jetons d'un espace viendront avec les agents d'espace (phase 4). Rien à voir dans cette recette.
11. **L11, ni OpenAPI, ni SDK, ni commande de contenu au lot 5** : un intégrateur lit le tableau des routes et `spec/mcp/tools.json`.
12. **L12, le pont stdio (`mementum mcp`) au lot 7** : un client qui ne parle que stdio attend.
13. **L13, deux niveaux de jeton, « Lecture » et « Modification »** ; pas « Accès complet » : à ce niveau, déplacer une page privée sous une page d'équipe la partagerait avec l'équipe sans passer par le partage. **Aucun jeton ne range une page sous une autre** : tu le fais à la main (étapes B2 et F1).
14. **L14, deux budgets neufs au §5.10 de la conception**, adoptés tels que mesurés (R1655) : la frappe d'une personne reste vue par un autre sous 150 ms au p95 pendant qu'un agent fait des opérations sur un bloc dans la même page de 10 000 blocs, au débit de son jeton (65 ms mesurés) ; l'appel d'un agent est visible dans le navigateur de qui a la page ouverte sous 150 ms au p95, jusqu'à 1 000 blocs (28 ms mesurés) (étape D4). Ils ne valent ni pour une écriture qui change toute une grande page, ni dans le navigateur au-delà de 1 000 blocs (limites LA-p et LA-q du README ; étape H1, facultative).
15. **L15, le porteur durable des props hors registre au lot 6**, avec l'import : `PUT /mmd` et `replace_page` refusent toujours une propriété inconnue.
16. **L16, la `0006` attendait la clôture du lot 4** : levée le 21 novembre par cette clôture. Rappelé pour mémoire.
17. **L17, deux révisions du protocole MCP servies, la 2026-07-28 d'abord, la 2025-11-25 pour un client plus ancien.** Ton Claude Code parle la première ; un client plus ancien, la seconde. Aucune étape de cette recette n'éprouve la 2025-11-25 : la Task X1 l'a fait, avec Claude Code 2.1.330 forcé dans cette révision (`MCP_PROTOCOL_NEGOTIATION=legacy`), et le parcours de bout en bout de la révocation la parle (registre, R1484, et « Essai avec le vrai client » de X1).
18. **L18, pas la révision 2025-06-18** : un client dont le kit précède la 2025-11-25 se déconnecte. À revoir si un client réel en a besoin.

**Pris en route, pendant l'exécution et les relectures** (une ligne chacun ; retenus par défaut, tu peux y revenir) :

19. **Un bloc qui porte un fil de commentaires n'est retiré par un agent qu'avec l'option `allow_deleting_content`** (R1571), comme un lien de sous-page ou un bloc d'une version plus récente : un agent n'efface pas une discussion sans le savoir.
20. **Sans l'option corbeille, un jeton ne sort pas non plus une page de la corbeille** (R1345 récrit) : deux gestes sous confirmation humaine dans la conception (étape F3).
21. **Un appel d'un jeton à une route qui lui est fermée (`403 token_not_allowed`) ne laisse aucune trace dans son activité** (R1361), comme un jeton inconnu. **À confirmer par toi** : sans trace, un jeton volé qui sonde les routes fermées ne se voit pas dans l'activité ; avec une trace, chaque sonde écrirait une ligne, que rien ne borne avant les plafonds par adresse du lot 7.
22. **Vite relaie `/mcp`** (R1447) : la commande prête à coller porte l'adresse sous laquelle tu as ouvert l'application, et mène au serveur en développement comme derrière un mandataire ; l'écran ne parle pas du port du serveur (étape C1).
23. **Les défauts de la fenêtre de création** (R1450) : « Lecture », la portée de l'espace affiché, 90 jours, la corbeille décochée ; une note dit ce que « Modification » permet (créer à la racine d'un espace, dans « Privé » par défaut ; ne jamais ranger une page sous une autre, ni supprimer pour de bon, ni partager) (étape B2).
24. **L'écran dit le risque d'une page piégée, et propose la portée la plus étroite d'abord** (R1500, corrigé à la fin du lot par R1664 ; limite LA-c) : un agent trompé par le contenu d'une page peut recopier ce que son jeton lit dans une page que son jeton écrit, et la portée est la seule barrière. Comme le plan le voulait, un avertissement le dit avant le niveau et la portée, et les portées viennent dans l'ordre « Des pages », « Un espace », « Tous mes espaces » ; l'espace affiché reste coché d'abord, le plan ne disant pas d'autre défaut. **Retenu par défaut, tu peux y revenir** : l'ancien ordre sans l'avertissement, ou « Des pages » cochée d'abord (étape B2).
25. **Le jeton montré, rien ne fait partir la fenêtre sans demander** (R1451, R1460, R1461) : Échap et Précédent demandent « Fermer la fenêtre du jeton ? » (le focus sur « Rester ») ; recharger ou fermer l'onglet montre la question générique du navigateur (quitter la page ?), qui ne peut pas dire autre chose ; le bouton « Fermer », un geste explicite, ferme sans demander (étape B5).
26. **Fermée sans créer, la fenêtre garde ce qui a été choisi, jamais le mot de passe** (R1452), le temps que tu restes sur l'écran (étape B4).
27. **Les gestes dans l'ordre du presse-papiers** (R1462) : copier et lancer la ligne qui attend le jeton, copier le jeton et le coller, puis la ligne qui branche Mementum ; une phrase dit de garder le jeton dans un gestionnaire de mots de passe et de le recoller une fois par shell (étape B6).
28. **La commande n'a pas de `--scope`** (R1464) : Claude Code ne branche alors Mementum que dans le dossier où tu la lances ; une phrase dit qu'avec `--scope user` il le branche partout. **À trancher avec toi** : faut-il `--scope user` dans la commande ? (étape C2).
29. **La liste des jetons ne se relit pas d'elle-même** (R1453) : la « dernière utilisation » d'un jeton qu'un agent emploie pendant que l'écran est ouvert date jusqu'à ton prochain geste sur l'écran (étape E1).
30. **L'activité d'un jeton se déplie sous sa ligne** (R1465, R1477), plusieurs à la fois, lue à ce moment-là seulement ; rien ne la relit d'elle-même ; son cadre garde sa hauteur, si bien que rien ne bouge sous le pointeur quand elle arrive (étape E2).
31. **Le filtre de l'activité est par issue exacte** (R1467) : « Tous les appels », « Réussis », « Refusés », « En erreur » ; voir refus et erreurs ensemble demande « Tous » (étape E3).
32. **Une ligne d'activité montre la date, l'issue, le code, l'outil, la page et les blocs touchés**, ni les révisions ni par où l'appel est venu (R1469) ; une page que tu ne vois plus y est « une page que tu ne vois pas », jamais son identifiant (R1625) (étape E2).
33. **Dans l'historique, « Alice (par un agent) »**, et non « Alice, par un agent » (R1471) : la mention dit que le compte a écrit au moins une fois par un jeton depuis la version d'avant ; le nom du jeton n'est jamais dans l'historique (étape E4).
34. **Une page née avec son contenu et jamais versionnée compte comme écrite par d'autres** (R1601) : le premier remplacement qu'un agent y fait coupe d'abord une version (étape E4).
35. **Les écritures des jetons d'un compte ont leur voie dans la file d'écriture**, distincte de celle de ta session (R1640) : un agent en boucle ne fait pas attendre tes renommages ; la révocation passe derrière ce que ses jetons y ont déjà mis (R1658).
36. **L'acteur d'une page qu'aucun onglet n'a ouverte s'arrête trois secondes après le dernier appel d'un agent**, au lieu de trente (R1657 ; trente quand sa tranche porte une écriture qui n'est pas d'un jeton, que sa version de fin doit garder) : un agent qui lit vite de grandes pages ne garde plus soixante pages en mémoire ; deux de ses appels espacés de plus de trois secondes rechargent la page (une centaine de millisecondes pour 10 000 blocs).
37. **Le débit des agents adopté tel que le plan le posait** (R1241, R1644) : 120 appels et 16 Mio par minute pour un jeton, 240 et 32 Mio pour tous les jetons d'un compte (réglables, « Brancher un agent » au README). Une connexion de Claude Code en coûte quatre avant son premier outil.
38. **Sur `/mcp`, un petit appel au-delà du débit reçoit un refus que le modèle lit** (`token_busy`, avec le délai), **un gros** (plus de 64 Kio, ou sans longueur annoncée) **le `429` HTTP sans être lu** (R1421, R1434) ; Claude Code passe aussi ce corps au modèle (R1485).
39. **`workspace_info` donne l'identifiant du compte et des membres d'un espace, jamais leur adresse e-mail** (R1435), pour que l'agent filtre une recherche par auteur.
40. **Un jeton limité à des pages ne duplique pas une de ses pages de tête** (R1351) : la copie irait à la racine de l'espace, qui demande d'être membre ; il duplique une sous-page.
41. **Un jeton dont la création est coupée juste après l'écriture reste vivant, sans que personne en ait le texte** (R1372) : il est dans la liste, à révoquer ; l'écran le dit si la réponse se perd.
42. **Le changement de mot de passe répond `200` avec le nombre de jetons révoqués**, au lieu de `204` (R1365) : rien ne change pour toi ; un outil qui attendait `204` change.
43. **Le bloc trouvé par `search` est dit par un alias de treize caractères** (`b` suivi de douze), plus long que celui de `fetch` pour le même bloc (R1426) : le serveur ne relit pas vingt pages par recherche pour calculer l'alias le plus court ; les deux se résolvent pareil.
44. **Un jeton « Lecture » ne permet pas la corbeille** (I4 de la relecture finale, R1665) : mettre à la corbeille et en sortir demandent de modifier ; sous « Lecture », la case « Permettre la corbeille » est grisée et décochée, et revenir à « Modification » la laisse décochée (étapes B2 et B4). Le serveur refuse aussi un tel jeton (`422`).

## Avant de commencer

**Ce qu'il te faut.**

- Un navigateur récent : Chrome, Edge ou Firefox, et une seconde fenêtre en navigation privée pour Camille.
- **Claude Code**, installé et connecté (`claude --version` : 2.1.329 et 2.1.330 ont été essayés ; une version plus récente devrait aller).
- Un shell que tu utilises d'ordinaire pour lancer Claude Code (PowerShell, cmd ou Git Bash sous Windows ; bash ou zsh ailleurs).
- Un dossier vide d'où brancher et lancer Claude Code, **hors du dépôt** : `recette-lot5-claude` dans ton dossier personnel (`~/recette-lot5-claude` sous bash, zsh et PowerShell, `%USERPROFILE%\recette-lot5-claude` sous cmd). Lancé dans le dépôt, Claude Code chargerait le `CLAUDE.md` du projet (des centaines de kilo-octets de consignes pour les agents qui le développent) et pourrait lire ses fichiers : il ne se conduirait pas comme chez quelqu'un qui verse ses notes. Sans `--scope`, Claude Code ne branche Mementum que dans ce dossier.
- Une adresse e-mail inventée, `camille@exemple.fr`. Aucun e-mail n'est envoyé.

**Une instance neuve, pas tes données.** La migration de ce lot (`0006`) peut encore changer jusqu'à la clôture du lot : une base ouverte par cette version pourrait refuser de démarrer avec la suivante. Fais donc cette recette sur un dossier de données à part, **jamais sur `data/instance`**. À la racine du dépôt, dans deux terminaux :

```
corepack pnpm install
cargo run -p mementum-cli -- serve --data-dir data/recette-lot5 --public-url http://localhost:5173 --version-every-secs 60
```

```
corepack pnpm -C apps/web dev
```

Puis ouvre `http://localhost:5173`. `--version-every-secs 60` coupe une version d'une page par minute d'écriture au lieu de dix : l'historique en aura pendant la recette. `--public-url` dit au serveur l'adresse sous laquelle tu l'ouvres : les pages qu'un agent crée lui sont nommées par leur adresse entière. Si `cargo` répond « Accès refusé », un autre serveur Mementum tourne déjà depuis ce dossier : arrête-le d'abord.

Si le port 5173 ou 8080 est pris, choisis-en d'autres, et donne la même adresse à `--public-url` ; la commande prête à coller suivra d'elle-même l'adresse de la page. Sous PowerShell :

```
cargo run -p mementum-cli -- serve --data-dir data/recette-lot5 --bind 127.0.0.1:8081 --public-url http://localhost:5174 --version-every-secs 60
```

```
$env:MEMENTUM_E2E_WEB_PORT = "5174"; $env:MEMENTUM_E2E_API_PORT = "8081"; corepack pnpm -C apps/web dev
```

Puis ouvre `http://localhost:5174`.

## Ce que ce lot contient, et ne contient pas

**Dedans :**

- l'écran « Jetons d'API » du compte : créer, copier, brancher Claude Code, révoquer, lire l'activité d'un jeton ;
- le serveur MCP intégré (`/mcp`, `/mcp/readonly`), ses six outils (`search`, `fetch`, `create_pages`, `update_page`, `manage_pages`, `workspace_info`) et la spécification du format (`mementum://spec/mmd`) ;
- les opérations d'édition des agents, qui fusionnent avec ce que tu tapes ;
- l'écriture par différence, aussi pour `PUT /mmd` et la restauration d'une version ;
- « par un agent » et « avant une modification par un agent » dans l'historique.

**Pas encore** (ne le signale pas comme un défaut) : le mode suggestion ; un badge ou une présence « l'agent écrit… » ; l'annulation de tout ce qu'un agent a fait d'un coup ; OAuth ; le pont stdio ; l'envoi de fichiers ou d'images du web par un agent ; les agents d'espace et les jetons d'un espace ; un admin qui voit les jetons des membres ; une alerte par e-mail ; l'OpenAPI ; le filtre « type » ; `[[Page#Titre]]`.

## A. Préparer l'espace (8 minutes)

- [ ] **A1. Les comptes et l'espace « Pro ».** Fenêtre 1 : crée ton compte, le premier de l'instance. Par le sélecteur d'espace, « Créer un espace », nomme-le « Pro » : tu dois y arriver. Dans « Pro », invite `camille@exemple.fr` comme « Membre » (« Réglages de l'espace », dans le même menu : ouverts au clavier, le focus arrive sur leur titre). Fenêtre 2 : colle le lien, crée le compte de Camille ; elle doit arriver dans « Pro ».
- [ ] **A2. Les pages.** Fenêtre 1, dans « Pro », section « Espace » : crée une page « Projet », et écris trois paragraphes : « Premier paragraphe. », « Deuxième paragraphe. », « Troisième paragraphe. ». Sous « Projet », crée une sous-page « Détails », avec la ligne « Un paragraphe de détails. ». À côté de « Projet », crée une page « Autre », avec « Rien à voir. ». Puis, dans ton premier espace (« Espace de » suivi de ton nom), une page « Perso » avec « Le mot zorquartz est ici. ».

Remarques :

## B. Créer un jeton (12 minutes)

Fenêtre 1, sur « Pro ».

- [ ] **B1. L'écran.** Ouvre le sélecteur d'espace : sous ton nom et ton adresse, « Jetons d'API », au-dessus de « Changer le mot de passe ». Clique : une page « Jetons d'API » s'ouvre, le focus sur son titre, avec « Un jeton laisse un agent (Claude Code, un script) lire ou modifier tes pages en ton nom, dans la portée que tu choisis, sans jamais faire plus que toi. Tu le révoques quand tu veux ; changer ton mot de passe les révoque tous. », le bouton « Créer un jeton », et, sous « Tes jetons », « Aucun jeton pour l'instant. ». Se comprend-il ?
- [ ] **B2. La fenêtre de création, sans rien toucher.** « Créer un jeton » : une fenêtre « Créer un jeton » s'ouvre, le curseur dans « Nom du jeton ». Tu dois y voir : sous le nom, l'avertissement qui commence par « Le texte d'une page peut tromper un agent qui la lit… » et finit par « choisis la portée la plus étroite qui suffit, et « Lecture » s'il n'a rien à écrire. » ; « Niveau », « Lecture » choisi (« Il lit les pages de sa portée, et n'écrit rien. ») et « Modification » (« Il les lit, les modifie, et crée des pages. ») ; « Portée », dans l'ordre « Des pages de « Pro », avec leurs sous-pages », « Un espace » (choisi, avec « Pro » dans sa liste), puis « Tous mes espaces » ; la note qui commence par « Avec « Modification », un agent modifie les pages de sa portée… » ; « Permettre la corbeille », décochée et grisée tant que « Lecture » est choisi (choix 44) ; « Échéance » à « 90 jours » ; « Mot de passe actuel ». Sans notice, comprends-tu ce que l'agent pourra faire, et ce qu'il ne fera jamais ? L'avertissement et l'ordre des portées te vont-ils (choix 24) ?
- [ ] **B3. Choisir des pages.** Choisis « Des pages de « Pro »… » : une recherche (« Chercher une page ») et les pages de « Pro » paraissent. Coche « Projet », puis décoche-la : elle reste à sa place. Tape « zzz » : « Aucune page de l'arbre ne porte ce titre. ». Entrée dans ce champ n'envoie rien. Reviens à « Un espace », « Pro ».
- [ ] **B4. Fermer sans créer.** Écris le nom « Claude Code recette », choisis « Modification » : « Permettre la corbeille » n'est plus grisée (laisse-la décochée). Échap : la fenêtre se ferme. Rouvre-la par « Créer un jeton » : le nom, « Modification » et « Un espace » sont toujours là, le mot de passe vide (choix 26).
- [ ] **B5. Un mot de passe faux, puis le bon.** Tape un mot de passe faux, « Créer le jeton » : la fenêtre reste ouverte et dit « le mot de passe actuel est incorrect », le curseur dans le mot de passe. Tape le bon, « Créer le jeton » : la fenêtre devient « Jeton créé », avec « Ce jeton ne sera plus affiché. » et « Garde-le dans un gestionnaire de mots de passe : il faudra le recoller une fois par shell, avant d'y lancer Claude Code. ». Le focus est sur « Copier la ligne 1 ». Appuie sur Échap : « Fermer la fenêtre du jeton ? », le focus sur « Rester » ; « Rester ». Fais « Précédent » dans le navigateur : la même question ; « Rester ». **Ne ferme pas la fenêtre : la section C s'en sert.**
- [ ] **B6. Les commandes.** Sous « Brancher Claude Code », « Ton shell » propose celui de ton système (PowerShell sous Windows, « bash ou zsh » ailleurs), et trois étapes : la ligne qui attend le jeton (« Copier la ligne 1 »), le jeton (« Copier le jeton »), puis la ligne qui branche Mementum (« Copier la ligne 2 »), qui commence par `claude mcp add --transport http mementum` et finit par `Authorization: Bearer ${MEMENTUM_TOKEN}` : **aucune ligne ne porte le jeton**. Passe d'un shell à l'autre : sous bash et PowerShell, l'adresse et l'en-tête sont entre apostrophes ; sous cmd, entre guillemets, et l'écran dit que le jeton reste affiché dans la fenêtre. Sous PowerShell, une seconde ligne 1 pour Windows PowerShell 5.1. Sous la ligne 2, la phrase sur `--scope`. Ces trois étapes, et les deux phrases du bas (le presse-papiers, cmd), se comprennent-elles sans notice ? Reviens à ton shell.

Remarques :

## C. Brancher ton Claude Code (15 minutes, toi ou Alex seulement)

C'est la vérification que le plan laissait à Alex (Rulings R1464, R1488, R1576) : ce que `claude mcp add` garde de l'en-tête, et si Claude Code y développe la variable à chaque lancement. Rien ici ne doit montrer le jeton à l'écran (sauf sous cmd) ni l'écrire dans ta configuration.

- [ ] **C1. Brancher.** Ouvre un terminal de ton shell, et va dans le dossier de la recette, hors du dépôt (sous bash, zsh ou PowerShell : `mkdir ~/recette-lot5-claude`, puis `cd ~/recette-lot5-claude` ; sous cmd : `mkdir %USERPROFILE%\recette-lot5-claude`, puis `cd %USERPROFILE%\recette-lot5-claude`). Fenêtre 1 : « Copier la ligne 1 », colle-la dans le shell, Entrée : le shell attend, avec l'invite « Jeton ». Fenêtre 1 : « Copier le jeton » ; colle-le dans le shell, Entrée : sous bash et zsh, rien ne s'affiche ; sous PowerShell, des astérisques ; sous cmd, le jeton en clair. Fenêtre 1 : « Copier la ligne 2 » ; colle-la, Entrée : Claude Code dit qu'il a ajouté le serveur `mementum`. Ferme maintenant la fenêtre du jeton par « Fermer » : la liste montre « Claude Code recette », « Modification · l'espace « Pro » ».
- [ ] **C2. Ce que la configuration garde.** Dans le même shell : `claude mcp get mementum`. Note **exactement** ce que dit la ligne de l'en-tête : `Authorization: Bearer ${MEMENTUM_TOKEN}` (ce qu'on attend), ou le jeton en clair (`mmt_…` : ne le recopie pas dans ta réponse, dis seulement « en clair »), ou `Bearer ` sans rien. **À renvoyer.**
- [ ] **C3. Le premier appel.** Toujours dans ce shell et ce dossier : `claude`. Tape `/mcp` : `mementum` doit y être, connecté. Demande : « Dans Mementum, qui suis-je, et quels espaces mon jeton ouvre-t-il ? ». Claude Code appelle `workspace_info` (il peut demander la permission d'utiliser l'outil : accepte). Il doit dire ton nom, que le jeton est « Modification », limité à « Pro », avec son échéance, et ne nommer **que « Pro »**, jamais ton premier espace. Quitte Claude Code (`/exit`).
- [ ] **C4. Un shell neuf.** Ouvre un **nouveau** terminal du même shell, va dans le même dossier, et lance `claude` **sans** poser la variable. `/mcp` : `mementum` ne doit pas se connecter. Selon la version de Claude Code, soit il envoie l'en-tête tel quel et le serveur le refuse, soit il ne peut pas lire la configuration d'un serveur dont la variable n'est pas posée, le dit, et ne joint jamais le serveur : dans les deux cas, « pas connecté » est le résultat attendu ; note ce qu'il dit. Quitte. Dans ce même terminal, lance la ligne 1 et colle le jeton (tu l'as gardé : sinon, crée un jeton neuf et refais C1), puis `claude` : `/mcp` doit montrer `mementum` connecté. C'est la preuve que Claude Code développe la variable à chaque lancement. **À renvoyer** : connecté sans la variable (ce serait un défaut), pas connecté sans elle, connecté avec elle.
- [ ] **C5. Si C2 ou C4 ne va pas (facultatif).** Si la configuration garde le jeton en clair, ou si C4 ne se connecte pas avec la variable posée : `claude mcp remove mementum`, puis la ligne 2 avec `--scope project` juste après `claude mcp add` ; le dossier gagne un fichier `.mcp.json`, qui doit porter `${MEMENTUM_TOKEN}` et pas le jeton ; refais C4. Dis ce que tu as vu : l'écran passera alors à `--scope project`.
- [ ] **C6. Tes autres shells (facultatif, sous Windows).** Refais C1 et C2 dans chacun de ceux dont tu te sers (Windows PowerShell 5.1, avec la ligne 1 qui lui est propre ; cmd ; Git Bash avec la commande « bash ou zsh »), après `claude mcp remove mementum` à chaque fois. Note, shell par shell, ce que garde l'en-tête, et si la ligne 1 de Windows PowerShell 5.1 attend bien le jeton sans l'afficher.

Remarques :

## D. Le cas d'Alex (20 minutes)

Claude Code lancé dans le shell de C4, la variable posée. Fenêtre 1 sur « Pro ».

- [ ] **D1. Verser un compte rendu.** Colle à Claude Code : « Verse ce compte rendu dans mon espace Pro, dans une page « Réunion du 21 novembre » : Présents : Alex, Camille. Décisions : livrer le lot 5 ; écrire la recette. Actions : Camille relit le README. ». Il doit appeler `create_pages` (parfois après `workspace_info` et la lecture de la spécification). Fenêtre 1, **sans recharger**, la page « Réunion du 21 novembre » doit paraître dans la section **« Privé »** de « Pro » en quelques secondes au plus. Ouvre-la : le contenu est-il mis en forme comme tu l'attends (titres, liste) ?
- [ ] **D2. Ce qu'il dit faire.** Claude Code te donne-t-il l'adresse de la page (`http://localhost:5173/p/…`) ? Clique-la : elle ouvre la page.
- [ ] **D3. Taper pendant qu'il complète.** Fenêtre 1, dans « Réunion du 21 novembre », clique à la fin du titre ou de la première ligne, prêt à taper. Demande à Claude Code : « Complète la page « Réunion du 21 novembre » : ajoute à la fin une section « Prochaines étapes » avec trois points, puis remplace « Camille relit le README » par « Camille relit le README et la recette ». ». **Pendant qu'il travaille**, tape sans t'arrêter, dans la première ligne, une phrase d'une quinzaine de mots. Tu dois voir ses ajouts arriver dans l'éditeur sans recharger, et **ta phrase entière et dans l'ordre**. Est-ce gênant ? Voit-on ce qu'il fait ? (Rien ne dit « l'agent écrit » : phase 4.)
- [ ] **D4. La vitesse.** Quand Claude Code écrit, le texte paraît-il dans ta fenêtre aussitôt que l'outil a répondu (le budget est de 150 ms) ?
- [ ] **D5. Remplacer ce que tu as écrit.** Dans la page, écris un paragraphe neuf : « Je tape ici une idée importante. », puis **arrête de taper**. Demande à Claude Code : « Dans la page Réunion, utilise replace_block pour remplacer le paragraphe qui commence par « Je tape ici » par « Paragraphe récrit par l'agent. ». ». Son paragraphe doit paraître à la place du tien. Comme ton paragraphe n'était gardé par aucune version, le serveur en a d'abord coupé une, « avant une modification par un agent » (étape E4) : l'appel de Claude Code a pris une fraction de seconde de plus.
- [ ] **D5 bis. Taper dans le bloc qu'il remplace (facultatif).** Écris un autre paragraphe, « Une seconde idée, », laisse le curseur au bout, demande à Claude Code de le remplacer de même par « Second paragraphe de l'agent. », et continue ta phrase pendant qu'il travaille. Tu peux voir : (a) un refus (`revision_conflict` : le bloc a changé depuis sa lecture), que Claude Code dit, avant de relire et recommencer ; ou (b) son paragraphe écrit, et ce que tu as tapé après son écriture gardé au bord de son texte (« Second paragraphe de l'agent. et la suite »). Ce que la règle garde et perd est au README (limite LA-a, R1629) : une frappe dans le texte d'un paragraphe remplacé par un paragraphe est gardée ; remplacé par un titre seul, ou dans un sous-bloc, elle est perdue. Ce qui est perdu n'est gardé par une version que si quelqu'un d'autre que l'agent l'avait écrit avant la version qu'il coupe. Est-ce acceptable ?
- [ ] **D6. Récrire toute la page pendant qu'elle est ouverte.** La page ouverte dans la fenêtre 1, demande : « Utilise replace_page pour récrire toute la page Réunion, avec ses sections dans l'ordre inverse. ». Il doit recevoir un refus (« quelqu'un écrit dans cette page en ce moment… »), te le dire, et proposer, ou faire, des opérations ciblées. Puis, fenêtre 1, va sur une autre page, et redemande : cette fois, la récriture passe (choix 6).
- [ ] **D7. Chercher (facultatif).** Demande : « Cherche « zorquartz » dans mes espaces Mementum. » : il ne doit rien trouver (« Perso » est hors de la portée du jeton) ; il peut dire qu'il ne voit que « Pro ». Puis : « Dans Pro, cherche « paragraphe » parmi les pages modifiées depuis hier. » : il doit trouver « Projet ».
- [ ] **D8. Une page d'équipe.** Demande : « Dans la page Projet de Pro, remplace « Deuxième paragraphe. » par « Deuxième paragraphe, revu par l'agent. » ». Fenêtre 1 : « Projet » montre le paragraphe changé, sans recharger. Attends une minute (l'intervalle des versions), puis écris un mot dans « Projet » : une version vient d'être coupée.

Remarques :

## E. L'activité et l'historique (12 minutes)

- [ ] **E1. La liste.** Fenêtre 1, menu du compte, « Jetons d'API ». La ligne de ton jeton dit « Modification · l'espace « Pro » », puis « se termine par … » suivi de quatre caractères, « créé le … · expire le … » et la dernière utilisation. Si l'écran était déjà ouvert, la dernière utilisation peut dater de ton dernier geste sur l'écran (choix 29) : recharge la page pour la voir à jour.
- [ ] **E2. L'activité.** « Activité » sur la ligne du jeton : elle se déplie sous la ligne, avec le filtre « Tous les appels », « Réussis », « Refusés », « En erreur », et la liste des appels, le plus récent en haut. Chaque ligne : la date à la seconde, « Réussi » ou « Refusé », l'outil (`workspace_info`, `create_pages`, `fetch`, `update_page`, `search`, et « POST /mcp » pour les échanges du protocole, quatre par connexion de Claude Code), la page par son titre, et les blocs touchés pour une écriture. Retrouve le refus de D6 : « Refusé », le code `page_in_use`, la page « Réunion du 21 novembre ». Y trouves-tu ce que tu cherches ? « POST /mcp » se comprend-il ?
- [ ] **E3. Le filtre.** Choisis « Refusés » : seuls les refus restent. Reviens à « Tous les appels ». Clique de nouveau « Activité » : elle se replie, et le focus reste sur le bouton.
- [ ] **E4. L'historique de la page de l'agent.** Ouvre « Réunion du 21 novembre », menu « … », « Historique ». Tu dois voir plusieurs versions, dont une marquée **« avant une modification par un agent »**, coupée en D5. Une telle version n'est coupée qu'avant un `replace_page`, un `replace_block` ou un `delete_block` d'un agent, et seulement si ce qu'il remplace n'est gardé par aucune version et que quelqu'un d'autre que ce jeton l'a écrit (toi, ici) ; un `str_replace` n'en coupe jamais. Tu dois voir aussi des auteurs suivis de **« (par un agent) »** là où l'agent a écrit ; nulle part le nom « Claude Code recette ». Choisis la version « avant une modification par un agent » : son aperçu montre ton paragraphe tel qu'il était avant. « Restaurer », puis confirme : la page retrouve ton paragraphe (comme toute restauration, ce qui a été écrit depuis cette version est remplacé). Ces deux libellés se comprennent-ils ?
- [ ] **E5. Ce que voit Camille.** Fenêtre 2 (Camille), ouvre « Projet », menu « … », « Historique » : la version coupée en D8 nomme ton compte suivi de « (par un agent) », sans le nom du jeton. Camille ne voit pas « Réunion du 21 novembre », qui est dans ton « Privé ».

Remarques :

## F. Ce qu'un agent ne fait pas (10 minutes)

Dans Claude Code, une demande par case. Ce qu'il dit importe autant que ce qu'il ne fait pas : le message du serveur lui dit-il quoi faire ?

- [ ] **F1. Ranger sous une autre page.** « Range la page Réunion du 21 novembre sous la page Projet. » : il ne doit pas y arriver (il ne déplace une page que parmi ses pages sœurs), et le dire. La barre latérale ne bouge pas.
- [ ] **F2. Une image du web.** « Ajoute à la fin de la page Réunion l'image https://example.com/logo.png. » : refusé (`external_media`, une image d'une adresse web) ; il le dit, et peut proposer un lien à la place, qui passe.
- [ ] **F3. La corbeille.** « Mets la page Autre à la corbeille. » : refusé, avec « ce jeton d'API ne met pas de page à la corbeille et ne l'en sort pas : crée un jeton avec l'option « corbeille » ». « Autre » reste dans la barre latérale.
- [ ] **F4. Partager.** « Partage la page Projet avec camille@exemple.fr. » : il n'a aucun outil pour cela, et doit le dire.
- [ ] **F5. Un jeton de lecture limité à une page (facultatif).** Fenêtre 1, crée un second jeton : « Lecture », portée « Des pages de « Pro » », coche « Projet ». Dans un nouveau terminal, dans le dossier de la recette, lance la ligne 1 et colle **ce** jeton, puis `claude` : le serveur `mementum` sert maintenant ce jeton-là. Demande « Cherche « paragraphe » dans Pro » : seuls « Projet » et « Détails » viennent ; « Lis la page Autre » : introuvable, comme une page qui n'existe pas ; « Ajoute une ligne à Projet » : refusé (le jeton ne fait que lire). Révoque ce second jeton à la fin.

Remarques :

## G. Révoquer, changer de mot de passe (8 minutes)

- [ ] **G1. Révoquer pendant qu'il travaille.** Claude Code ouvert avec le premier jeton (shell de C4). Fenêtre 1, « Jetons d'API », « Révoquer » sur « Claude Code recette » : une confirmation « Révoquer le jeton « Claude Code recette » ? » dit que les agents qui s'en servent n'auront plus accès à rien dès leur prochain appel, et qu'une révocation ne se défait pas. « Révoquer » : la ligne dit « Révoqué », à la même place, et le focus va à son nom. Demande à Claude Code : « Relis la page Réunion du 21 novembre. » : il doit dire que le serveur a refusé l'en-tête `Authorization` de sa configuration, qu'il faut la mettre à jour, puis `/mcp` pour se reconnecter (c'est le message de Claude Code, pas celui du serveur : Ruling R1252). Se comprend-il ?
- [ ] **G2. Un jeton mort se lit encore.** « Activité » sur le jeton révoqué : son activité est toujours là, les refus de la section F compris. L'appel refusé en G1 n'y est pas : un jeton révoqué ne laisse pas de trace, comme un jeton inconnu (`401` avant tout journal, R1348).
- [ ] **G3. Le mot de passe.** Crée un jeton neuf quelconque. Menu du compte, « Changer le mot de passe » : la fenêtre dit, avant que tu valides, « Tes jetons d'API seront tous révoqués : chaque agent qui s'en sert en demandera un neuf. ». Change-le : elle doit dire « 1 jeton d'API révoqué : l'agent qui s'en servait en demandera un neuf. ». Retourne sur « Jetons d'API » : le jeton neuf dit « Révoqué ».

Remarques :

## H. Les limites à voir (facultatif)

- [ ] **H1. Une très grande page (facultatif, 10 minutes).** Le changement de mot de passe (G3) a révoqué tes jetons : crée un nouveau jeton « Modification » limité à « Pro », pose-le dans un nouveau terminal par la ligne 1, et lance `claude` dans le dossier de la recette (le branchement reste ; seule la variable change). Crée une page « Grande » : cinq lignes de texte, puis, sept fois de suite, Ctrl+A **deux fois** puis Ctrl+D (dupliquer) : la page compte 640 lignes. Ctrl+D ne copie pas plus de 1 000 blocs d'un geste (il le dit) : pour aller au-delà, Ctrl+A deux fois, Ctrl+C, clique à la fin de la dernière ligne, puis Ctrl+V quinze fois : la page compte environ 10 000 lignes (le collage d'autant de blocs n'a pas été essayé : s'il est refusé ou fige l'onglet longtemps, note-le, et arrête-toi là). Garde-la ouverte, et demande à Claude Code d'y remplacer un mot d'une ligne. Pendant qu'il écrit, l'onglet se fige environ trois quarts de seconde à chaque écriture reçue (limite LA-q, défaut d'avant ce lot, qu'une tâche dédiée est proposée pour lever). Est-ce gênant au point de bloquer le lot ?
- [ ] **H2. Une instance du lot 4 (facultatif, une vingtaine de minutes, surtout de compilation).** Arrête le serveur et le Vite de cette recette. À côté du dépôt : `git worktree add ..\mementum-lot4 5d10757` sous PowerShell ou cmd, `git worktree add ../mementum-lot4 5d10757` sous Git Bash, qui avale la barre `\` (la clôture du lot 4, sans la migration `0006`), puis, dans `..\mementum-lot4` (`../mementum-lot4`) : `corepack pnpm install`, `cargo run -p mementum-cli -- serve --data-dir data/avant-lot5` et `corepack pnpm -C apps/web dev`. Crée un compte, trois pages avec du texte, et attends une minute en écrivant. Arrête ce serveur et ce Vite. Copie `..\mementum-lot4\data\avant-lot5` dans `data\avant-lot5` du dépôt, et relance le serveur de ce lot sur ce dossier (`--data-dir data/avant-lot5`) : il doit écrire `data/avant-lot5/mementum.db.avant-0006.bak` et démarrer ; tes trois pages et leur historique sont là. Retire ensuite l'atelier : `git worktree remove ..\mementum-lot4` (`../mementum-lot4` sous Git Bash ; son dossier `target` pèse plusieurs gigaoctets).

Remarques :

## À la fin

- Débranche Claude Code : dans le dossier de la recette, `claude mcp remove mementum` (et supprime `.mcp.json` si tu as essayé `--scope project`). Ferme les terminaux où tu as posé `MEMENTUM_TOKEN` (sous cmd, le jeton y reste affiché). Si ton presse-papiers garde un historique (Win+V sous Windows), effaces-en le jeton.
- Arrête le serveur et Vite. Les dossiers `data/recette-lot5`, `recette-lot5-claude` (dans ton dossier personnel) et, s'il existe, `data/avant-lot5` peuvent être supprimés.
- Envoie, avec tes remarques :
  - les choix que tu veux autrement (numéros 1 à 44), et ta réponse aux choix 21 (une trace des sondes de routes fermées ?), 24 (l'avertissement et l'ordre des portées, retenus comme le plan le voulait : les gardes-tu ?) et 28 (`--scope user` dans la commande ?) ;
  - les numéros des cases qui ne vont pas ;
  - **pour C2, C4 et C6, shell par shell** : ce que la configuration garde de l'en-tête (`${MEMENTUM_TOKEN}`, en clair, ou vide), si `mementum` se connecte sans la variable et avec elle, et ce qu'a donné la ligne 1 de Windows PowerShell 5.1. Ne recopie jamais un jeton dans ta réponse.
