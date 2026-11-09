# Maquettes de Mementum

Trois directions, toutes calquées sur la logique de Notion pour ne pas dépayser :
- barre latérale à gauche ;
- page au centre ;
- propriétés sous le titre ;
- menu « Partager » en haut à droite.

Elles montrent toutes la même scène : le coffre « Projet Mementum », une page avec la présence en direct (curseur de Marie), une suggestion de l'agent Scribe, la base « Tâches » et le menu de partage ouvert.

> Les maquettes datent d'avant l'amendement « espaces façon Notion » (conception, §4.4) : là où elles disent « coffre », lire « espace », et la bascule « Tout le coffre / Cette page seulement » du menu Partager disparaît.

| Fichier | Direction | En bref |
|---|---|---|
| [`maquettes/Main.dc.html`](maquettes/Main.dc.html) | **A · Papier : RETENUE** | La plus proche de Notion. Barre latérale claire `#F6F5F1`, page blanche, police Inter, accent vert `#23704F`, base en tableau |
| [`maquettes/Atelier.dc.html`](maquettes/Atelier.dc.html) | B · Atelier | Coffres mis en avant (couleur, membres, partage), panneau d'activité des agents à droite, base en kanban |
| [`maquettes/Nuit.dc.html`](maquettes/Nuit.dc.html) | C · Nuit | Sombre par défaut, page centrée, barre IA flottante (Ctrl+K), base en liste |

La direction A est décrite en détail dans la conception, §4.3 : couleurs, typographie, menu Partager, suggestions d'agent.

## Format des fichiers

Ce sont les sources d'un canvas de design. Chaque `*.dc.html` est un artboard (HTML + un petit composant `DCLogic`), et `canvas.json` décrit leur disposition. Les maquettes sont interactives :
- le bouton Partager ouvre le menu ;
- on choisit l'accès général ;
- on bascule entre le coffre et la page ;
- on accepte ou refuse la suggestion de l'agent.

Elles dépendent du runtime du canvas (`support.js`) : ouvertes seules dans un navigateur, elles ne s'affichent pas correctement. Elles servent de **référence pour l'interface de la phase 1** : structure, textes, couleurs, comportements.

Le canvas en ligne (privé, réservé au propriétaire) : https://claude.ai/artifact/5VPJk8gPwX1cMdwufLHTXw
