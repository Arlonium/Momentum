# Recette du lot 3, partie B : blocs riches et organisation

Pour le propriétaire de l'instance (Alex), ou Lenny. Écrit le 17 novembre 2026.

Tu vas essayer l'application toi-même, ce document sous les yeux. Compte une heure et demie environ, une heure si tu sautes les étapes marquées « facultatif ».

Tout ce qu'un test automatique peut vérifier l'est déjà. Cette liste porte sur ce qu'un test ne juge pas : ce que tu vois quand vous êtes deux, le confort au clavier et à la souris, le dessin, les mots. Chaque case dit ce que tu fais, puis ce que tu dois voir. Coche si c'est bon. Sinon, note le numéro (« C4 ne va pas »), et une phrase sur ce que tu as vu à la place, dans la ligne « Remarques : » de la section.

Fais les sections dans l'ordre : chacune part de l'état laissé par la précédente.

## D'abord : les choix de produit pris sans toi

Pendant l'exécution, le contrôleur a pris des décisions que seul le propriétaire peut confirmer. Elles sont en service dans la version que tu essaies. Pour chacune : ce qui a été choisi, pourquoi, ce que coûterait de la défaire, et l'étape où tu la vois. Dis, dans ta réponse, lesquelles tu gardes et lesquelles tu veux autrement.

**Les cinq du 13 novembre** (relecture du plan) :

1. **Pas de quota de fichiers avant le lot 7** (Ruling R247). Qui peut modifier une page, invité compris, peut y envoyer autant de fichiers qu'il veut, jusqu'à remplir le disque, et un disque plein arrête toute écriture de l'instance, pour tout le monde. Le README le dit : d'ici le lot 7, n'ouvre l'instance qu'à des personnes de confiance. Le défaire, c'est avancer les quotas par espace et les plafonds d'envoi au lot 3. Rien ne le montre à l'écran ; l'étape B5 montre la borne par fichier, qui n'est pas un quota.
2. **Mentionner une page copie son titre dans le document** (R251). Qui lit la mention lit ce titre, même s'il ne voit pas la page mentionnée : mentionner une page privée dans une page d'équipe en montre le titre à toute l'équipe, tel qu'il était à ce moment, sans que rien le dise à qui mentionne. Notion montre plutôt « pas d'accès ». Le défaire demande que le serveur filtre le libellé selon les droits de chaque lecteur (étapes F5, F6 et G6).
3. **Où va la copie d'une page** (R252). Sous la page parente, juste après la page, si tu peux y créer une page ; sinon, pour un membre, à la racine, dans son « Privé » ; un invité ne duplique que sous une page qu'il peut modifier. Conséquence : la copie d'une page d'équipe de premier niveau est privée, à repartager à la main (Notion la laisse dans sa section) (section J).
4. **Un favori n'avance pas la version de l'espace** (R254). Poser un favori ne fait rien relire aux autres ; un autre onglet de ton propre compte ne le voit qu'à son retour au premier plan. L'effet immédiat demanderait un signal par compte, pas celui de l'espace, qui ferait relire l'arbre à tout le monde (étape K3).
5. **Une invitation qui ne donne rien n'est pas consommée** (R257). Accepter une invitation qu'on a déjà, jugée sur le niveau réel (accès général et pages parentes compris), n'écrit rien, et le lien reste valable pour son destinataire. Coût : un membre qui accepte une invitation à un niveau qu'il a déjà par l'accès général n'est pas nommé sur la page, et la perd comme les autres membres si elle leur est fermée ensuite (étape L4).

**Pris depuis, pendant l'exécution** :

6. **16 Mio et 100 pages au plus par copie de page** (R608). Au-delà, on duplique par morceaux, une sous-page à la fois ; la copie en plusieurs étapes irait au lot 7. Lenny l'a confirmé le 16 novembre (d'après le contrôleur) : rappelé ici pour mémoire.
7. **1 000 blocs au plus par duplication de blocs** (R641). Au-delà, rien n'est copié et un message le dit. Mesuré : copier 1 000 blocs fige l'onglet environ 0,2 s (0,35 s sur une machine une fois et demie plus lente), 2 000 blocs 0,5 s, 5 000 blocs 2,4 s. Le défaire, c'est accepter un onglet figé plus longtemps, ou copier par morceaux.
8. **Échap ne ferme un message de création `[[` que depuis ce message** (R632). Pendant qu'une page demandée par `[[` se crée, un message au bas de la fenêtre nomme son titre, qui n'est plus dans le texte. Échap tapé ailleurs (pour fermer un menu, un panneau) ne le ferme pas : il faut aller sur son bouton. Sinon, un Échap pour autre chose effaçait le seul endroit où le titre tapé était encore écrit (étape G9, facultative).
9. **La copie d'une page ne s'ouvre que si tu n'as rien fait depuis la demande** (R646). Une copie peut prendre quelques secondes. Si, entre-temps, tu as cliqué, tapé, collé, déposé un texte ou changé de page, elle ne s'ouvre pas par-dessus ce que tu fais : le pied de la barre latérale dit qu'elle est prête, et où (étape J6, facultative).
10. **Pas de pourcentage pendant un envoi** (R667). La page dit qu'un envoi est en cours (« Envoi de … », avec « Annuler »), sans dire où il en est : le navigateur ne donne pas l'avancée d'un envoi par le moyen qu'utilise l'application, et un autre moyen contournerait ses protections de session. Sur un réseau lent, on ne sait pas où en est un gros fichier (étape B6).
11. **Forme MMD des formules qui portent un `$`** (R658). Une formule en ligne qui contient un `$` non échappé s'écrit `<math latex="…"/>`, jamais `$…$` ; la forme `$\text{$x$}$`, que le lot 1 acceptait, est refusée par `PUT /mmd` avec la forme à écrire. Cela ne touche que les agents et les outils (section O, facultative).
12. **Un envoi raté laisse son bloc vide, sans le nom du fichier** (R685). Après un refus, une annulation ou une coupure, le nom du fichier (celui de ton poste, que tous lisent pendant l'envoi) est retiré du bloc, mais le bloc reste, vide, à supprimer ou à réutiliser. Le supprimer emporterait ce qu'une autre personne y aurait mis au même instant (étapes B5 et B6).
13. **Seul un bloc copié ou glissé depuis Mementum rattache un fichier** (R713). Un bloc image ou fichier copié d'une page de Mementum et collé dans une autre y est rattaché, et la page le dit ; un texte ou une image collés depuis une autre application, même s'ils nomment le fichier d'une page de Mementum, ne rattachent rien : sinon, un message de messagerie suffisait à faire partager, sans le savoir, le fichier d'une page privée. Coût : une image collée depuis une autre application, ou un bloc écrit par un agent, reste liée à sa première page (étapes C1 et C5).

## Avant de commencer

**Ce qu'il te faut.**

- Un navigateur récent : Chrome, Edge ou Firefox. Les étapes « ralentis le réseau » supposent Chrome ou Edge.
- Une seconde fenêtre, en navigation privée, pour jouer une autre personne. Dans la suite, « fenêtre 1 » est la tienne, « fenêtre 2 » celle de Camille, puis, à la section L, celle de Dom. Mets-les côte à côte : presque tout se juge en regardant les deux à la fois.
- Deux adresses e-mail inventées, `camille@exemple.fr` et `dom@exemple.fr`. Aucun e-mail n'est envoyé.
- Quelques fichiers sur ton poste : une image (PNG ou JPEG), une courte vidéo (MP4 ou WebM) et, si tu veux, un son (MP3), tous de moins de 20 Mio ; un PDF ; une image SVG si tu en as une ; et un fichier de plus de 20 Mio, de n'importe quel type (une vidéo, une archive).

**Une instance neuve, pas tes données.** La migration de base de la partie B (`0004`) peut encore changer jusqu'à sa clôture : une base ouverte par cette version pourrait ensuite refuser de démarrer avec la suivante. Fais donc cette recette sur un dossier de données à part. À la racine du dépôt, dans deux terminaux :

```
corepack pnpm install
cargo run -p mementum-cli -- serve --data-dir data/recette-lot3b --version-every-secs 10 --max-file-mb 20
```

```
corepack pnpm -C apps/web dev
```

Puis ouvre `http://localhost:5173` dans les deux fenêtres. `--max-file-mb 20` borne un fichier à 20 Mio au lieu de 100 : c'est ce qui rend l'étape B5 faisable avec un fichier ordinaire. `--version-every-secs 10` coupe une version de page dix secondes après la première écriture pas encore versionnée : c'est ce qui rend l'étape N2 faisable en une minute. Si `cargo` répond « Accès refusé », un autre serveur Mementum tourne déjà depuis ce dossier : arrête-le d'abord.

Si le port 5173 ou 8080 est déjà pris, choisis-en d'autres. Sous PowerShell :

```
cargo run -p mementum-cli -- serve --data-dir data/recette-lot3b --version-every-secs 10 --max-file-mb 20 --bind 127.0.0.1:8081
```

```
$env:MEMENTUM_E2E_WEB_PORT = "5174"; $env:MEMENTUM_E2E_API_PORT = "8081"; corepack pnpm -C apps/web dev
```

Puis ouvre `http://localhost:5174`. Le port du serveur sert aussi à la section O.

**Ralentir le réseau** (étapes facultatives) : dans la fenêtre concernée, F12, onglet « Réseau » (« Network »), puis, dans la liste « Pas de limitation » (« No throttling »), choisis « 3G lente » (« Slow 3G »). Remets « Pas de limitation » après l'étape.

## Ce que cette partie contient, et ne contient pas

**Dedans :**

- les fichiers dans une page : image, vidéo, son, fichier à télécharger, envoyés par dépôt, collage ou menu ;
- un fichier copié ou glissé d'une page à l'autre ;
- les blocs riches : callout, signet, équation en bloc et en ligne, table des matières, lien vers une page ;
- les mentions `@` (page, personne, date) et `[[`, qui lie ou crée une page ;
- dupliquer un bloc (Ctrl+D) et dupliquer une page ;
- les favoris, les pages récentes, le fil d'Ariane de la barre du haut ;
- quatre reports du lot 2 qui se voient : l'écran du premier compte, l'invitation sans effet, le titre non enregistré, l'invitation de page qui nomme sa page.

**Pas encore :** l'aperçu d'un signet (titre, image du site) ; les miniatures ; un quota de fichiers ; l'affichage d'un PDF ou d'un SVG dans la page (ils se téléchargent) ; `@rappel` ; les rétroliens ; les notifications (une mention ne prévient personne) ; l'icône et la couverture de page. Ne signale pas ces absences comme des défauts.

## A. Préparer l'espace, et l'écran du premier compte (6 minutes)

- [ ] **A1. Deux premiers comptes en même temps.** Ouvre l'application dans les deux fenêtres : chacune montre « Bienvenue dans Mementum ». Fenêtre 1 : crée ton compte, le premier de l'instance. Tu dois arriver dans ton espace, vide.
- [ ] **A2.** Fenêtre 2, sans recharger : remplis le même écran avec le nom « Camille », `camille@exemple.fr` et un mot de passe, puis valide. L'écran doit devenir celui de la connexion, avec « Un compte a déjà été créé sur cette instance. Connecte-toi, ou demande une invitation. », et non une erreur qui te laisse sur place.
- [ ] **A3.** Fenêtre 1 : invite `camille@exemple.fr` comme « Membre », par les réglages de l'espace, comme au lot 2. Fenêtre 2 : colle le lien, crée le compte de Camille. Camille doit arriver dans ton espace.
- [ ] **A4.** Fenêtre 1 : crée une page « Essai » dans la section « Espace », et une page « Secret » dans la section « Privé » (le « + » de la section). Sur « Essai », clique « Partager », et règle « Tous les membres de l'espace » à « Peut modifier ». C'est ainsi que tu changeras le niveau de Camille sur « Essai » dans la suite (« passe les membres à … »). Fenêtre 2 : Camille voit « Essai », pas « Secret ».

Remarques :

## B. Des fichiers dans une page, à deux (10 minutes)

Fenêtre 1 et fenêtre 2 sur « Essai ».

- [ ] **B1. Une image.** Fenêtre 1 : glisse ton image depuis l'explorateur de fichiers dans le texte de « Essai », et lâche-la. Le bloc image doit apparaître aussitôt, dire « Envoi… » un instant, puis montrer l'image. En bas à droite de la fenêtre, une boîte doit dire « Envoi de » suivi du nom du fichier, avec « Annuler », puis s'effacer. Fenêtre 2 : l'image doit apparaître chez Camille en une seconde environ, sans rechargement.
- [ ] **B2. Une vidéo, un son.** Fenêtre 1 : dépose la vidéo de la même façon. Elle doit se lire dans la page ; clique plus loin sur sa barre de temps : la lecture doit sauter à cet endroit. Fenêtre 2 : Camille doit pouvoir la lire aussi. Si tu as un son, dépose-le : il doit se lire de même.
- [ ] **B3. Un PDF, un SVG.** Fenêtre 1 : dépose le PDF. Il doit devenir un bloc fichier, avec son nom et « Télécharger », pas une image ni un aperçu. Clique « Télécharger » : le navigateur doit télécharger le PDF, sans ouvrir de page. Si tu as un SVG, dépose-le : il doit devenir lui aussi un bloc fichier à télécharger, pas une image.
- [ ] **B4. Par le menu.** Fenêtre 1 : sur une ligne vide, tape `/image` et choisis « Image ». Clique le bloc vide : un panneau propose d'envoyer un fichier depuis ton poste (ou d'intégrer une adresse web). Choisis ton image : elle doit s'afficher comme en B1.
- [ ] **B5. Trop gros.** Fenêtre 1 : dépose le fichier de plus de 20 Mio. La boîte en bas à droite doit dire, avant tout envoi, que le fichier est trop gros, avec la limite (« … est trop gros : 20 Mio au plus par fichier sur cette instance. Rien n'a été envoyé. »), jusqu'à ce que tu fermes le message. Le bloc posé par le dépôt reste, vide, **sans** le nom du fichier (choix 12) : supprime-le.
- [ ] **B6. Annuler (facultatif).** Fenêtre 1 : ralentis le réseau (« Avant de commencer »), puis dépose la vidéo. Pendant l'envoi, la boîte dit « Envoi de » et le nom, sans pourcentage (choix 10), et le bloc dit « Envoi… ». Fenêtre 2 : pendant ce temps, le bloc de Camille montre le nom du fichier (c'est une limite connue). Fenêtre 1 : clique « Annuler ». L'envoi doit s'arrêter ; le bloc reste, vide, sans le nom. Remets le réseau, et supprime le bloc vide.
- [ ] **B7. Lire sans pouvoir envoyer.** Fenêtre 1 : passe les membres à « Peut lire ». Fenêtre 2 : la page passe en lecture seule. Camille voit toujours l'image et la vidéo, et peut télécharger le PDF (« Télécharger ») ; glisser un fichier dans la page ne doit rien envoyer. Fenêtre 1 : remets les membres à « Peut modifier » ; fenêtre 2 : « Rouvrir la page ».
- [ ] **B8. Un fichier qui ment (facultatif).** Dans le Bloc-notes, écris `<script>alert("perdu")</script>` et enregistre-le sous le nom `photo.png` (type « Tous les fichiers »). Fenêtre 1 : dépose ce `photo.png` dans « Essai ». Il doit devenir un bloc fichier, pas une image ; « Télécharger » doit télécharger le fichier, et aucune fenêtre « perdu » ne doit jamais s'ouvrir. Supprime ensuite le bloc.

Remarques :

## C. Un fichier venu d'une autre page (8 minutes)

« Secret » est une page privée : Camille ne la voit pas.

- [ ] **C1. Copier, coller.** Fenêtre 1 : ouvre « Secret », dépose-y ton image. Clique sur l'image (elle se sélectionne), puis Ctrl+C. Ouvre « Essai », clique sur une ligne vide, puis Ctrl+V. L'image doit apparaître, et la boîte en bas à droite dire « Un fichier d'une autre page a été rattaché à celle-ci : ceux qui lisent cette page le voient. », puis s'effacer seule au bout de quelques secondes.
- [ ] **C2.** Fenêtre 2 : Camille, qui ne lit pas « Secret », doit voir l'image collée dans « Essai ».
- [ ] **C3. Annuler le collage.** Fenêtre 1 : Ctrl+Z. Le bloc collé doit partir d'un coup, chez toi et chez Camille. Ctrl+Maj+Z : il doit revenir, et Camille le voir encore.
- [ ] **C4. Glisser d'une fenêtre à l'autre.** Ouvre une seconde fenêtre ordinaire du même navigateur (Ctrl+N, pas en navigation privée), sur « Secret », à côté de la fenêtre 1 ouverte sur « Essai ». Survole l'image de « Secret », prends la poignée à sa gauche (⋮⋮) et glisse le bloc dans le texte de « Essai », dans l'autre fenêtre. Le bloc doit arriver, avec le même message de rattachement. Fenêtre 2 : Camille doit voir cette image aussi. Ferme la seconde fenêtre ordinaire.
- [ ] **C5. Un texte venu d'ailleurs.** Fenêtre 1, sur « Secret » : clic droit sur l'image, « Copier l'adresse de l'image ». Dans le Bloc-notes, écris `![photo](`, colle l'adresse, et ferme par `)`. Dans ce texte, retire le début de l'adresse jusqu'à `/api/` exclu (il reste `![photo](/api/v1/pages/…)`). Copie toute la ligne, puis colle-la sur une ligne vide de « Essai ». Toi, tu vois l'image (tu lis « Secret »). Mais **aucun** message de rattachement ne doit paraître (choix 13), et, fenêtre 2, Camille doit voir à sa place une image cassée, pas ton image. Supprime ce bloc.

Remarques :

## D. Callout et signet (5 minutes)

Fenêtre 1 et fenêtre 2 sur « Essai ».

- [ ] **D1. Le menu.** Fenêtre 1 : sur une ligne vide, tape `/`. Le menu doit finir par un groupe « Blocs riches » : « Callout », « Équation », « Équation en ligne », « Signet », « Table des matières », « Lien vers une page ». Tape `/encadré` : « Callout » doit être proposé (les noms se trouvent en français comme en anglais).
- [ ] **D2. Un callout.** Choisis « Callout ». La ligne devient un encadré, avec une icône à gauche ; tape `Attention aux délais`. Fenêtre 2 : le callout doit arriver chez Camille.
- [ ] **D3. Son icône et sa couleur, au clavier.** Fenêtre 1 : clique l'icône du callout (son nom, au survol : « Icône et couleur du callout »). Une petite fenêtre s'ouvre : huit icônes, puis dix couleurs. Au clavier : les flèches parcourent, Entrée choisit ⚠️, puis une couleur, « Bleu » par exemple. Chaque choix doit s'appliquer aussitôt, chez toi et chez Camille. Échap ferme la fenêtre, et le focus revient au bouton de l'icône. Le texte doit rester bien lisible sur chaque couleur.
- [ ] **D4. Ctrl+Alt+Entrée.** Clique dans le texte du callout, puis Ctrl+Alt+Entrée (Cmd+Option+Entrée sur Mac) : la même fenêtre doit s'ouvrir. Échap : le curseur revient dans le texte.
- [ ] **D5. Transformer en.** Sélectionne le texte d'un paragraphe ordinaire : dans la barre de mise en forme qui apparaît, le choix du type de bloc doit proposer « Callout ».
- [ ] **D6. Un signet.** Sur une ligne vide, `/signet`, puis « Signet ». Une fenêtre s'ouvre sur un champ « Adresse de la page ». Tape `exemple.fr`, puis « Enregistrer » : la fenêtre doit dire « Une adresse commence par http:// ou https:// », et rien ne s'écrit. Tape `https://mementum.dev`, puis « Enregistrer » : la carte doit montrer l'hôte (`mementum.dev`) et l'adresse, devenue `https://mementum.dev/`. Il n'y a pas d'aperçu du site : c'est voulu (Mementum ne va chercher aucune adresse). Clique la carte : le site s'ouvre dans un autre onglet.
- [ ] **D7.** Clique « Modifier » sur le signet : la fenêtre se rouvre, avec l'adresse. Sans rien taper, fenêtre 2 : Camille clique « Modifier » sur le même signet et enregistre `https://exemple.org/`. Fenêtre 1 : le champ doit suivre la nouvelle adresse ; « Enregistrer » ne doit pas remettre l'ancienne.
- [ ] **D8. Pour un lecteur.** Fenêtre 1 : passe les membres à « Peut lire ». Fenêtre 2 : sur le callout, l'icône n'est plus un bouton ; sur le signet, plus de « Modifier ». Remets les membres à « Peut modifier », puis « Rouvrir la page » dans la fenêtre 2.

Remarques :

## E. Équations (6 minutes)

- [ ] **E1. Une équation.** Fenêtre 1 : sur une ligne vide, `/équation`, puis « Équation ». Une fenêtre s'ouvre, avec un champ et un aperçu au-dessous. Tape `\frac{a}{b} = \sqrt{2}`. L'aperçu doit suivre ta frappe ; la page, elle, ne change pas tant que tu n'as pas fini (regarde la fenêtre 2 : l'équation de Camille reste vide). Clique « Terminer » : la formule s'écrit, centrée, et arrive chez Camille.
- [ ] **E2. Une erreur.** Clique sur la formule : la fenêtre se rouvre. Efface la dernière accolade : l'aperçu montre la formule en rouge, avec ce qu'en dit KaTeX. Appuie sur Échap : la fenêtre se ferme sans rien écrire. Rouvre-la : ton texte, accolade manquante comprise, doit y être encore. Remets l'accolade, puis Ctrl+Entrée : la formule s'écrit.
- [ ] **E3. Une formule en ligne.** Dans un paragraphe, place le curseur après un mot, puis `/équation en ligne` et « Équation en ligne ». Tape `E = mc^2`, puis Ctrl+Entrée. La formule doit se placer dans la ligne, alignée sur le texte, et le curseur revenir dans le texte, où tu peux continuer à taper.
- [ ] **E4. Ce qui est refusé.** Ajoute une équation avec `\def\x{1} \x`. Elle doit se montrer en texte, avec un message qui dit que les définitions ne sont pas permises. Remplace par `x \tag{1}` : la formule s'affiche, numérotée.
- [ ] **E5. Pour un lecteur.** Fenêtre 2, membres passés à « Peut lire » : Camille voit les formules, mais un clic dessus n'ouvre rien. Remets « Peut modifier » et « Rouvrir la page ».

Remarques :

## F. Table des matières et lien de page (5 minutes)

- [ ] **F1. Les titres.** Fenêtre 1 : dans « Essai », ajoute trois titres : `# Contexte`, `## Objectifs`, `### Détails` (en début de ligne, `#` suivi d'une espace fait un titre). Sur une ligne vide en tête de la page, tape `/table des matières` et choisis « Table des matières ». Elle doit lister les trois titres, décalés selon leur niveau.
- [ ] **F2. À deux.** Fenêtre 2 : Camille ajoute un titre `## Calendrier`. Fenêtre 1 : il doit apparaître dans la table une fraction de seconde après la fin de sa frappe, sans rechargement. Garde ta souris sur la table pendant que Camille tape : la table ne doit pas bouger sous ton pointeur ; elle se met à jour quand tu en sors.
- [ ] **F3.** Clique « Détails » dans la table : la page doit défiler jusqu'au titre, et le curseur s'y placer.
- [ ] **F4. Un lien vers une page.** Sur une ligne vide, `/lien vers une page`. Une petite fenêtre s'ouvre, avec « Chercher une page » et une liste. Tape `sec` : « Secret » doit être proposé (toi, tu le vois). Entrée : le lien est posé, avec le titre « Secret ». Clique-le : « Secret » s'ouvre. Reviens sur « Essai ».
- [ ] **F5. Ce que voit Camille.** Fenêtre 2 : le lien montre « Secret », le titre copié quand tu l'as posé. Un clic mène à « Page introuvable ». Reviens sur « Essai ».
- [ ] **F6. Un renommage.** Fenêtre 1 : renomme « Secret » en « Secret v2 ». Dans « Essai », ton lien doit montrer « Secret v2 ». Fenêtre 2 : celui de Camille montre toujours « Secret » (choix 2).

Remarques :

## G. Mentions et `[[` (8 minutes)

- [ ] **G1. Le menu `@`.** Fenêtre 1 : dans un paragraphe de « Essai », tape `@`. Sur un clavier AZERTY, c'est AltGr+0 : le menu doit s'ouvrir quand même. Il montre trois groupes : « Pages », « Personnes » et « Dates », huit lignes au plus chacun.
- [ ] **G2. Une personne.** Continue en tapant `cam` : Camille doit être proposée. Flèches, puis Entrée : une mention « Camille » est posée, suivie d'une espace, et tu continues à taper dans le texte. Le focus n'a jamais quitté le texte.
- [ ] **G3. Des dates.** Tape `@demain`, Entrée : une date s'écrit, de la forme « 18 nov. 2026 ». Recommence avec `@lundi` (le prochain lundi, jamais aujourd'hui), puis `@01/12` (le 1er décembre de cette année). Tape `@31/02` : aucune date ne doit être proposée.
- [ ] **G4. Échap.** Tape `@pla`, puis Échap : le menu se ferme, et `@pla` reste dans le texte, tel que tu l'as tapé.
- [ ] **G5. Une page.** Tape `@sec`, et choisis « Secret v2 » : une mention de page est posée. Clique-la : « Secret v2 » s'ouvre. Reviens sur « Essai ».
- [ ] **G6. Ce que voit Camille.** Fenêtre 2 : Camille lit la mention « Secret v2 », le titre copié, bien qu'elle ne voie pas la page (choix 2) ; un clic n'ouvre rien d'autre que « Page introuvable ».
- [ ] **G7.** Fenêtre 2 : Camille tape `@sec` dans « Essai ». « Secret v2 » ne doit pas lui être proposé : seules les pages qu'elle voit le sont.
- [ ] **G8. `[[`.** Fenêtre 1 : tape `[[Compte rendu`. Le menu propose les pages dont le titre commence ainsi, et, en dernier, « Créer la page « Compte rendu » ». Choisis-la : une sous-page « Compte rendu » est créée sous « Essai », et une mention de cette page est posée là où tu as tapé `[[`. Fenêtre 2 : la sous-page doit apparaître dans la barre latérale de Camille, sous « Essai ».
- [ ] **G9. Le message d'une création (facultatif).** Fenêtre 1 : ralentis le réseau, puis tape `[[Annexe` et choisis « Créer la page « Annexe » ». Pendant l'attente, un message au bas de la fenêtre doit nommer « Annexe ». Clique dans le texte et appuie sur Échap : le message reste (choix 8). Mets le focus sur son bouton (Tab, ou la souris) : il se ferme. Remets le réseau. La page « Annexe » doit être créée et mentionnée.

Remarques :

## H. Dupliquer un bloc (4 minutes)

- [ ] **H1. Ctrl+D.** Fenêtre 1 : clique dans le callout de « Essai », puis Ctrl+D (Cmd+D sur Mac). Une copie identique (icône, couleur, texte) doit apparaître juste en dessous, le curseur dans la copie. Fenêtre 2 : Camille voit les deux callouts.
- [ ] **H2. Plusieurs blocs.** Sélectionne trois paragraphes à la souris, puis Ctrl+D : les trois sont copiés, dans le même ordre, après le dernier. Ctrl+Z : les trois copies partent d'un coup.
- [ ] **H3. Par la poignée.** Survole un bloc, clique sa poignée (⋮⋮) : le menu doit proposer « Dupliquer », après « Commenter ». Choisis-le : le bloc est copié. Échap, dans ce menu, rend le focus au texte.
- [ ] **H4. Les deux restent.** Fenêtre 1 et fenêtre 2 : tapez chacun quelques lettres, l'un dans l'original, l'autre dans la copie. Les deux blocs doivent rester, chez toi comme chez Camille, et chaque frappe à sa place.
- [ ] **H5. Hors du texte.** Clique dans le titre de la page, puis Ctrl+D. C'est le navigateur qui répond (son « Ajouter aux favoris »), pas Mementum : rien n'est dupliqué. Ferme sa fenêtre.

Remarques :

## I. Fil d'Ariane et récents (5 minutes)

- [ ] **I1. Une chaîne de pages.** Fenêtre 1 : dans la section « Espace », crée « Niveau 1 », puis, sous elle, « Niveau 2 » (le « + » de sa ligne), et ainsi de suite jusqu'à « Niveau 6 », chacune sous la précédente. Ouvre « Niveau 6 ». Dans la barre du haut, le fil d'Ariane doit montrer « Niveau 1 », « … », « Niveau 5 », « Niveau 6 » ; la dernière n'est pas un lien.
- [ ] **I2.** Clique « … » : un menu doit lister « Niveau 2 », « Niveau 3 », « Niveau 4 ». Au clavier : les flèches parcourent, Entrée ouvre la page, Échap ferme le menu et rend le focus à « … ». Clique « Niveau 1 » dans le fil : la page s'ouvre.
- [ ] **I3. En direct.** Fenêtre 2 : Camille ouvre « Niveau 6 ». Fenêtre 1 : glisse « Niveau 4 » sous « Niveau 1 ». Fenêtre 2 : le fil d'Ariane de Camille doit suivre, sans rechargement (« Niveau 1 », « Niveau 4 », « Niveau 5 », « Niveau 6 »). Fenêtre 1 : remets « Niveau 4 » sous « Niveau 3 ».
- [ ] **I4. Les récents.** Fenêtre 1 : ouvre l'une après l'autre « Essai », « Niveau 1 », « Niveau 2 », « Niveau 3 », « Secret v2 », « Niveau 6 ». En bas de la barre latérale, sous les trois sections, « Récents » doit montrer les cinq dernières, la plus récente en haut ; « Essai », la plus ancienne, n'y est plus. Recharge la page (F5) : la liste reste.
- [ ] **I5.** Ouvre « Compte rendu » (la sous-page créée en G8), puis mets-la à la corbeille. Elle doit quitter « Récents ». Restaure-la depuis la corbeille.
- [ ] **I6. Déconnexion.** Fenêtre 1 : déconnecte-toi (nom de l'espace, puis « Se déconnecter »), puis reconnecte-toi. « Récents » ne doit plus être là : la déconnexion l'a effacé dans ce navigateur.

Remarques :

## J. Dupliquer une page (6 minutes)

- [ ] **J1. Une sous-page.** Fenêtre 1 : menu « … » de la ligne « Niveau 2 », « Dupliquer ». La copie « Niveau 2 (copie) » doit apparaître juste sous « Niveau 2 », à côté d'elle, avec ses sous-pages (« Niveau 3 » à « Niveau 6 », un instant après), et s'ouvrir, son titre sélectionné, prêt à être changé. Fenêtre 2 : la copie doit apparaître chez Camille en une ou deux secondes. Fenêtre 1 : mets ensuite « Niveau 2 (copie) » à la corbeille, pour que la suite ne parle que de la chaîne d'origine.
- [ ] **J2. Une page de premier niveau.** Fenêtre 2 : Camille ouvre le menu « … » de la page « Essai » (en haut à droite), et choisit « Dupliquer ». Sa copie doit s'ouvrir dans **son** « Privé » (choix 3), avec les images, le callout, les formules. Fenêtre 1 : tu ne dois pas voir cette copie.
- [ ] **J3. Les fichiers suivent.** Fenêtre 2 : dans la copie de Camille, l'image collée depuis « Secret » (section C) doit s'afficher.
- [ ] **J4. À côté, ou dans « Privé ».** Fenêtre 1 : sur « Niveau 1 », passe les membres à « Peut lire » (« Partager »). Fenêtre 2 : Camille duplique « Niveau 2 ». Elle ne peut pas créer sous « Niveau 1 » : la copie doit aller dans son « Privé », et rien ne doit paraître sous « Niveau 1 » dans ta fenêtre.
- [ ] **J5. Un double clic.** Fenêtre 2 : Camille double-clique vite « Dupliquer » dans le menu d'une page. Une seule copie doit être faite.
- [ ] **J6. Un geste pendant la copie (facultatif).** Fenêtre 1 : ralentis le réseau, choisis « Dupliquer » sur « Essai », puis, tout de suite, clique dans le texte de la page. La copie ne doit pas s'ouvrir : le pied de la barre latérale dit « La copie « Essai (copie) » est prête, … », et où (choix 9). Remets le réseau.

Remarques :

## K. Favoris (4 minutes)

- [ ] **K1.** Fenêtre 1 : sur « Essai », menu « … » de la page, « Ajouter aux favoris ». Une section « Favoris » doit apparaître en tête de la barre latérale, avec « Essai ». Ajoute aussi « Niveau 3 », par le menu « … » de sa ligne. Ils doivent être listés dans l'ordre où tu les as ajoutés.
- [ ] **K2.** Fenêtre 2 : Camille ne doit pas voir tes favoris. Elle ajoute « Essai » aux siens : chez toi, rien ne change.
- [ ] **K3. Un autre onglet du même compte.** Fenêtre 1 : ouvre un second onglet sur l'application. Dans le premier onglet, retire « Niveau 3 » des favoris (« Retirer des favoris »). Passe au second onglet : « Niveau 3 » doit quitter ses favoris quelques secondes au plus après ton retour sur lui (choix 4), sans rechargement. Ferme le second onglet.
- [ ] **K4. L'accès retiré.** Fenêtre 2 : Camille met « Niveau 1 » en favori. Fenêtre 1 : sur « Niveau 1 », règle l'accès général à « Seulement les invités » (et confirme). Fenêtre 2 : « Niveau 1 » doit quitter les favoris de Camille, avec l'arbre. Fenêtre 1 : remets « Tous les membres de l'espace » à « Peut lire ». Fenêtre 2 : le favori revient à sa place.

Remarques :

## L. Le titre, les invitations, un invité (10 minutes)

- [ ] **L1. Un titre non enregistré.** Fenêtre 2 : Camille clique dans le titre de « Essai », ajoute ` v2` à la fin, **sans** valider. Fenêtre 1 : passe les membres à « Peut lire » sur « Essai ». Fenêtre 2 : la page passe en lecture seule ; le champ du titre montre de nouveau « Essai », et, sous lui, un message dit « Ce titre n'a pas été enregistré : Essai v2 ». Fenêtre 1 : le titre n'a pas changé.
- [ ] **L2.** Fenêtre 1 : remets les membres à « Peut modifier ». Fenêtre 2 : « Rouvrir la page ». Le message reste, avec « Reprendre ce titre » et « Écarter ». Clique « Reprendre ce titre » : « Essai v2 » passe dans le champ, qui a le focus, et le message part. Entrée : le titre s'enregistre, et la fenêtre 1 le voit. Remets ensuite « Essai ».
- [ ] **L3. Une invitation de page qui nomme sa page.** Fenêtre 1 : sur « Niveau 3 », « Partager », invite `dom@exemple.fr` à « Peut modifier ». Copie le lien. Dans les réglages de l'espace, la liste des invitations en attente doit montrer celle de Dom avec « Peut modifier sur « Niveau 3 » », l'échéance en tête de la ligne ; de même dans le menu « Partager » de « Niveau 3 ».
- [ ] **L4. Essayer son propre lien.** Fenêtre 1 : colle le lien de Dom dans la barre d'adresse. L'écran doit dire « Tu as déjà cet accès. Le lien reste valable pour quelqu'un d'autre. », avec un lien vers la page, et rien d'autre ne change (choix 5). L'invitation doit être toujours en attente dans les réglages.
- [ ] **L5. Dom arrive.** Fenêtre 2 : déconnecte Camille. Colle le lien de Dom, crée son compte (nom « Dom »). Dom doit arriver dans ton espace, en invité : sa barre latérale ne montre que « Niveau 3 » et ses sous-pages, rien d'autre.
- [ ] **L6. Le fil d'Ariane d'un invité.** Fenêtre 2 : ouvre « Niveau 6 ». Le fil de Dom doit commencer à « Niveau 3 » (« Niveau 3 », « Niveau 4 », « Niveau 5 », « Niveau 6 ») : ni « Niveau 1 » ni « Niveau 2 », nulle part.
- [ ] **L7. Ce qu'on propose à un invité.** Fenêtre 1 : ouvre « Niveau 3 ». Fenêtre 2 : sur « Niveau 3 », Dom tape `@`. Les pages proposées ne sont que celles qu'il voit ; les personnes, seulement celles qui ont la page ouverte en ce moment (toi). Échap.
- [ ] **L8. Un invité duplique.** Fenêtre 2 : le menu « … » de la ligne « Niveau 4 » propose « Dupliquer » (Dom peut modifier « Niveau 3 ») : la copie va sous « Niveau 3 ». Celui de « Niveau 3 » ne propose pas « Dupliquer » : Dom ne voit pas sa page parente.
- [ ] **L9.** Fenêtre 2 : déconnecte Dom, et reconnecte Camille.

Remarques :

## M. Le dessin : direction A « Papier » (5 minutes)

Le dessin doit ressembler à Notion sans le copier : mêmes emplacements, tons de papier, un seul vert. Regarde chaque élément d'un œil critique, et note ce qui s'en écarte.

- [ ] **M1. Callout.** Un encadré aux coins doux, d'un fond pâle de la couleur choisie, l'icône à gauche, le texte bien lisible sur chacune des dix couleurs.
- [ ] **M2. Signet.** Une carte sobre, d'un trait fin : l'hôte en évidence, l'adresse en gris, coupée proprement si elle est longue.
- [ ] **M3. Équations.** Une équation centrée, à la taille du texte ; une formule en ligne alignée sur la ligne, sans la faire sauter. Une formule très large défile en dedans, sans déborder de la page.
- [ ] **M4. Table des matières et lien de page.** Une liste discrète, décalée par niveau ; un lien de page qui se lit comme un lien, avec son titre.
- [ ] **M5. Mentions.** Une mention se distingue du texte (une pastille légère), sans changer la hauteur de la ligne ; une date se lit « 1 déc. 2026 ».
- [ ] **M6. Fichiers.** Un bloc fichier montre son nom et « Télécharger » ; la boîte des envois, en bas à droite, ne cache ni le titre ni le texte, et rien n'y bouge sous ton pointeur.
- [ ] **M7. Barre latérale et barre du haut.** « Favoris » et « Récents » ressemblent aux trois autres sections (repliables de la même façon) ; le fil d'Ariane remplace le titre dans la barre du haut, sans la faire grandir.
- [ ] **M8. Fenêtres de réglage.** Les fenêtres du callout, du signet, de l'équation et du lien de page se posent près de leur bloc, toujours dans l'écran ; ce qui a le focus porte le contour vert ; Tab ne sort pas de la fenêtre, Échap la ferme.
- [ ] **M9. Les mots.** Tous les textes sont en français, tutoient, et aucun ne montre un code technique (sauf la section O, dans le terminal).

Remarques :

## N. Les cas de bord (6 minutes)

- [ ] **N1. Annuler une suppression.** Fenêtre 1 : dans « Essai », supprime le bloc de ton image (B1), puis Ctrl+Z. L'image doit revenir, et s'afficher, chez toi et chez Camille.
- [ ] **N2. Une version restaurée.** Supprime de nouveau ce bloc, attends quinze secondes, tape une ligne. Ouvre « … », « Historique », choisis une version qui a l'image, puis « Restaurer cette version ». L'image doit revenir, et s'afficher.
- [ ] **N3. La corbeille.** Mets « Essai » à la corbeille, et ouvre-la depuis la corbeille. Les images s'affichent encore, et le PDF se télécharge ; mais la page est en lecture seule, et rien ne s'y dépose. Restaure-la.
- [ ] **N4. La première page supprimée.** Mets « Secret v2 » à la corbeille, puis supprime-la pour de bon depuis la corbeille. Dans « Essai », l'image collée en C1 doit toujours s'afficher, chez toi comme chez Camille.
- [ ] **N5. L'accès retiré.** Fenêtre 1 : sur « Essai », règle l'accès général à « Seulement les invités » (et confirme). Fenêtre 2 : la page se ferme sur « Tu n'as plus accès à cette page ». Remets « Tous les membres de l'espace » à « Peut modifier ».
- [ ] **N6. Redémarrage.** Arrête le serveur (Ctrl+C dans son terminal) et relance-le avec la même commande. Recharge « Essai » : toutes les images, la vidéo et le PDF doivent être là. Dans le terminal, au démarrage, aucune ligne ne doit parler de fichiers que le dépôt n'a plus.

Remarques :

## O. Ce que lit un agent (facultatif, 5 minutes)

Pour qui veut voir le format MMD des blocs riches, que liront les agents. Il te faut le cookie `mementum_session` de la fenêtre 1 (outils du navigateur, onglet « Application », « Cookies ») et l'identifiant de « Essai » (dans l'adresse, après `/p/`). Sous Windows PowerShell 5.1, écris `curl.exe` au lieu de `curl` ; mets le port de ton serveur s'il n'est pas 8080.

- [ ] **O1.** `curl -H "Cookie: mementum_session=<la valeur>" http://127.0.0.1:8080/api/v1/pages/<identifiant>/mmd`. Tu dois y lire, entre autres : `<callout icon="⚠️" color="blue">` et son texte ; `$$`, la formule, `$$` ; `$E = mc^2$` dans sa ligne ; `<bookmark url="…"/>` ; `<toc/>` ; `<mention type="user" …>Camille</mention>` et `<mention type="date" id="2026-12-01">1 déc. 2026</mention>` ; `![](/api/v1/pages/…/files/…) {name="…"}` pour une image, `<file src="…" name="…"/>` pour le PDF.
- [ ] **O2.** Écris dans un fichier `essai.mmd` la seule ligne `$\text{$x$}$` suivie d'un retour à la ligne, puis `curl -i -X PUT -H "Cookie: mementum_session=<la valeur>" -H "Content-Type: text/markdown" --data-binary @essai.mmd http://127.0.0.1:8080/api/v1/pages/<identifiant d'une page de test>/mmd` (pas sur « Essai », qui serait remplacée). La réponse doit être `422`, avec le code `invalid_mmd`, la ligne 1, et un message qui commence par « forme non canonique : écrire » et donne la forme à écrire, `<math latex="\text{$x$}"/>` (dans la réponse JSON, les guillemets et la barre oblique y sont échappés) (choix 11).

Remarques :

## Limites connues de cette partie

- **D'ici le lot 7, n'ouvre l'instance qu'à des personnes de confiance : qui peut modifier une page peut remplir le disque, et un disque plein arrête toute écriture** (R247).
- **Mentionner une page privée dans une page d'équipe en montre le titre : qui lit la mention lit le titre copié, même sans voir la page** (R251, G6). De même pour un lien vers une page (F5). Une mention de personne garde le nom copié, même si la personne en change.
- Changer le type d'un bloc (« Transformer en », un bloc choisi dans « / » sur une ligne vide) efface ce qu'une autre personne tape au même moment dans ce bloc ; déposer un fichier sur un paragraphe vide aussi, tant que celui qui dépose n'a pas reçu cette frappe.
- Une mention ou une formule posée au milieu d'un texte pendant que quelqu'un d'autre tape plus loin dans ce bloc fait passer sa frappe avant elle (rien n'est perdu) ; deux formules posées au même instant au milieu du même texte en dupliquent la suite.
- Un envoi refusé, annulé ou interrompu laisse un bloc vide, sans le nom du fichier (R685) ; pendant l'envoi, ce nom est lu par tous ceux qui ont la page ouverte (B6). Pas de pourcentage (R667) ; un envoi coupé repart de zéro.
- Un SVG, un PDF, une image HEIC ou AVIF se téléchargent, sans s'afficher dans la page (R246). Le signet n'a pas d'aperçu (R250).
- Seuls les blocs copiés ou glissés depuis Mementum sont rattachés à la page où on les pose (R713) : un texte venu d'ailleurs, un bloc écrit par un agent, un lien dans un texte restent liés à leur première page. Cent fichiers au plus par collage.
- Les fichiers d'une page pèsent sur le disque jusqu'à sa suppression définitive, même quand plus aucun bloc ne les montre. Une sauvegarde doit prendre tout le dossier des données (la base **et** `files/`) : la copie d'avant migration ne contient que la base.
- Au-delà de 100 pages ou de 16 Mio de contenu, une page se duplique par morceaux (R608) ; une page dupliquée naît sans version ; un lien ou une mention de la copie pointe encore vers l'original.
- Au plus 1 000 blocs par duplication de blocs (R641). Un sous-bloc dupliqué pendant qu'une autre personne déplace son bloc parent peut être perdu, si le déplacement arrive au serveur avant la copie.
- Un favori posé dans un onglet ne paraît dans un autre onglet du même compte qu'au retour sur cet onglet (R254). Les récents restent dans le navigateur où on les a ouverts, et partent à la déconnexion (R255).
- Une invitation essayée sans effet reste valable pour tous ; retirer un accès ne retire pas les invitations en attente : il faut les révoquer.
- Une personne invitée, même en lecture seule, peut encore surcharger le serveur si elle le veut : les plafonds viennent au lot 7 (R121).

## Comment rendre le résultat

Donne la liste des numéros qui ne vont pas, dans la conversation où le lot a été lancé. Pour chacun : une phrase qui dit ce que tu as vu à la place, et si possible une capture d'écran. Dis aussi, pour chacun des treize choix du début, si tu le gardes. Les lignes « Remarques : » sont pour tout le reste : une gêne, une idée, un mot qui sonne faux. Chaque écart sera repris au registre du lot 3, avec la décision qu'il appelle.

Si tout est coché, dis-le aussi : c'est ce qui clôt la partie B du lot 3.
