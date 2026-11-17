// Les blocs de fichier (`image`, `video`, `audio`, `file` : ceux de BlockNote 0.57, mêmes props,
// R242), tels que l'éditeur de Mementum les montre (Task C8, R669 à R671).
//
// Leur vue reste celle de BlockNote (des éléments créés une fois, sans React), à trois choses près :
//
// - **Les props sont lues comme du texte** (`textProp`, R403) avant d'aller à BlockNote : Yjs garde
//   toute valeur JSON, et un nom qui n'était pas une chaîne faisait lever la vue du bloc (« Cannot
//   convert object to primitive value »), chez tous ceux qui ouvrent la page.
// - **Un lien pour télécharger**, là où le bloc montre le nom du fichier (le bloc fichier, et une
//   image, une vidéo ou un son dont l'aperçu est masqué) : BlockNote n'y rendait que du texte, et
//   dans un éditeur en lecture seule rien ne permettait d'avoir le fichier. Le lien ne porte que ce
//   que rend `shownUrl` (`editor/files.ts`) : pour un fichier de l'instance, le navigateur le
//   télécharge sans quitter la page (`download` ; le serveur le sert de toute façon en
//   téléchargement, R246) ; pour une adresse web, il s'ouvre dans un autre onglet, sans son
//   ouvreur ; pour toute autre adresse, pas de lien, et le bloc le dit.
// - **Le nom et la légende sont du texte isolé** (`dir="auto"`, `unicode-bidi: isolate`).
//
// L'image, la vidéo et le son eux-mêmes sont chargés par BlockNote depuis ce que rend
// `resolveFileUrl`, que l'éditeur règle sur `shownUrl` (`Editor.tsx`) : jamais l'adresse du document
// telle quelle. Copiés hors de l'éditeur, les quatre blocs ne portent de même qu'une adresse permise.
import { defaultBlockSpecs } from "@blocknote/core";
import { BLANK, isFilePath, plainName, shownUrl } from "../files";
import { element } from "./richView";

type View = {
  dom: HTMLElement;
  stopEvent?: (event: Event) => boolean;
  [key: string]: unknown;
};
type Block = { id: string; type: string; props: Record<string, unknown>; [key: string]: unknown };
type Render = (this: unknown, block: Block, editor: unknown) => View;
type PropSchema = Readonly<Record<string, { default?: unknown }>>;

/**
 * Les props de `props` telles que le schéma les veut : une chaîne là où le défaut en est une, un
 * booléen là où il en est un, un nombre fini ou rien ailleurs (`previewWidth`). Tout le reste (un
 * objet, une liste, écrits par un autre client) prend la valeur par défaut.
 */
export function cleanProps(props: Readonly<Record<string, unknown>>, schema: PropSchema): Record<string, unknown> {
  const clean: Record<string, unknown> = {};
  for (const [name, { default: fallback }] of Object.entries(schema)) {
    const value = props[name];
    if (typeof fallback === "string") clean[name] = typeof value === "string" ? value : fallback;
    else if (typeof fallback === "boolean") clean[name] = typeof value === "boolean" ? value : fallback;
    else clean[name] = typeof value === "number" && Number.isFinite(value) ? value : fallback;
  }
  return clean;
}

/**
 * Pose dans `row` (l'icône et le nom d'un fichier) de quoi l'avoir : un lien vers l'adresse permise,
 * ou ce qui dit qu'elle ne l'est pas. Rend le lien, s'il y en a un.
 */
function offer(row: HTMLElement, url: string, name: string): HTMLAnchorElement | null {
  const href = shownUrl(url);
  if (href === BLANK) {
    const unavailable = element("span", "mm-file-unavailable");
    unavailable.textContent = "Adresse non prise en charge";
    row.append(unavailable);
    return null;
  }
  const link = element("a", "mm-file-download");
  link.href = href;
  // Un îlot : le lien se suit, même dans un éditeur modifiable.
  link.setAttribute("contenteditable", "false");
  link.draggable = false;
  // Dans l'attribut, le nom est isolé (U+2068, U+2069), sans ses propres marques de direction.
  const named = `\u2068${plainName(name)}\u2069`;
  if (isFilePath(url)) {
    link.download = name;
    link.textContent = "Télécharger";
    link.setAttribute("aria-label", `Télécharger ${named}`);
  } else {
    // `download` ne vaut pas d'une origine à l'autre : le lien s'ouvre ailleurs, sans son ouvreur.
    link.target = "_blank";
    link.rel = "noopener noreferrer";
    link.textContent = "Ouvrir";
    link.setAttribute("aria-label", `Ouvrir ${named} dans un autre onglet`);
  }
  row.append(link);
  return link;
}

/** La vue de BlockNote pour `block`, ses props lues comme du texte, avec de quoi télécharger. */
function fileView(render: Render, self: unknown, block: Block, editor: unknown, schema: PropSchema): View {
  const props = cleanProps(block.props, schema);
  const view = render.call(self, { ...block, props }, editor);
  for (const text of view.dom.querySelectorAll<HTMLElement>(".bn-file-name, figcaption")) text.dir = "auto";
  const row = view.dom.querySelector<HTMLElement>(".bn-file-name-with-icon");
  if (row === null) return view;
  const link = offer(row, props.url as string, props.name as string);
  if (link === null) return view;
  const inner = view.stopEvent;
  return {
    ...view,
    // Un clic ou une touche sur le lien lui appartient : ProseMirror n'y voit ni un clic dans le
    // document, ni une frappe (Entrée y couperait le paragraphe du curseur).
    stopEvent: (event: Event) => {
      const onLink = event.target instanceof Node && link.contains(event.target);
      if (onLink && !event.type.startsWith("drag") && event.type !== "drop") return true;
      return inner?.(event) ?? false;
    },
  };
}

/**
 * Suit l'adresse `url` d'un fichier nommé `name`, comme le lien d'un bloc (`offer`) : un fichier de
 * l'instance est téléchargé sans quitter la page ; une adresse web s'ouvre dans un autre onglet, sans
 * son ouvreur ni son référent ; toute autre adresse ne fait rien. Rend vrai si quelque chose a été
 * suivi. Pour « Télécharger » de la barre de mise en forme (`Toolbar.tsx`, R681).
 */
export function followFile(url: unknown, name: string): boolean {
  const href = shownUrl(url);
  if (href === BLANK) return false;
  if (isFilePath(url)) {
    const link = document.createElement("a");
    link.href = href;
    link.download = name;
    link.hidden = true;
    document.body.append(link);
    link.click();
    link.remove();
  } else {
    window.open(href, "_blank", "noopener,noreferrer");
  }
  return true;
}

type Spec = (typeof defaultBlockSpecs)["file" | "image" | "video" | "audio"];

/**
 * Le bloc `spec` de BlockNote : sa vue passée par `fileView`, et ce qu'il devient copié hors de
 * l'éditeur (`toExternalHTML` : le presse-papiers, un export) passé par les mêmes règles : props lues
 * d'après le schéma, et l'adresse que rend `shownUrl`, jamais celle du document ; pour une adresse
 * qui n'est pas permise, le nom du fichier seul, en texte (R682). BlockNote écrivait l'adresse du
 * document dans le `src` d'une image, d'une vidéo, d'un son, et dans le `href` d'un lien quand
 * l'aperçu est masqué.
 */
function guarded<S extends Spec>(spec: S): S {
  const render = spec.implementation.render as unknown as Render;
  const external = spec.implementation.toExternalHTML as unknown as Render | undefined;
  const schema = spec.config.propSchema as PropSchema;
  return {
    ...spec,
    implementation: {
      ...spec.implementation,
      render(this: unknown, block: Block, editor: unknown) {
        return fileView(render, this, block, editor, schema);
      },
      toExternalHTML(this: unknown, block: Block, editor: unknown) {
        const props = cleanProps(block.props, schema);
        const href = props.url === "" ? "" : shownUrl(props.url);
        if (href === BLANK) {
          const text = document.createElement("p");
          text.textContent = (props.name as string) || "fichier";
          return { dom: text };
        }
        return external?.call(this, { ...block, props: { ...props, url: href } }, editor);
      },
    },
  } as unknown as S;
}

/** L'image, la vidéo et le son de BlockNote, props lues comme du texte. */
export const Image = () => guarded(defaultBlockSpecs.image);
export const Video = () => guarded(defaultBlockSpecs.video);
export const Audio = () => guarded(defaultBlockSpecs.audio);

/**
 * Le bloc fichier : celui de BlockNote (mêmes props, même `fileBlockAccept`, même lecture d'un HTML
 * collé), avec son lien pour télécharger.
 */
export const FileBlock = () => guarded(defaultBlockSpecs.file);
