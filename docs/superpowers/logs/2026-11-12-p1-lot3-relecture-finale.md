# Lot 3, partie A : relecture finale, constats et vague de correction

Ce document garde ce que la relecture finale de la partie A a trouvé, ce qui a été décidé pour chaque constat, et **où en est la vague de correction**. Lenny a mis le travail en pause le 2026-11-12 au soir, l'a repris dans la nuit, peu après minuit, et l'a remis en pause le 2026-11-13 vers 3 h 40 : **le tableau d'état ci-dessous est celui de cette seconde pause.** Il est fait pour reprendre la vague sans rien savoir de la session qui l'a lancée. À supprimer à la clôture de la partie A, avec le fichier d'[avancement](2026-11-12-p1-lot3-avancement.md) : le registre gardera les décisions.

## Ce qui a été relu

Deux passes ont relu tout ce qui a changé depuis `6b6f104` (tâches 1 à 11) : **sécurité**, et **intégration entre client et serveur**. Chaque constat est prouvé par un test jetable du relecteur, sauf mention « lu ». **Aucun constat critique.** Quatre importants (S1, S2, S3, C1), des mineurs retenus, une décision prise par le contrôleur (S5).

La troisième passe, **documentation contre le code**, n'est pas faite : elle vient après la Task 14.

## État à la pause du 2026-11-13 (3 h 40 du matin)

**En une phrase** : la piste C est finie et relue ; la piste S a fait ses sept corrections, mais sa relecture y a trouvé un défaut critique (corrigé et poussé) et trois importants, dont **aucun n'est encore corrigé** ; tout le reste de ce fichier dit quoi faire.

| Constat | Sujet | État |
|---|---|---|
| S1 | une écriture du serveur à la fois par page (restauration, `PUT /mmd`) | fait, `a859f04`, R200 ; **relu : incomplet, voir I1 plus bas** |
| S2 | suppression sur des horloges que la page n'a pas encore | fait, `414df88`, R198 et R199 ; **relu : contournable, voir C1 plus bas, corrigé** (`869a0a0`, R207, R208) |
| S3 | `PUT /mmd` garde l'identifiant des blocs inchangés | fait et relu, `9218fef`, R201 et R202 ; deux règles sans test (m2) |
| S4 | plafonds des commentaires | fait, `226496c`, R204 ; **relu : insuffisant, voir I2 plus bas** |
| S5 | l'historique se lit à partir de « Peut modifier » | **fait et relu, fermé**, `2ff3cdb`, R203 ; un mineur (m6) |
| S6 | le canal de l'espace redit sa version à chaque ping | **fait et relu, fermé**, `8dab2f5`, R205 |
| S7 | savoir si un bloc existe sans relire la page | fait et relu, `0de9b38`, R206 ; la route n'a pas de test (m1) |
| C1 à C8 | les huit corrections du client | **faits et relus** (une relecture, deux tours de correction, une re-relecture) ; R210 à R218 |
| C9 | sept parcours de bout en bout, et les e2e de C6 et C8 contre le vrai serveur | **faits**, repris sur `main` (`d5485cf`, `b0540e1`, `6c89e3a`, `21a753d` ; R219) ; **pas encore relus** |

**Piste C, close sauf relecture de ses e2e.** Dans l'atelier où ils ont été écrits (serveur d'avant C1) : suite e2e entière, 160 verts et 2 `fixme` voulus (bloc déplacé) ; budgets tenus (1 000 blocs : 160 ms au p95 ; 10 000 blocs : 1,38 s ; poste occupé). Chaque e2e a été vu rouge en défaisant provisoirement le correctif qu'il garde. Aucun défaut du serveur trouvé. Sur `main` après reprise : Vitest 516 verts et les 12 échecs attendus, typecheck propre ; la suite e2e entière, relancée sur `main` à la pause avec le correctif de C1 : 159 verts, 2 `fixme` voulus, et 1 échec, `space_without_a_role_is_not_found`, une course du test lui-même (il comparait l'écran pendant que la barre latérale chargeait encore), corrigée dans la foulée.

Ce que les e2e de C9 ont appris : quand un agent **modifie** par `PUT /mmd` le texte du bloc commenté (à sa place, même type), le bloc garde son identifiant et le fil le suit (R202) ; un membre retiré de l'espace ne le voit que par le minuteur de repli, jusqu'à dix secondes (le serveur ferme le canal sans trame, R152) ; l'e2e `typed_special_characters_roundtrip` attendait un identifiant neuf après une récriture identique : il prouve maintenant l'arrivée par une seconde écriture, avec un bloc de plus.

**Piste S : tour de correction 1 commencé, arrêté après le critique.** Voir la section « Relecture de la piste S », plus bas : c'est par là que la reprise commence.

Un implémenteur et un relecteur se sont vu refuser par l'outil l'écriture de leur fichier de rapport ; leurs comptes rendus sont passés par message, et ce fichier en garde l'essentiel.

## Règles de la vague (les deux pistes)

- TDD strict : pour chaque constat, le test d'abord, **vu rouge** (les sondes des relecteurs sont décrites plus bas : les refaire), puis le code.
- **Deux pistes sans fichier commun**, pour être menées de front : la piste S ne touche qu'à `crates/` et `packages/protocol` (types régénérés) ; la piste C ne touche qu'à `apps/web`. Aucune ne touche à `CLAUDE.md`, au README, à la conception ni à `docs/REPRISE.md` : la Task 14 les reprend d'après la section « Ce que la documentation doit dire » de ce fichier.
- Chaque piste écrit ses décisions au registre du lot, à la fin du fichier, sous son titre (`### Relecture finale, piste S`, `### Relecture finale, piste C`), avec des **numéros réservés** : **R198 à R209 pour la piste S, R210 à R219 pour la piste C**. R220 à R239 sont à la Task 13 (R220 à R222 pris par son découpage), R240 à R257 au plan de la partie B.
- Si une compilation ou un test échoue dans du code de l'autre piste, attendre deux minutes et relancer, trois fois au plus, puis le signaler sans rien corriger là-bas.
- Commits petits, un sujet chacun, au nom du dépôt, sans ligne d'attribution d'outil.
- Après la vague : une re-relecture ciblée de chaque correction, comme pour un tour de tâche.

---

## Piste S (serveur)

### S1 — Important (sécurité) : des restaurations mises en file s'exécutent après le retrait du droit

- **Scénario** : Tom (« Peut modifier ») envoie d'un coup N `POST …/versions/{A|B}/restore`, en alternant deux versions. Tous passent leurs deux contrôles dans les premières millisecondes (le `load_page` de chacun est servi avant la première restauration). N commandes `Restore` attendent alors dans la boîte de l'acteur ; `Hub::call` n'a ni borne ni délai. Olga retire Tom : le retrait est validé, mais `Hub::kick` n'attend l'acteur que 2 s (`ASK_WAIT`), puis la route répond `200`. Les restaurations restantes s'exécutent quand même, une par une.
- **Ce qu'il obtient** : le contenu de la page continue d'être remplacé après son retrait (ou après la fermeture de sa session, un changement de mot de passe, une mise à la corbeille) ; chaque restauration fait perdre ce que les autres tapent à cet instant (R188), et la page ne répond plus pendant toute la file (5 s par restauration sur 10 000 blocs).
- **Où** : `crates/mementum-server/src/routes/versions.rs` (`restore`) ; `crates/mementum-sync/src/hub.rs` (`call`, et `kick` avec `ASK_WAIT`).
- **Preuve** (serveur debug, page de 400 blocs, 70 restaurations) : 70 répondent `204` ; le retrait est validé en base à 223 ms et répondu à 2 226 ms ; la dernière restauration passe à 3 337 ms ; 62 restaurations passent plus de 100 ms après la validation du retrait, 15 après sa réponse. Même test avec `PUT /mmd` : 29 passent, 41 reçoivent `404`, 24 écrivent après la validation du retrait (le défaut est antérieur au lot pour `PUT /mmd` ; la restauration le rend moins cher : aucun corps à envoyer).
- **Décision** : **une écriture du serveur à la fois par page.** Un verrou asynchrone par page (dans l'état de l'application), pris par `restore` et par `put_mmd` autour de « second contrôle des droits, commande à l'acteur, réponse » : chaque requête suivante relit alors ses droits après la précédente, et un droit retiré arrête la file. Le verrou ne doit pas fuir de mémoire (une entrée par page vivante, retirée quand plus personne ne l'attend), ni permettre à une page d'en bloquer une autre.
- **Tests à garder** : le test du relecteur, inversé, pour `restore` **et** pour `PUT /mmd` (après la validation du retrait, au plus une écriture en cours répond `204`, les autres `404`) ; et un test que deux pages ne s'attendent pas l'une l'autre.

### S2 — Important (sécurité) : un jeu de suppressions sur les horloges à venir du client d'un autre efface tout ce qu'il tapera

- **Scénario** : Tom (éditeur) lit le `ClientID` de Mona dans la présence ou dans une mise à jour. Il envoie une mise à jour de dix octets : aucun bloc, un jeu de suppressions `{client de Mona : [horloge actuelle, +1 000 000)}`. Le serveur l'accepte, l'écrit au journal et la relaie (R181 a choisi d'écrire et de relayer une suppression en attente).
- **Ce qu'il obtient** : tout ce que Mona tape ensuite est supprimé à l'arrivée, sur le serveur et chez tous ; rien n'en reste dans aucune version. Chez Mona, son texte reste affiché jusqu'à la prochaine mise à jour reçue, puis disparaît ; son onglet n'émet rien. Elle n'en sort qu'en rechargeant (nouveau `ClientID`), que Tom peut viser de nouveau. Variante lue, non prouvée : viser le `ClientID` du document du serveur ferait arriver vides les `PUT /mmd` et les restaurations qui suivent.
- **Statut** : antérieur au lot (la base ne refusait que les blocs en attente, pas les suppressions en attente) ; ce lot a regardé le cas (R181) et l'a laissé passer.
- **Où** : `crates/mementum-sync/src/actor.rs` (branche `has_pending_deletions` de `client_update`) ; `crates/mementum-crdt/src/page.rs` ; `crates/mementum-crdt/src/guard.rs` (`Shape::awaited` ne regarde que les blocs).
- **Preuve** : test jetable de `mementum-sync` : Tom n'est pas fermé, `doc_updates` passe de 2 à 3, puis, après « deux » tapé par Mona, le serveur, Mona, Tom, un arrivant et la page rechargée lisent tous « Alpha un ». Avec le vrai Yjs 13.6.33 dans Node : Mona vient de taper « Alpha un deux », Léa a reçu « Alpha un ».
- **Décision** : **une suppression qui vise des horloges que la page n'a pas encore est traitée comme un trou (R131)** : `Shape::awaited` rend aussi, par client, la fin de la plus lointaine plage supprimée ; la mise à jour est retenue dans la file de sa connexion (mêmes bornes : 32 mises à jour, 1 Mio ; perdue à la fermeture de la connexion) jusqu'à ce que la page y soit. Le cas honnête passe toujours : une suppression qui suit un ajout retenu attend derrière lui, dans l'ordre. La branche `has_pending_deletions` de R181 n'a alors plus d'usage. `PageDoc::apply_update` (relecture du stockage) ne change pas.
- **Tests à garder** : le test du relecteur avec l'attente inverse (le texte de Mona reste ; la mise à jour de Tom est retenue, puis perdue à sa fermeture) ; une suppression honnête arrivée avant l'ajout qu'elle vise est appliquée quand il arrive ; la variante sur le client du serveur ; les tests de fil (R130) et des retenues (R131) restent verts sans changer de sens.
- **Où c'en est** : voir « Reprendre S2 », plus bas.

### S3 — Important (intégration) : une récriture par `PUT /mmd`, même identique, détache tous les fils de leur bloc

- **Scénario** : Alice ouvre un fil sur le bloc « Second ». Un agent relit la page (`GET /mmd`), puis la récrit exactement telle quelle (`PUT /mmd`). Chaque bloc reçoit un nouvel identifiant : le fil reste, mais passe sous « Bloc supprimé » (plus de pastille, plus de lien vers le bloc). Le seul moyen de le rattacher est de restaurer une version d'avant, ce qui efface l'écriture de l'agent.
- **Où** : `crates/mementum-server/src/routes/pages.rs` (`GET /mmd` rend sans ancres ; `PUT /mmd` fait `parse`, puis `write_blocks`) ; `crates/mementum-sync/src/actor.rs` (`write`) ; `routes/comments.rs` (le fil garde l'identifiant du bloc).
- **Preuve** : test jetable `probe_identical_mmd_rewrite_keeps_threads_on_their_blocks`, rouge : le MMD lu est `"Premier\n\nSecond\n"`, le MMD relu est identique, et aucun des deux blocs d'après ne porte l'identifiant du bloc commenté.
- **Décision** : dans l'écriture de `PUT /mmd` (pas dans une restauration, dont les blocs portent déjà leurs identifiants), avant `write_blocks`, **un bloc lu sans identifiant reçoit celui du bloc courant qui lui correspond** : plus longue sous-suite commune, liste de frères par liste de frères, sur (type, contenu, props) ; puis, pour ce qui reste, même position et même type. Un identifiant n'est jamais donné deux fois ; un bloc qui porte déjà un identifiant le garde ; le coût reste linéaire ou quasi linéaire sur une page de 10 000 blocs (le mesurer, sans dégrader les mesures existantes de `PUT /mmd`). C'est un sous-ensemble de l'écriture par différence du lot 5 (R40) : dire au registre ce que cette règle fait et ne fait pas (elle garde les identifiants ; elle ne garde pas les éditions concurrentes).
- **Tests à garder** : la sonde du relecteur ; `edited_mmd_keeps_threads_on_unchanged_blocks` (un bloc modifié, un inséré, un retiré, deux échangés : les blocs inchangés gardent leur identifiant, les fils restent attachés) ; blocs imbriqués ; deux blocs identiques (le premier garde le sien, pas de doublon) ; un test de propriétés : après un `PUT` du MMD lu, tous les identifiants sont ceux d'avant.

### S4 — Mineur retenu (sécurité) : commentaires sans plafond

- Un invité au niveau « Peut commenter » peut dégrader durablement une page pour tous : chaque onglet lit tous les fils à sa première synchro, panneau fermé, puis à chaque trame `comments` ; 10 000 messages font 3,6 Mio par lecture ; un fil sur chaque bloc d'une grande page fait mesurer une boîte par bloc marqué à chaque image ; le nettoyage est d'un `DELETE` par message, par quelqu'un en accès complet.
- **Décision** : trois plafonds durs et généreux, **vérifiés dans la transaction qui écrit** (`crates/mementum-store/src/comments.rs`), en constantes nommées : **2 000 fils par page** (résolus compris), **500 messages par fil**, **32 emojis distincts par message**. Au-delà : un refus `409` au code propre (`thread_limit`, `message_limit`, `reaction_limit`), avec un message qui tutoie et dit quoi faire (résoudre ou supprimer des fils…). Ruling au registre : le plan renvoyait tous les plafonds au lot 7 ; dire pourquoi ceux-ci viennent maintenant.
- **Tests à garder** : `threads_of_a_page_are_capped`, `messages_of_a_thread_are_capped`, `reactions_of_a_message_are_capped` (store, avec la borne et la borne plus un), et les refus des routes.

### S5 — Décision du contrôleur : l'historique se lit à partir de « Peut modifier »

- **Prouvé** par le chemin réel : Olga écrit un secret dans une page privée, puis le récrit ; elle partage la page en lecture avec une invitée ; l'invitée lit `…/versions/{id}/state` et y trouve le secret. Rien ne permet de retirer une version : un texte collé par erreur reste lisible de tout lecteur jusqu'à cent versions plus tard.
- **Décision** : `GET /api/v1/pages/{id}/versions` et `…/versions/{versionId}/state` exigent **`edit`** (par `permit_level`, pour que la lecture reste possible à la corbeille pour qui a ce niveau). Le plan disait `read` ; la conception ne dit pas le niveau. Ruling au registre : pourquoi (un lecteur ne doit pas lire ce qui a été effacé avant qu'on partage avec lui) ; coût si faux (un lecteur ne voit plus l'historique ; c'est deux lignes à défaire). Mettre à jour le tableau des routes de `lib.rs`, les tests de niveaux, et l'ordre des refus (inchangé : page, niveau, version, corbeille).
- **Cette décision attend l'avis du propriétaire** (elle est réversible) : voir l'avancement, « Ce qui attend le propriétaire ».

### S6 — Reporté à corriger (intégration) : canal de l'espace à demi ouvert, côté serveur

- Le client ne voit pas les pings ; un canal mort sans bruit laisse la barre latérale périmée sans limite (le minuteur de repli est arrêté tant que le canal est tenu pour vivant). C'est l'équivalent manquant de R107.
- **Décision, part du serveur** : à chaque échéance de ping, le canal **réécrit aussi `{"lsn":N}`** (la version courante), après les mêmes contrôles de session et de rôle qu'aujourd'hui. Le client ignore un numéro égal (R156) et s'en sert comme signe de vie. Rien d'autre qu'un numéro ne sort toujours ; le critère de vie par pong (R153) ne change pas. Test : sans aucune écriture dans l'espace, une trame `{"lsn":N}` arrive à chaque ping.

### S7 — Mineur (intégration) : chaque commentaire sur un bloc fait relire toute la page à son acteur

- `routes/comments.rs` passe par `read_blocks` (`Command::Read`) : environ 39 ms en release sur 10 000 blocs, pendant lesquelles les frappes de cette page attendent.
- **Décision** : une commande d'acteur qui répond seulement « ce bloc existe-t-il », sans bâtir la liste des blocs. Test : un commentaire sur un bloc d'une page de 10 000 blocs n'appelle pas `read_blocks` (ou mesure : sous une borne large).

---

## Piste C (client)

### C1 — Important (intégration) : un renommage fait ailleurs est écrasé par un simple clic dans le titre — **fait** (`8a8ced1`, R210)

- **Scénario** : Alice clique dans le titre sans rien taper. Bob renomme la page, et la barre latérale en direct apporte le nouveau titre en moins d'une seconde. Alice quitte le champ : au blur, l'ancien titre d'Alice est envoyé par `PATCH` et efface celui de Bob, sans que rien le dise.
- **Décision** : on n'envoie que ce que la personne a tapé. Un drapeau « saisi », levé par la saisie et remis à zéro quand le titre est repris ou enregistré ; `save` ne part que s'il est levé ; tant que la personne n'a rien tapé, le champ reprend le titre distant même pendant le focus. Si elle a tapé, sa saisie gagne.
- **Fait** : `apps/web/src/page/TitleInput.tsx`, drapeau `typed` ; curseur et sélection gardés quand le titre distant remplace le champ ; un champ en lecture seule suit le titre distant même après une saisie (écart, dit par R210). Onze tests dans `TitleInput.test.tsx` (neuf), dont `remote_rename_while_focused_without_typing_is_kept` (la sonde, vue rouge : `updatePage` appelé avec `{"title":"Ancien"}`) et le cas inverse `what_was_typed_wins_over_a_remote_rename_and_is_sent`.

### C2 — Mineur à corriger (intégration) : « Chargement… » pour toujours après un `401`, la synchro encore ouverte

- **Scénario** : la session est échue ou supprimée ; la connexion de synchro reste ouverte ; on ouvre l'historique (ou le panneau des commentaires) : la liste répond `401` ; on se reconnecte ; rien ne relit la liste. L'état reste `loading`, sans « Réessayer ».
- **Où** : `apps/web/src/history/useVersions.ts`, `HistoryDialog.tsx` ; même défaut dans `apps/web/src/comments/useComments.ts`. À l'inverse, `usePageGate.ts` et `TitleInput.tsx` attendent `afterLogin`, puis relisent.
- **Preuve** : Vitest jetable `probe_history_list_after_a_401_with_the_sync_still_open`, rouge : `reads: 1 state: loading versions: null` après 60 s.
- **Décision** : dans la lecture, sur une session perdue, attendre le retour du compte (`afterLogin`), puis relire, dans le module partagé `rereads.ts`, sans changer les autres comportements des deux hooks.
- **Test à garder** : `the_list_is_read_again_once_the_account_is_back`, pour les deux hooks.
- **Où c'en est** : voir « Reprendre C2 », plus bas.

### C3 — Mineur à corriger (intégration) : R177 ne tient pas pour une session de page en lecture seule

- Le schéma du serveur n'est relu qu'avant une connexion de synchro **qui écrit** (`apps/web/src/page/usePageGate.ts`). Un commentateur a toujours une session en lecture seule : il n'apprend jamais une mise à jour du serveur ; `commentsWritable` (`PageView.tsx`) reste vrai, et il commente depuis un onglet périmé. Même chose pour « Restaurer » depuis une session passée de lecteur à éditeur.
- **Décision** : la vérification d'avant chaque connexion relit le schéma **dès que la personne peut écrire quelque chose depuis cet onglet** (commenter compte) ; sous un autre schéma, les commentaires et la restauration passent en lecture seule (R177), sans rien changer à ce que fait l'éditeur. Test dans `usePageGate.test.ts`, et un test que le panneau cache alors ses champs.
- **Attention** : ne rien ajouter à l'ouverture d'une page par un simple lecteur (budgets) ; relire R35 avant de toucher `usePageGate.ts`.

### C4 — Mineur à corriger (intégration) : Ctrl+Alt+M sous la fenêtre modale « Historique »

- Éditeur en lecture seule, sélection restée dans la page, fenêtre « Historique » ouverte : l'écouteur `keydown` posé sur le document (`apps/web/src/comments/BlockMarks.tsx`) ouvre le panneau des commentaires derrière la fenêtre modale, inerte.
- **Décision** : le raccourci ne fait rien, et n'avale pas la touche, quand une fenêtre modale est ouverte (`dialog[open]`) ; pareil pour le raccourci de l'éditeur modifiable. Test dans les tests du raccourci.

### C5 — Reporté à corriger (intégration) : le brouillon d'une personne qui perd le droit d'écrire disparaît sans explication — **fait** (`ddfaf93`, R211)

- Après un refus `403` ou `409`, la page est relue ; le niveau tombé, la corbeille ou le schéma périmé faisaient disparaître les champs, et avec eux le texte tapé et le message du serveur.
- **Décision** : ces brouillons sont montrés dans l'encart « Textes non envoyés », avec la raison (le message du serveur s'il y en a un ; sinon « Tu ne peux plus écrire ici. ») et « Écarter » ; le texte reste sélectionnable ; si le droit revient, le brouillon retrouve son champ.
- **Fait** : `drafts.ts` (`list`, `refuse`, `refusal`, `version`), `Composer.tsx`, `CommentsPanel.tsx`, `ThreadView.tsx` (le champ de modification d'un message suit `canEditMessage`). Huit tests dans `apps/web/src/comments/unsent.test.tsx` (neuf), dont les deux sondes du relecteur avec l'attente inverse et le retour du droit (rien n'est envoyé, aucun champ ne prend le focus).

### C6 — Reporté à corriger (intégration) : canal de l'espace à demi ouvert, côté client

- **Où** : `apps/web/src/space/useSpaceEvents.ts` (aucun chien de garde).
- **Décision, part du client** (le serveur, S6, réécrit `{"lsn":N}` à chaque échéance de ping, toutes les 20 s) : un **chien de garde de 45 s** (`EVENTS_SILENCE_MS = 45_000`, armé une fois le canal ouvert) : sans aucune trame reçue pendant ce temps, le canal est fermé, `live` passe à faux (le minuteur de repli reprend), et la reconnexion habituelle repart ; et une **reconnexion immédiate sur l'événement `online`** du navigateur. Une trame au numéro égal ne relit rien (R156) mais remet le chien de garde à zéro.
- **Tests à garder** (Vitest, WebSocket simulée) : `a_silent_channel_is_closed_and_falls_back` ; `an_equal_version_keeps_the_channel_alive_without_a_reload` ; `coming_back_online_reopens_the_channel_at_once`. Pas d'e2e avant S6.

### C7 — Suite de S5 : « Historique » à partir de « Peut modifier »

- Le menu « … » ne montre « Historique » qu'à partir de « Peut modifier » (à la corbeille, il reste lisible pour qui a ce niveau, sans « Restaurer »). L'e2e `reader_sees_the_history_and_cannot_restore` change de sens : un lecteur ne voit pas l'historique ; garder un e2e pour l'éditeur à la corbeille s'il n'existe pas. Un refus `403` de la liste (droit baissé pendant que la fenêtre est ouverte) est affiché tel quel et la fenêtre se ferme proprement.

### C8 — Suite de S4 : les plafonds des commentaires

- Le serveur refusera par `409` au-delà de 2 000 fils par page, 500 messages par fil, 32 emojis distincts par message. Le client affiche le message du serveur tel quel (c'est déjà la règle) : le vérifier par un test, et que le brouillon refusé reste (il ne part pas).

### C9 — Les parcours de bout en bout qui manquent (à écrire **quand la piste S a rendu**)

Par ordre d'importance :

1. un agent récrit la page par `PUT /mmd` pendant qu'un fil est ouvert sur un bloc : le fil reste sur son bloc, la pastille aussi (S3) ;
2. un commentateur rédige ; son droit baisse, ou la page part à la corbeille : son texte reste montré, le refus se lit (C5) ;
3. B restaure une version où le bloc commenté n'existe pas, pendant que A a le panneau ouvert : le fil passe sous « Bloc supprimé », puis revient sur son bloc quand on restaure « avant une restauration » ;
4. B renomme la page pendant que A a le focus dans le titre, sans avoir rien tapé : le titre de B reste (C1) ;
5. une personne qui peut modifier a l'historique ouvert pendant qu'une autre écrit : la nouvelle version arrive dans la liste sans rien faire ;
6. la barre latérale en direct après un changement de droits : une page partagée apparaît, un accès retiré la fait disparaître, un membre retiré de l'espace le voit ;
7. les avatars suivent un niveau baissé (« (lecture) »).

Chaque e2e est vu rouge d'abord quand c'est possible (en défaisant provisoirement le correctif qu'il garde), avec des attentes de condition bornées large, jamais d'attente fixe ; un e2e qui retient les lectures de la page retient aussi celles de l'arbre.

Les relecteurs ont aussi nommé, sans qu'ils soient retenus dans cette liste : une page de 10 000 blocs avec des centaines de fils ; un `401` avec l'historique ou le panneau ouvert ; la mise en garde de restauration.

---

## Relecture de la piste S (sécurité) : constats, décisions, où en est le tour de correction 1

La relecture indépendante de S1 à S7, tournée vers la sécurité, a rendu **1 critique, 3 importants, 7 mineurs**, chacun prouvé par une sonde jetable (sauf mention « lu »). S5 et S6 sont conformes et fermées ; S2 tient face à yrs sur 40 000 mises à jour aléatoires à un client par entrée. Le tour de correction 1 a été lancé avec les décisions ci-dessous, dans cet ordre ; **la pause l'a arrêté après C1**.

| Constat | État à la pause |
|---|---|
| C1 (avec m4, m5) | **fait**, `869a0a0`, R207 et R208 ; puis `6b4370e`, R258 (une entrée du journal à client répété est écartée à la relecture) ; poussé |
| I1 | **fait**, `4dc342d`, R209, poussé |
| I3 | **plafond fait**, `b005adc`, R259 (12 000 blocs par `PUT /mmd`), poussé ; la cause est trouvée, le correctif de fond n'est pas fait |
| I2 (avec m7) | **fait**, `12a45a9` et `1035ee7`, R350 à R352, poussé : plafonds par page (10 000 messages, 4 Mio de texte, `409 page_comment_limit`), part de chaque compte (200 fils par page, 8 emojis par message), et `DELETE …/comments/{threadId}` pour l'accès complet |
| m1, m2, m3, m6 | **faits**, `00de298`, `2f0a5ac`, `aa6efe5`, `609645c`, R353 à R356, poussés |

**Le tour de correction 1 est fini** (deux implémenteurs successifs) : `cargo test --workspace`, 744 verts, 18 ignorés ; fmt et clippy propres ; Vitest 561 verts ; `comments`, `history`, `keepalive` et `live-tree` relancés en e2e sur `main` : 27 verts. **Sa re-relecture ciblée est rendue** (voir « Re-relecture du tour 1 », plus bas) : la piste n'est pas close. Ce que les implémenteurs laissent ouvert, et que la re-relecture doit trancher : **les réactions ne sont pas bornées par page** (8 réactions d'un seul compte sur chacun des 10 000 messages : 20,5 Mio et 1,5 s de lecture ; environ 14 Mio de plus par commentateur) ; **un seul compte peut encore remplir à lui seul** les 10 000 messages ou les 4 Mio d'une page (l'accès complet les libère en 200 `DELETE` au plus) ; le correctif de fond de l'écriture quadratique n'est pas fait. Mesures : une écriture sur une page pleine de commentaires, 3,4 ms (1,8 ms sur une page vide ; 12 ms sans l'index d'expression ajouté à `0003`) ; la lecture d'une page pleine, 6,6 Mio et 78 ms. Prochains numéros de Rulings libres pour la piste S : R357 à R359.

### C1 — Critique, antérieur au lot (présent depuis le lot 1) : un client répété dans une mise à jour détruit la page — **corrigé**

- **Scénario** : quelqu'un qui peut modifier une page écrit sur sa connexion de synchro une mise à jour de 26 octets où un même client paraît deux fois dans la partie « blocs » (un `Skip` à l'horloge courante, puis une plage ramassée `[0, 1)` que la page connaît déjà). `Shape` n'y voyait ni trou ni refus ; yrs écrit la plage par-dessus le bloc connu (la racine). Chaque connexion reçoit une page vide à sa prochaine étape 2 ; au compactage suivant, l'instantané écrit est l'état abîmé : la page est vide pour de bon. Avec une plage plus longue : panique en debug, état illisible par le serveur lui-même en release. Le même moyen contournait R198.
- **Corrigé** (`869a0a0`) : une mise à jour où un client paraît deux fois dans la partie « blocs » est refusée (`Decode`), qu'elle vienne d'un client ou du stockage (R207) ; en filet de R198, une mise à jour de client qui laisse une suppression en attente que la page n'avait pas est refusée, l'acteur recharge la page et ferme la connexion ; une plage de suppression vide est refusée quand elle vient d'un client, acceptée à la relecture du stockage (R208 : refusée à la relecture, une plage inoffensive déjà au journal aurait rendu sa page inchargeable ; **validé par le contrôleur**). Tests gardés : les deux sondes du relecteur inversées (crate et synchro, avec le compactage par défaut et à 2), et un test de propriétés de 4 096 mises à jour faites à la main (`hand_made_updates.rs`) : ce que la garde laisse passer ne panique pas et ne laisse rien en attente. Filet mesuré en release : frappe à 2,0 à 2,3 ms au p95 (2,07 avant).
- **Vérifié par l'implémenteur** : aucun encodeur honnête n'écrit deux entrées pour un client ni de plage de suppression vide (Yjs 13.6.33 : `writeClientsStructs`, `mergeUpdatesV2`, `diffUpdateV2`, structures en attente, étape 2 ; yrs 0.28.1 : `write_blocks_from`, `Update::encode_diff`, `merge_updates`).
- **Le doute de la pause est tranché (R258, `6b4370e`)** : à la relecture du stockage, une entrée du journal où un client paraît deux fois est **écartée et journalisée** (`error`), et la page se charge sans elle, au lieu d'être refusée en entier (avant : `500`, page inchargeable). Un instantané ne peut pas en porter (il est écrit par yrs, une entrée par client). Limite : une entrée suivante qui dépendrait de l'entrée écartée resterait en attente, et la page serait refusée à elle, comme avant.

### I1 — Important (S1) : une session fermée n'arrête pas la file des écritures du serveur

- **Scénario** : Tom envoie d'un coup 24 `PUT /mmd`, puis se déconnecte (ou la titulaire d'un compte volé change son mot de passe). Sous le verrou, le second contrôle relit les **droits** du compte, jamais la **session** : 23 écritures sur 24 passent encore. R200 écrit pourtant qu'une session fermée arrête la file.
- **Décision** : sous le verrou de la page, après le chargement et avant le second contrôle des droits, la route relit la session de la requête (`Store::session_is_live`, comme `/sync` en R88 et le canal de l'espace) et répond `401` si elle ne vaut plus ; pour `PUT /mmd` **et** pour la restauration. Tests : la sonde inversée pour les deux routes, après une déconnexion et après un changement de mot de passe (au plus une écriture sans session, les autres `401`). Corriger R200.
- **Fait (R209, `4dc342d`)** : un extracteur `SessionUser` porte la session jusqu'à la route ; sous le verrou, la session est relue avant le second contrôle des droits. Rouge vu, la sortie du relecteur : 24 réponses `204`, 23 écritures sans session. Après : au plus une écriture sans session, les autres `401`, pour `PUT /mmd` et pour la restauration, après une déconnexion comme après un changement de mot de passe. Le fichier `.patch` qui gardait ses tests est retiré du dépôt.

### I3 — Important, antérieur : l'écriture complète d'une page est quadratique en nombre de blocs frères

- **Scénario** : un `PUT /mmd` de 90 Ko (30 000 paragraphes d'un caractère) occupe l'acteur de la page 48 s en release, avec un pic de 98 Mio ; aucune frappe de cette page n'est traitée pendant ce temps, et depuis R200 la requête tient le verrou jusqu'au bout. 10 000 blocs : 5 s ; à l'inverse, 9 000 blocs rangés en 100 listes de 90 niveaux : 0,1 s. C'est donc le nombre de frères d'une même liste qui coûte (`write_all`, `crates/mementum-crdt/src/page.rs`), pas le rapprochement des identifiants de S3 (3 % au plus). La ligne en cause n'a pas été cherchée.
- **Décision** : chercher la ligne (une insertion à un index qui reparcourt la liste ?). S'il existe un correctif simple et sûr, le faire : le document écrit doit rester lisible octet pour octet de la même façon (fidélité, fixtures, `revision_is_canonical`, mesures existantes inchangées), mesuré avant et après à 10 000 et 30 000 blocs (c'est aussi le coût d'une restauration, qui attend le propriétaire). **Dans tous les cas**, plafonner le nombre de blocs d'un `PUT /mmd` : une constante choisie d'après la mesure pour que la pire écriture acceptée occupe l'acteur moins de dix secondes en release, jamais sous 10 000 blocs ; refus `422 too_many_blocks`, avant de prendre le verrou si le compte se fait sur le MMD lu. La restauration d'une version n'est pas plafonnée ; le dire au registre.
- **Fait pour le plafond (R259, `b005adc`)** : un `PUT /mmd` porte 12 000 blocs au plus (lignes de tableau comprises) ; au-delà, `422 too_many_blocks` : « ce MMD porte N blocs (lignes de tableau comprises) : une page s'écrit en 12 000 blocs au plus ; découpe-la en plusieurs pages ; rien n'a été écrit ». **La cause est trouvée** : `write_nodes` insère chaque frère à un index croissant, et yrs (`Branch::index_to_ptr`) reparcourt la liste depuis le début à chaque insertion. Mesures en release : 10 000 blocs, 5,0 à 5,3 s par récriture ; 12 000, 7,5 s ; 30 000, 50 s. **Le correctif de fond n'est pas fait** (insérer à rebours, à une place fixe) : il change l'origine Yjs de chaque écriture du serveur et l'ordre des identifiants, et demande sa propre vérification ; il est reporté, avec l'écriture par différence du lot 5 (R40). La restauration d'une version n'est pas plafonnée (environ 50 s pour une page de 30 000 blocs). Ni les cellules d'une ligne de tableau ni les suites de marques d'un paragraphe ne sont comptées.

### I2 — Important (S4) : les plafonds ne bornent pas la lecture des fils, et un invité peut fermer les fils d'une page à tous

- **(a)** Les plafonds de S4 permettent 2 000 × 500 messages de 10 000 caractères. Mesuré à 1 % du plafond des fils (21 fils, 10 001 messages) : `GET …/comments` rend 192 Mio, à chaque première synchro de chaque onglet puis à chaque trame `comments`. **(b)** Une invitée « Peut commenter » ouvre 2 000 fils en quelques secondes : plus personne n'ouvre de fil sur la page ; l'accès complet ne libère qu'une place par `DELETE` de message, qu'elle reprend aussitôt.
- **Décision**, tout vérifié dans la transaction qui écrit, en constantes nommées : **par page**, 10 000 messages et 4 Mio de texte de messages au plus (refus `409 page_comment_limit`) ; **par compte et par page**, 200 fils ouverts par un même compte, résolus compris (`409 thread_limit`, avec un message qui dit que c'est sa part), et 8 emojis distincts posés par un même compte sur un message, sur les 32 (`409 reaction_limit` ; c'est aussi m7) ; **une route pour qui a l'accès complet**, `DELETE /api/v1/pages/{id}/comments/threads/{threadId}`, qui supprime le fil avec ses messages et ses réactions en une transaction, dans l'ordre des refus de R168 (page, accès complet, fil, corbeille), trame `comments` après la validation ; elle entre dans `comment_routes` et dans le tableau de `lib.rs`. Mesurer la lecture d'une page pleine et le coût d'une écriture à la borne (compte et somme lus dans la transaction ; un index s'il en faut un : la migration `0003` peut encore changer). **Côté client : fait** (« Supprimer le fil », R360 à R364 et R370 au registre ; les refus neufs s'affichent tels quels).

### Mineurs

- **m1** (S7) : rien ne garde la route : remettre `open_thread` sur `Hub::read_blocks` laisse les tests verts. À faire : un compteur des lectures de blocs de l'acteur, ou une mesure à la route.
- **m2** (S3) : deux règles écrites sans test : « le bloc courant qui portait cet identifiant ne donne le sien à personne » (`kept_ids.rs`, `claimed`), et « seulement s'ils sont du même type » (R202 b). À faire : un bloc sans ancre identique à un bloc dont l'identifiant est pris en ancre plus bas ; un bloc qui change de type à sa place.
- **m3** (S4) : les trois messages de refus des plafonds portent une suite de 22 espaces (`crates/mementum-server/src/error.rs`, vers les lignes 255 à 269) : les `\` de fin de ligne ont été mangés par un script. À remettre à la main, avec un test : aucun message de refus du serveur ne porte deux espaces de suite. **Attention** : `apps/web/src/comments/limits.test.tsx` et l'e2e `message_limit_is_read_and_the_reply_stays` citent ces messages tels que le serveur les écrit aujourd'hui ; ils sont à mettre d'accord dans le même pas.
- **m4** (S2), **m5** (S2) : faits avec C1.
- **m6** (S5) : la trame `versions` part encore aux lecteurs, qui apprennent qu'une version vient d'être coupée (ni contenu ni auteur). **Décision** : l'envoyer aux seules connexions qui peuvent écrire (`ConnInfo::can_write`), et le dire avec R203 ; vérifier qu'aucun test du client ni e2e ne compte sur elle pour un lecteur.
- **m7** (S4) : avec I2.

### Re-relecture du tour 1, et tour de correction 2 (2026-11-13)

Le relecteur d'origine a re-relu le tour : **neuf de ses onze constats sont levés**, chacun par une mutation qui rougit (C1 avec son filet, I1, m1 à m7 ; pour I2 : la part de 200 fils par compte, les messages et le texte par page, le `DELETE` d'un fil, sans course et pour 3,15 ms au p50 sur une page pleine). Le filet de C1 est sûr pour un client honnête : 300 parcours d'onglets (4 351 mises à jour : désordre, pertes, étapes 2, compactages, récritures, restaurations), 0 refus ; il n'offre aucun levier neuf. Aucun budget ne recule (frappe à 2,06 ms au p95 ; 42,5 ms avec la coupe d'une version ; chargements à 155 à 168 ms). Suite entière : 744 verts.

**Restent, en correction (tour 2, lancé le 2026-11-13) :**

- **I2, la fin (important).** Les réactions ne sont pas bornées par page : 6,6 Mio et 237 ms au p95 sans réaction, 20,5 Mio et 1,29 s avec 8 réactions d'un compte par message, environ 14 Mio de plus par compte. Et un seul compte ferme encore les commentaires à tous : il remplit les 4 Mio de la page en 105 messages d'un fil ; un `DELETE` libère, il recommence. **Décision** : par page, 20 000 réactions au plus ; et une part par compte sur une page : 2 000 messages, 1 Mio de texte, 2 000 réactions ; tout dans la transaction, les plafonds communs avant la part du compte. Test : « Olga écrit encore quand Gina a pris toute sa part ».
- **I3, pas levé pour `PUT /mmd` (important).** Le plafond de 12 000 blocs ne compte ni les cellules ni le contenu en ligne. En release, sous le plafond : un paragraphe de 20 000 plages en gras (120 Ko), 91 s d'acteur ; un paragraphe de 40 000 sauts de ligne (240 Ko), 112 à 145 s ; un tableau de 100 lignes sur 4 000 colonnes (1,6 Mo), 20 à 25 s. Même cause que pour les frères : une insertion à un index, qui reparcourt la liste. **Décision** : `PUT /mmd` refuse (`422`, avant le verrou) un MMD dont le coût estimé dépasse un budget — la somme, sur chaque liste bâtie par insertions successives (frères, cellules d'une ligne, plages du contenu en ligne d'un bloc), du carré de sa longueur, pondérée d'après les mesures — pour que la pire écriture acceptée occupe l'acteur moins de dix secondes en release. Un correctif de fond pour une de ces listes est permis s'il est simple et que le document écrit reste lisible de la même façon. La restauration d'une version reste sans plafond.
- **N1 (mineur, neuf ; base d'avant R130 seulement).** R258 peut laisser la page dans l'état que R131 interdit : une entrée écartée, puis une frappe du même client qui n'en dépend pas : le serveur lit `["Un", "defDeux"]`, Yjs lit `["Un", "Deux"]` et garde le bloc en attente. **Décision** : après une entrée écartée, toute entrée qui laisserait un trou est écartée aussi, et journalisée.
- **N2 (mineur, neuf).** `POST …/comments` avec `blockId` ne relit pas la session après l'attente de l'acteur : 20 fils demandés pendant une longue écriture, la personne se déconnecte : 20 réponses `201` sans session. **Décision** : relire la session après le chargement de la page, comme R209.

**Où en est le tour 2** (2026-11-13) : **I2, N1 et N2 sont faits et poussés** (`f95b68e`, `3b263e2`, `296f9ee` ; R357, R358, R365, R366 ; `cargo test --workspace` : 752 verts ; `comments.spec.ts` relancé sur `main` : 14 verts). Son implémenteur s'est arrêté, contexte épuisé, avant **I3, repris par un agent neuf** (R367 à R369).

- I2, fait : 20 000 réactions par page (`409 reaction_limit`) ; la part d'un compte sur une page : 2 000 messages et 1 Mio de texte (`409 page_comment_limit`), 2 000 réactions (`409 reaction_limit`) ; les plafonds communs passent avant la part du compte. La sonde inversée passe : un compte prend toute sa part, un autre écrit encore. L'index de `0003` devient `comments_by_thread (thread_id, created_at, author_id, octet_length(body), id)`, couvrant pour les deux requêtes. **Lecture d'une page au pire cas accepté** (10 000 messages, 4 Mio, 20 000 réactions) : 10,1 Mio, 356 ms au p50, 378 ms au p95, en release ; écriture à la borne : 5,5 ms au p50 pour une réponse, 9,0 ms pour une réaction (la ramener à la milliseconde demanderait un `node_id` sur chaque réaction : non fait).
- N2, fait : `POST …/comments` avec `blockId` relit la session après le chargement de la page (R365). Aucune autre route n'attend l'acteur avant d'écrire.
- N1, fait : après une entrée du journal écartée, une entrée qui laisserait un trou est écartée aussi (R366) ; le serveur et Yjs 13.6.33 lisent la même page.

Les refus neufs, tels que le serveur les écrit : « les commentaires de cette page portent déjà 20 000 réactions : retire celles qui ne servent plus, ou demande à quelqu'un qui a l'accès complet de supprimer des fils ; rien n'a été écrit » ; « tu as déjà écrit 2 000 messages sur cette page : c'est ta part, la même pour chaque compte ; supprime ceux des tiens qui ne servent plus avant d'en écrire un autre ; rien n'a été écrit » ; « tes messages sur cette page dépasseraient 1 Mio de texte : c'est ta part, la même pour chaque compte ; raccourcis ton message, ou supprime ceux des tiens qui ne servent plus ; rien n'a été écrit » ; « tu as déjà posé 2 000 réactions sur cette page : c'est ta part, la même pour chaque compte ; retire celles des tiennes qui ne servent plus avant d'en poser une autre ; rien n'a été écrit ».

## Ce que la documentation doit dire (pour la Task 14)

Pour les constats finis :

- `CLAUDE.md`, puce neuve : le titre n'envoie que ce qui a été tapé (drapeau `typed`, `TitleInput.tsx`) ; sans saisie, il suit le titre distant même pendant le focus, curseur et sélection gardés ; un champ en lecture seule suit le titre distant (R210).
- `CLAUDE.md`, « Commentaires, côté client » : sans le droit d'écrire, les zones disparaissent mais pas les brouillons, montrés dans « Textes non envoyés » avec le refus du serveur (`Drafts.refuse`, gardé même zone démontée) ou « Tu ne peux plus écrire ici. » ; le droit revenu, ils retrouvent leur zone sans rien envoyer ni prendre le focus ; le champ de modification d'un message suit `canEditMessage` (R211).
- README, « Commentaires » : une phrase, « un texte qu'on ne peut plus envoyer (droit perdu, corbeille) reste affiché pour être copié ou écarté ».
- `CLAUDE.md` : `SESSION_LOST` et `scheduleRereads(read, afterLogin)` dans `rereads.ts` : une liste dont la lecture répond `401` attend le retour du compte puis relit ; tout hook de liste neuf prend `afterLogin` (R212).
- `CLAUDE.md`, « Vérification avant chaque connexion de synchro » : le schéma est relu aussi pour une session en lecture seule dès que la personne peut commenter ou restaurer depuis cet onglet (`writesOutsideEditor`, R213).
- `CLAUDE.md`, « Commentaires, côté client » : Ctrl+Alt+M se tait, sans avaler la touche, sous une `dialog` ouverte ; la règle est dans `isCommentShortcut` (R214).
- `CLAUDE.md`, « Barre latérale en direct » : un chien de garde de 45 s (`EVENTS_SILENCE_MS`) ferme un canal muet et rend la main au minuteur de repli ; reconnexion immédiate sur `online` ; cela suppose que le serveur écrit au moins toutes les 20 s (S6, R215).
- `CLAUDE.md`, « Historique, côté client », README et conception : l'historique se lit à partir de « Peut modifier » ; le bouton « … » disparaît quand il n'a aucune entrée ; un refus de la liste s'affiche tel quel dans la fenêtre, qui ne se ferme pas d'elle-même ; le focus revient au titre de la page si le bouton n'est plus là (R216, R217).

Piste S (R198 à R206) :

- `CLAUDE.md` : la puce des trous d'horloge (R131) s'étend aux suppressions : une suppression qui vise des horloges que la page n'a pas est retenue comme un trou, sauf les plages déjà en attente dans la page (R198, R199) ; nouvelles aides de test `deletion` et `write_var` (`crates/mementum-sync/tests/common/raw.rs`).
- `CLAUDE.md`, « Historique, côté serveur » : retirer la phrase sur `has_pending_deletions` (la branche est partie avec S2) ; la lecture des versions demande « Peut modifier » (R203) ; `PUT /mmd` et la restauration passent par le verrou de la page, pris autour du second contrôle, de la commande et de la réponse (R200).
- `CLAUDE.md` : `PUT /mmd` garde l'identifiant des blocs inchangés, et une page récrite à l'identique se restaure sans rien faire (R201, R202) ; il ne garde pas les éditions concurrentes (lot 5, R40).
- `CLAUDE.md`, « Commentaires, côté serveur » : les plafonds (2 000 fils par page, 500 messages par fil, 32 emojis distincts par message), vérifiés dans la transaction, et leurs codes `409` (R204) ; un fil sur un bloc passe par `Hub::has_block`, sans relire la page (R206).
- `CLAUDE.md`, « Canal de l'espace, côté serveur » : la version est redite à chaque ping (R205) ; un test qui compte des trames prend `NO_PING` ; le déclencheur témoin pour compter des écritures sans horloge.
- README : limites connues (`PUT /mmd` garde les identifiants mais pas les éditions concurrentes), historique à partir de « Peut modifier », plafonds des commentaires, suppression en attente possible dans une base d'avant le lot.
- Conception : synchro (R198, R199, R200, R206) ; §5.4, la version redite au ping (R205) ; §6.2, l'API MMD et les identifiants gardés (R201, R202) ; le niveau de l'historique (R203) ; les plafonds des commentaires (R204).

Piste C, après relecture :

- `CLAUDE.md`, titre : le drapeau `typed` suit chaque frappe, comparé au titre de la page ou à celui en cours d'envoi ; un envoi refusé ou doublé ne perd pas la saisie (R210, R218).
- `CLAUDE.md`, « Historique, côté client » : un menu « … » qui perd ses entrées rend le focus à la page ; un aperçu refusé par `403` se lit tel quel, sans « Réessayer », et fait relire la page (R216).
- `CLAUDE.md`, « Tester » : une attente dans un test de composant passe par l'aide commune `until` (`apps/web/src/test/hooks.ts`), qui s'arrête quand l'hôte du test quitte le document ; tout branchement neuf de `PageView` prend son test dans `PageView.test.tsx`, qui rend le composant.

Tour de correction 1 de la piste S, et part e2e de la piste C :

- `CLAUDE.md` : un client répété dans une mise à jour est refusé, à l'arrivée comme à la relecture du stockage (R207) ; une plage de suppression vide venue d'un client aussi (R208) ; R198 vaut désormais plage vide comprise ; le filet (une mise à jour de client qui laisse une suppression en attente neuve : page rechargée, connexion fermée) ; le test différentiel `hand_made_updates.rs`. Conception §5 : les mêmes règles.
- `CLAUDE.md`, pièges de méthode : un script qui écrit du Rust doit garder ses `\` de fin de ligne (m3) ; après une mutation d'un fichier Rust défaite en recopiant une sauvegarde, redonner une date au fichier (`touch`), sinon cargo garde le binaire faussé ; plusieurs agents qui commitent dans le même arbre nomment leurs chemins (`git commit -m … -- <chemins>`) ; le cache incrémental de `target/debug` a pesé 28 Go : `CARGO_INCREMENTAL=0` pour les agents.
- `CLAUDE.md`, e2e : le repli d'un canal muet se prouve sous l'horloge simulée de la page, l'espace calme en temps réel (R219, `keepalive.spec.ts`) ; un e2e qui compare tout le texte de l'écran attend d'abord que la barre latérale ait fini de charger (`space_without_a_role_is_not_found` lisait sa référence pendant « Chargement… », et échouait de temps en temps) ; `general_access_makes_a_private_page_a_space_page` a échoué une fois sous la charge de plusieurs suites lancées de front, puis passé seul : à surveiller sur le clone neuf.

Suite du tour de correction 1 de la piste S (R258, R209, R259) :

- `CLAUDE.md` : à la relecture du stockage, une entrée du journal où un client paraît deux fois est écartée et journalisée, la page se charge sans elle (R258) ; `PUT /mmd` et la restauration relisent la session sous le verrou de la page, `401` sinon (R209, extracteur `SessionUser`) ; un `PUT /mmd` porte 12 000 blocs au plus, et l'écriture complète d'une page est quadratique en nombre de blocs frères (R259 : `write_nodes` insère à un index croissant, yrs reparcourt la liste).
- README : la limite de 12 000 blocs par `PUT /mmd` ; une grande page est lente à récrire et à restaurer (5 s pour 10 000 blocs).
- Conception : §5 pour R207, R208 et R258 ; l'API MMD pour R259 ; l'ordre des refus (`401` de session sous le verrou, R209).

Fin du tour de correction 1 de la piste S (R350 à R356) :

- `CLAUDE.md`, « Commentaires, côté serveur » : les plafonds par page (10 000 messages, 4 Mio de texte : `409 page_comment_limit`) et la part de chaque compte (200 fils par page, 8 emojis par message), leurs codes et leur ordre (les plafonds communs avant la part du compte ; une modification qui allonge un message compte dans le texte de la page) ; la route `DELETE …/comments/{threadId}` (accès complet, ordre R168, trame après validation, `404` sans trame pour un fil déjà parti) ; l'index d'expression de `0003` : la requête reprend `octet_length(c.body)` telle quelle, une colonne générée casse l'index couvrant (test `page_totals_are_read_from_the_index`) ; `Hub::block_reads`, le compteur qui garde qu'un fil sur un bloc ne fait pas bâtir la liste des blocs.
- `CLAUDE.md`, « Trame de page » : `versions` ne va qu'aux connexions qui ont `can_write` (`Broadcast::editors_only`), onglet d'un autre schéma compris.
- `CLAUDE.md`, méthode : le test `no_string_of_the_server_has_a_run_of_spaces` refuse deux espaces de suite dans les chaînes du serveur.
- README : les plafonds des commentaires ; la suppression d'un fil entier par qui a l'accès complet (le bouton reste à faire côté client) ; la lecture des fils n'est bornée qu'en messages et en texte.
- Conception : les plafonds des commentaires, la route `DELETE`, le public de la trame `versions`.

Tour de correction 2 de la piste S (R357, R358, R365, R366) :

- `CLAUDE.md`, « Commentaires, côté serveur » : 20 000 réactions par page ; la part d'un compte sur une page (2 000 messages, 1 Mio de texte, 2 000 réactions), ses codes, l'ordre des refus (les plafonds communs d'abord) ; l'index étendu de `0003` ; un fil sur un bloc relit la session une fois la page chargée (R365).
- `CLAUDE.md`, la puce de R258 : après une entrée écartée, une entrée qui laisserait un trou est écartée aussi (R366).
- README et conception : les plafonds des commentaires et la part de chaque compte ; la lecture des fils d'une page est bornée à environ 10 Mio.

« Supprimer le fil », côté client (R360 à R364, R370) :

- `CLAUDE.md`, « Commentaires, côté client » : « Supprimer le fil » pour l'accès complet (`canDeleteThread`) ; sa confirmation est tenue par le panneau et rendue à côté de l'`<aside>` (R360) ; un `404` y vaut réussite (R362) ; le focus va au fil voisin choisi avant l'envoi (R363) ; la confirmation se ferme quand le droit part (R364) ; `Confirm` ignore le second clic d'un double clic sur son voile (R361) ; `snippet` (`dom.ts`) coupe un texte venu d'ailleurs entre graphèmes.
- README, « Commentaires » : qui a l'accès complet supprime un fil entier, après une confirmation qui le nomme ; c'est ce qui fait de la place quand un plafond est atteint.

Pour chaque constat fini ensuite, ajouter ici ce que la documentation doit en dire : la Task 14 part de cette liste. Sont déjà sûrs de changer : le README et `CLAUDE.md` pour l'historique (« Peut lire » lit l'historique sans restaurer : faux après S5 et C7), la puce R181 de `CLAUDE.md` (S2), le canal de l'espace (S6, C6), `PUT /mmd` et les identifiants (S3), les plafonds des commentaires (S4).

## Doutes laissés par les implémenteurs

- C5 : un refus sans rapport avec le droit (un plafond, S4) reste la raison affichée si le droit part ensuite sans nouvelle frappe ; depuis un onglet d'un autre schéma, l'encart ne dit pas de recharger, et recharger perd les brouillons (gardés en mémoire seulement) : le texte reste sélectionnable pour être copié.
- C5 : pas d'annonce aux lecteurs d'écran quand un brouillon passe dans l'encart (comme pour les orphelins).
- Passe sécurité, mineur laissé : la liste des présents donne à un invité l'identifiant UUIDv7 de chaque compte présent, donc sa date de création ; à écrire dans le coût de R136.
- Passe intégration, mineur laissé : le panneau des commentaires est dans le paquet de démarrage (129 Ko) ; les budgets tiennent.
