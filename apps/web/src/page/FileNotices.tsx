// Ce que la page dit de ses fichiers (Task C8, `editor/files.ts`) : les envois refusés ou interrompus,
// en alerte, jusqu'à ce qu'on les ferme, les nouvelles (un fichier d'une autre page rattaché à celle-ci,
// Task C9 : un état, qui part seul), et les envois en cours, chacun avec « Annuler ».
//
// Rendu par `PageView` dans une boîte posée par-dessus la fenêtre, dans son coin bas droit (R675,
// R683) : hors du flux, rien de la page ne bouge quand un envoi commence ou finit, et ni son titre ni
// le haut de son texte ne sont recouverts. Hors de l'éditeur : une session recréée (droits baissés,
// corbeille) en apporte un neuf, et le refus qui l'explique doit encore se lire.
//
// **Rien ne bouge sous le pointeur ni sous le focus dans la boîte** (la règle de R296 et R411). Elle
// tient au bas de la fenêtre et grandit vers le haut : les refus sont au-dessus des envois, le plus
// récent des envois en haut, si bien qu'un refus ou un envoi de plus ne déplace aucune ligne déjà là.
// Et tant que le pointeur ou le focus est dedans, une ligne dont l'envoi a fini reste à sa place, sans
// son bouton : sinon « Annuler » d'un autre envoi venait sous le pointeur, et un clic annulait le
// mauvais. Elle part quand le pointeur et le focus sont sortis.
//
// Le focus ne tombe jamais sur `body` : un bouton qui part sous lui (l'envoi annulé ou fini, le
// message fermé) le rend au texte de la page (`focusBack`). Échap ne ferme les messages que si le
// focus est dedans : tapé ailleurs, il appartient à ce qu'il ferme (R632).
import { useCallback, useLayoutEffect, useReducer, useRef, useSyncExternalStore } from "react";
import { closeRefusals, filesOf, notesOf, watchFiles, type Note } from "../editor/files";
import { CloseIcon } from "../icons";

type Props = {
  pageId: string;
  /** Où rendre le focus quand ce qui l'avait part (le texte de la page, sinon son titre). */
  focusBack: () => void;
};

/** Une ligne de la boîte : un envoi en cours, ou (`cancel` nul) un envoi fini resté à sa place. */
type Row = { readonly id: number; readonly name: string; readonly cancel: (() => void) | null };

export function FileNotices({ pageId, focusBack }: Props) {
  const files = useSyncExternalStore(
    watchFiles,
    useCallback(() => filesOf(pageId), [pageId]),
  );
  /** Les nouvelles de la page (un fichier d'une autre page rattaché à celle-ci) : un état, qui part seul. */
  const notes = useSyncExternalStore(
    watchFiles,
    useCallback(() => notesOf(pageId), [pageId]),
  );
  const [, again] = useReducer((n: number) => n + 1, 0);
  /** Le pointeur est dans la boîte. */
  const hovered = useRef(false);
  /** Le focus est dans la boîte : React n'appelle pas `onBlur` pour un élément qu'il retire (R339). */
  const held = useRef(false);
  /** Les lignes montrées au dernier rendu, de la plus ancienne à la plus récente. */
  const kept = useRef<readonly Row[]>([]);
  /** Les nouvelles montrées au dernier rendu. */
  const keptNotes = useRef<readonly Note[]>([]);

  // Les lignes : les envois en cours ; figées tant que le pointeur ou le focus est dans la boîte.
  let rows: readonly Row[] = files.uploads;
  if (hovered.current || held.current) {
    const live = new Map(files.uploads.map((upload) => [upload.id, upload]));
    rows = [
      ...kept.current.map((row): Row => live.get(row.id) ?? { ...row, cancel: null }),
      ...files.uploads.filter((upload) => !kept.current.some((row) => row.id === upload.id)),
    ];
  }
  kept.current = rows;
  // Les nouvelles, de même : figées tant que le pointeur ou le focus est dans la boîte.
  let said: readonly Note[] = notes;
  if (hovered.current || held.current) {
    said = [...keptNotes.current, ...notes.filter((note) => !keptNotes.current.some((shown) => shown.id === note.id))];
  }
  keptNotes.current = said;
  const empty = rows.length === 0 && files.refusals.length === 0 && said.length === 0;
  // La boîte partie, le pointeur n'y est plus (aucun événement ne le dirait).
  if (empty) hovered.current = false;

  useLayoutEffect(() => {
    if (!held.current) return;
    const active = document.activeElement;
    if (active !== null && active !== document.body) return;
    // Le bouton qui avait le focus est parti : le focus va au texte, et les lignes finies partent.
    held.current = false;
    focusBack();
    again();
  });
  if (empty) return null;
  return (
    <div
      className="mm-file-notices"
      onMouseEnter={() => {
        hovered.current = true;
      }}
      onMouseLeave={() => {
        hovered.current = false;
        again();
      }}
      onFocus={() => {
        held.current = true;
      }}
      onBlur={(event) => {
        if (event.currentTarget.contains(event.relatedTarget)) return;
        held.current = false;
        again();
      }}
    >
      {files.refusals.length > 0 && (
        <div
          className="mm-file-refusals"
          role="alert"
          onKeyDown={(event) => {
            if (event.key === "Escape") closeRefusals(pageId);
          }}
        >
          <div className="mm-file-refusal-texts">
            {files.refusals.map((refusal) => (
              <p key={refusal.id} dir="auto">
                {refusal.text}
              </p>
            ))}
          </div>
          <button
            type="button"
            className="mm-icon-button"
            aria-label="Fermer"
            onClick={() => closeRefusals(pageId)}
          >
            <CloseIcon />
          </button>
        </div>
      )}
      {said.length > 0 && (
        <div className="mm-file-notes">
          {/* La plus récente en haut, comme les envois ; un état : ni alerte, ni focus pris. */}
          {[...said].reverse().map((note) => (
            <p key={note.id} role="status" dir="auto">
              {note.text}
            </p>
          ))}
        </div>
      )}
      {rows.length > 0 && (
        <ul className="mm-file-uploads" aria-label="Envois en cours">
          {/* Le plus récent en haut : la boîte grandit vers le haut, sans déplacer les lignes déjà là. */}
          {[...rows].reverse().map((row) => (
            <li key={row.id} className={row.cancel === null ? "mm-file-upload-ended" : undefined}>
              {row.cancel === null ? (
                <span className="mm-file-upload-text">
                  L'envoi de <bdi>{row.name}</bdi> n'est plus en cours.
                </span>
              ) : (
                <>
                  <span className="mm-file-upload-text" role="status">
                    Envoi de <bdi>{row.name}</bdi>…
                  </span>
                  <button
                    type="button"
                    className="mm-button mm-button-small"
                    aria-label={`Annuler l'envoi de ${String.fromCharCode(0x2068)}${row.name}${String.fromCharCode(0x2069)}`}
                    onClick={row.cancel}
                  >
                    Annuler
                  </button>
                </>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
