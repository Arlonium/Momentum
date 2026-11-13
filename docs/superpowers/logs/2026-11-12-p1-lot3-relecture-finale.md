# Lot 3, partie A : relecture finale, constats et vague de correction

Ce document garde ce que la relecture finale de la partie A a trouvé, ce qui a été décidé pour chaque constat, et **où en est la vague de correction**. Lenny a mis le travail en pause le 2026-11-12 au soir ; il a repris le 2026-11-13, et le tableau d'état ci-dessous est tenu à jour depuis. Il est fait pour reprendre la vague sans rien savoir de la session qui l'a lancée. À supprimer à la clôture de la partie A, avec le fichier d'[avancement](2026-11-12-p1-lot3-avancement.md) : le registre gardera les décisions.

## Ce qui a été relu

Deux passes ont relu tout ce qui a changé depuis `6b6f104` (tâches 1 à 11) : **sécurité**, et **intégration entre client et serveur**. Chaque constat est prouvé par un test jetable du relecteur, sauf mention « lu ». **Aucun constat critique.** Quatre importants (S1, S2, S3, C1), des mineurs retenus, une décision prise par le contrôleur (S5).

La troisième passe, **documentation contre le code**, n'est pas faite : elle vient après la Task 14.

## État (mis à jour le 2026-11-13, en cours de vague)

| Constat | Sujet | État |
|---|---|---|
| S1 | une écriture du serveur à la fois par page (restauration, `PUT /mmd`) | en cours |
| S2 | suppression sur des horloges que la page n'a pas encore | **fait**, `414df88`, R198 et R199 ; à relire |
| S3 | `PUT /mmd` garde l'identifiant des blocs inchangés | pas commencé |
| S4 | plafonds des commentaires | pas commencé |
| S5 | l'historique se lit à partir de « Peut modifier » | pas commencé |
| S6 | le canal de l'espace redit sa version à chaque ping | pas commencé |
| S7 | savoir si un bloc existe sans relire la page | pas commencé |
| C1 | le titre n'envoie que ce qui a été tapé | **fait**, `8a8ced1`, R210 ; en relecture |
| C2 | relire la liste après le retour du compte | **fait**, `abdffee`, R212 ; en relecture |
| C3 | schéma relu pour qui commente | **fait**, `9151e88`, R213 ; en relecture |
| C4 | raccourci muet sous une fenêtre modale | **fait**, `29ab75a`, R214 ; en relecture |
| C5 | le brouillon de qui perd le droit d'écrire reste montré | **fait**, `ddfaf93`, R211 ; en relecture |
| C6 | chien de garde du canal de l'espace | **part cliente faite**, `973b78e`, R215, contre un serveur simulé ; e2e après S6 |
| C7 | « Historique » à partir de « Peut modifier » | **part cliente faite**, `b3a4c36`, R216 et R217 ; l'e2e du lecteur est en `test.fixme`, à inverser après S5 |
| C8 | plafonds affichés | **test fait**, `38149d4`, contre un refus simulé ; la conversion des codes est à retirer quand S4 aura régénéré les types |
| C9 | sept parcours de bout en bout | pas commencé (après la piste S) |

Ordre de travail de la piste S : S2, S1, S3, S5, S4, S6, S7.

À la reprise du 2026-11-13, les deux pistes ont été relancées de front. La piste C a rendu sa première part (Vitest : 497 verts et les 12 échecs attendus du bloc déplacé ; e2e `pages`, `comments`, `history`, `live-tree`, `auth`, `reload`, `sharing`, `trash`, `presence`, `keepalive` verts) ; elle est en relecture indépendante. Restent pour elle, quand la piste S aura rendu : les e2e de C6, C7 et C8 contre le vrai serveur, C9, puis la suite complète et les budgets.

**Dans l'intervalle** (tant que S6 n'est pas sur `main`) : un onglet sur un espace calme ferme et rouvre son canal toutes les 45 s (une poignée de main, rien de relu). Tant que S5 n'y est pas : le client ne propose plus l'historique à un lecteur, que le serveur lui servirait encore.

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

## S2 et C2, repris

Les deux sections « Reprendre S2 » et « Reprendre C2 » de ce fichier ont servi : S2 est fait (`414df88` ; R198 et R199 au registre, avec ce que l'implémenteur a lu dans yrs 0.28.1), C2 aussi (`abdffee`, R212). Le fichier `.patch` qui gardait les tests de S2 est retiré du dépôt.

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

Pour chaque constat fini à la reprise, ajouter ici ce que la documentation doit en dire : la Task 14 part de cette liste. Sont déjà sûrs de changer : le README et `CLAUDE.md` pour l'historique (« Peut lire » lit l'historique sans restaurer : faux après S5 et C7), la puce R181 de `CLAUDE.md` (S2), le canal de l'espace (S6, C6), `PUT /mmd` et les identifiants (S3), les plafonds des commentaires (S4).

## Doutes laissés par les implémenteurs

- C5 : un refus sans rapport avec le droit (un plafond, S4) reste la raison affichée si le droit part ensuite sans nouvelle frappe ; depuis un onglet d'un autre schéma, l'encart ne dit pas de recharger, et recharger perd les brouillons (gardés en mémoire seulement) : le texte reste sélectionnable pour être copié.
- C5 : pas d'annonce aux lecteurs d'écran quand un brouillon passe dans l'encart (comme pour les orphelins).
- Passe sécurité, mineur laissé : la liste des présents donne à un invité l'identifiant UUIDv7 de chaque compte présent, donc sa date de création ; à écrire dans le coût de R136.
- Passe intégration, mineur laissé : le panneau des commentaires est dans le paquet de démarrage (129 Ko) ; les budgets tiennent.
