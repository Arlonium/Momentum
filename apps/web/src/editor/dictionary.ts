// Les textes de BlockNote (son dictionnaire), dits comme le reste de l'application (R766) : en
// français, en tutoyant, « Envoyer » pour un fichier qui part du poste et « Télécharger » pour un
// fichier qu'on récupère, un seul groupe « Médias » dans le menu « / », « … » plutôt que trois
// points. Le dictionnaire `fr` de BlockNote 0.57 vouvoyait (« Entrez du texte ou tapez '/'… »,
// « Cliquez pour ajouter un bloc », « Veuillez réessayer »), disait « Télécharger » pour l'envoi
// (le panneau d'un bloc de fichier : « Télécharger une image », « Erreur : échec du
// téléchargement »), rangeait l'image dans « Médias » et la vidéo, le son et le fichier dans
// « Média », et annonçait Ctrl+Maj+X pour barrer, quand c'est Ctrl+Maj+S.
//
// On part du sien, et on récrit ce qui ne va pas ; `dictionary.test.ts` relit tout le dictionnaire.
// À une montée de version de BlockNote, une clé neuve arrive telle que BlockNote la traduit, ou en
// anglais : le test le dit.
import { fr } from "@blocknote/core/locales";
import { locales as multiColumnLocales } from "@blocknote/xl-multi-column";

/** Le groupe des blocs de fichier dans le menu « / ». */
export const MEDIA_GROUP = "Médias";

const slash = fr.slash_menu;
const columns = multiColumnLocales.fr.slash_menu;

/** Le dictionnaire donné à BlockNote (`EDITOR_OPTIONS`, `Editor.tsx`). */
export const DICTIONARY = {
  ...fr,
  slash_menu: {
    ...slash,
    heading_2: { ...slash.heading_2, subtext: "Titre de deuxième niveau, pour les sections clés" },
    toggle_heading: { ...slash.toggle_heading, title: "Titre repliable 1" },
    toggle_heading_2: { ...slash.toggle_heading_2, title: "Titre repliable 2" },
    toggle_heading_3: { ...slash.toggle_heading_3, title: "Titre repliable 3" },
    quote: { ...slash.quote, aliases: ["citation", "extrait", ...slash.quote.aliases] },
    numbered_list: { ...slash.numbered_list, title: "Liste numérotée" },
    paragraph: { ...slash.paragraph, subtext: "Le corps du texte" },
    image: {
      ...slash.image,
      aliases: ["image", "envoyer image", "téléverser image", "img", "photo", "média", "url", "upload"],
      group: MEDIA_GROUP,
    },
    video: {
      ...slash.video,
      aliases: ["vidéo", "video", "envoyer vidéo", "téléverser vidéo", "mp4", "film", "média", "url"],
      group: MEDIA_GROUP,
    },
    audio: {
      ...slash.audio,
      aliases: ["audio", "son", "envoyer audio", "téléverser audio", "mp3", "média", "url"],
      group: MEDIA_GROUP,
    },
    file: {
      ...slash.file,
      aliases: ["fichier", "envoyer fichier", "téléverser fichier", "intégrer fichier", "insérer fichier", "média", "url"],
      group: MEDIA_GROUP,
    },
    divider: { ...slash.divider, title: "Séparateur", aliases: ["séparateur", "separateur", ...slash.divider.aliases] },
  },
  placeholders: {
    ...fr.placeholders,
    default: "Écris, ou tape « / » pour les commandes",
    toggleListItem: "Liste repliable",
    new_comment: "Écrire un commentaire…",
    edit_comment: "Modifier le commentaire…",
    comment_reply: "Ajouter un commentaire…",
  },
  toggle_blocks: {
    ...fr.toggle_blocks,
    add_block_button: "Liste repliable vide. Clique pour ajouter un bloc.",
  },
  formatting_toolbar: {
    ...fr.formatting_toolbar,
    strike: { ...fr.formatting_toolbar.strike, secondary_tooltip: "Mod+Maj+S" },
    file_preview_toggle: { tooltip: "Montrer ou masquer l'aperçu" },
    unnest: { tooltip: "Diminuer le retrait du bloc", secondary_tooltip: "Maj+Tab" },
  },
  file_panel: {
    upload: {
      title: "Envoyer",
      file_placeholder: {
        image: "Envoyer une image",
        video: "Envoyer une vidéo",
        audio: "Envoyer un fichier audio",
        file: "Envoyer un fichier",
      },
      upload_error: "Erreur : l'envoi a échoué",
    },
    embed: { ...fr.file_panel.embed, url_placeholder: "Saisis l'adresse" },
  },
  comments: {
    ...fr.comments,
    discard_pending_comment: "Abandonner ce commentaire ?",
  },
  versioning: {
    ...fr.versioning,
    action_failed: "Une erreur s'est produite. Réessaie.",
  },
  generic: {
    ...fr.generic,
    loading: "Chargement…",
  },
  multi_column: {
    slash_menu: {
      two_columns: { ...columns.two_columns, title: "Deux colonnes" },
      three_columns: { ...columns.three_columns, title: "Trois colonnes" },
    },
  },
};
