// Ce que la page dit de ses fichiers (Task C8, `editor/files.ts`) : les envois en cours, chacun avec
// « Annuler », et les envois refusés ou interrompus, en alerte, jusqu'à ce qu'on les ferme.
//
// Rendu par `PageView` sous la barre du haut, par-dessus la page : rien ne bouge sous le pointeur
// quand un envoi commence ou finit. Hors de l'éditeur : une session recréée (droits baissés,
// corbeille) en apporte un neuf, et le refus qui l'explique doit encore se lire.
//
// Le focus ne tombe jamais sur `body` : un bouton qui part sous lui (l'envoi annulé ou fini, le
// message fermé) le rend au texte de la page (`focusBack`). Échap ne ferme les messages que si le
// focus est dedans : tapé ailleurs, il appartient à ce qu'il ferme (R632).
import { useCallback, useLayoutEffect, useRef, useSyncExternalStore } from "react";
import { closeRefusals, filesOf, watchFiles } from "../editor/files";
import { CloseIcon } from "../icons";

type Props = {
  pageId: string;
  /** Où rendre le focus quand ce qui l'avait part (le texte de la page, sinon son titre). */
  focusBack: () => void;
};

export function FileNotices({ pageId, focusBack }: Props) {
  const files = useSyncExternalStore(
    watchFiles,
    useCallback(() => filesOf(pageId), [pageId]),
  );
  const box = useRef<HTMLDivElement>(null);
  /** Le focus est dans la boîte : React n'appelle pas `onBlur` pour un élément qu'il retire (R339). */
  const held = useRef(false);
  useLayoutEffect(() => {
    if (!held.current) return;
    const active = document.activeElement;
    if (active !== null && active !== document.body) return;
    held.current = false;
    focusBack();
  });
  if (files.uploads.length === 0 && files.refusals.length === 0) return null;
  return (
    <div
      ref={box}
      className="mm-file-notices"
      onFocus={() => {
        held.current = true;
      }}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) held.current = false;
      }}
    >
      {files.uploads.length > 0 && (
        <ul className="mm-file-uploads" aria-label="Envois en cours">
          {files.uploads.map((upload) => (
            <li key={upload.id}>
              <span className="mm-file-upload-text" role="status">
                Envoi de <bdi>{upload.name}</bdi>…
              </span>
              <button
                type="button"
                className="mm-button mm-button-small"
                aria-label={`Annuler l'envoi de ⁨${upload.name}⁩`}
                onClick={upload.cancel}
              >
                Annuler
              </button>
            </li>
          ))}
        </ul>
      )}
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
    </div>
  );
}
