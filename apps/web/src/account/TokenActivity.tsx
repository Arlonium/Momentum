// L'activité d'un jeton d'API (lot 5, Task C2), dépliée sous sa ligne dans l'écran « Jetons d'API » :
// ses appels, le plus récent d'abord (date à la seconde, issue et code du refus, outil ou route, page
// par son titre si le compte la voit encore, blocs touchés), lus par cinquante (`useTokenCalls`). Au-
// dessus, ce que le journal ne montre pas : les appels refusés par le débit, comptés sans ligne, et les
// lignes que la purge a retirées (R1244). Un filtre par issue (réussis, refusés, en erreur) relit
// l'activité depuis le début, sous cette issue.
//
// Lue seulement quand on la déplie, jamais avec la liste des jetons. Le jeton n'est nommé qu'ici, à
// son propriétaire : l'historique d'une page ne dit que « par un agent ».
//
// Une page que le compte ne voit pas est dite comme telle : ni son titre, ni son identifiant (le
// serveur ne rend son titre que si `access.rs` la montre au compte, R1368). Ce qui vient d'ailleurs (un
// titre, un outil nommé par un client) est isolé (`dir`).
//
// Rien ne bouge sous le pointeur sans un geste (R1477) :
// - la liste défile en dedans, et « Afficher plus d'appels » est sous elle, hors de la zone qui défile ;
//   la suite s'ajoute dans la liste, au-dessus du bouton, qui ne descend pas (R1094) ; un refus de la
//   suite se dit sous le bouton. Quand il n'y a plus rien à lire, une phrase de sa hauteur prend sa
//   place, et s'il avait le focus, la liste le prend (R296, R339) ;
// - la hauteur de la liste et de son pied est gardée avant leur arrivée : toute la hauteur que la liste
//   peut prendre pour un jeton qui a déjà servi (`lastUsedAt`), puis, d'une issue à l'autre, celle
//   qu'elles avaient ; elles ne rapetissent pas tant que l'activité est dépliée, si bien que la ligne du
//   jeton du dessous ne bouge pas quand une page arrive ;
// - la phrase des comptes, au-dessus du filtre, part des comptes de la liste et reste d'une issue à
//   l'autre (`useTokenCalls`, R1476).
//
// Le focus ne tombe jamais sur `body` : « Réessayer », grisé pendant le nouvel essai, part avec la
// lecture réussie, et la liste (ou le filtre, si elle est vide) prend le focus qu'il avait (R1478).
import type { TokenCall, TokenCallOutcome, TokenInfo } from "@mementum/protocol";
import { useEffect, useId, useRef, useState } from "react";
import { FormError } from "../session/forms";
import { quoted } from "./quoted";
import { useTokenCalls } from "./useTokenCalls";

const count = new Intl.NumberFormat("fr-FR");
const when = new Intl.DateTimeFormat("fr-FR", {
  day: "numeric",
  month: "short",
  year: "numeric",
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
});

/** Les issues du filtre ; `all` : toutes. */
const FILTERS: readonly { value: TokenCallOutcome | "all"; label: string; none: string }[] = [
  { value: "all", label: "Tous les appels", none: "Aucun appel pour l'instant." },
  { value: "ok", label: "Réussis", none: "Aucun appel réussi." },
  { value: "refused", label: "Refusés", none: "Aucun appel refusé." },
  { value: "error", label: "En erreur", none: "Aucun appel en erreur." },
];

const OUTCOME_LABELS: Record<TokenCallOutcome, string> = {
  ok: "Réussi",
  refused: "Refusé",
  error: "En erreur",
};

/** « 1 appel …», « 2 appels … » : `one` et `many` suivent le nombre. */
function counted(n: number, one: string, many: string): string {
  return `${count.format(n)} ${n > 1 ? many : one}`;
}

type Props = {
  token: TokenInfo;
  /** L'identifiant de la zone, que nomme `aria-controls` du bouton qui la déplie. */
  id: string;
  /** Tenue quand le compte revient, après une session perdue (`useAfterLogin`). */
  afterLogin?: () => Promise<void>;
};

export function TokenActivity({ token, id, afterLogin }: Props) {
  const [filter, setFilter] = useState<TokenCallOutcome | "all">("all");
  const outcome = filter === "all" ? null : filter;
  const read = useTokenCalls(token, outcome, afterLogin);
  const filterId = useId();
  const zone = useRef<HTMLDivElement>(null);
  const filterRef = useRef<HTMLSelectElement>(null);
  const frame = useRef<HTMLDivElement>(null);
  /**
   * La hauteur au-dessous de laquelle la liste et son pied ne descendent pas : pour un jeton qui a déjà
   * servi, toute celle qu'ils peuvent prendre (`theme.css`) ; puis, à chaque autre issue, la hauteur
   * qu'ils avaient, qui ne fait que croître.
   */
  const [reserved, setReserved] = useState<string | undefined>(
    token.lastUsedAt !== null ? "var(--mm-token-calls-reserve)" : undefined,
  );

  // La suite a été offerte sous ce filtre : quand la dernière page retire le bouton, une phrase prend
  // sa place, à la même hauteur (R296). Un autre filtre repart sans pied.
  const [offer, setOffer] = useState({ filter, offered: false });
  let offered = offer.filter === filter && offer.offered;
  if (offer.filter !== filter || (read.more && !offer.offered)) {
    offered = read.more;
    setOffer({ filter, offered });
  }

  // « Afficher plus d'appels » part avec la dernière page : s'il avait le focus, la liste le prend
  // (React n'appelle pas `onBlur` pour un élément qu'il retire, R339).
  const moreFocused = useRef(false);
  useEffect(() => {
    if (read.more || !moreFocused.current) return;
    moreFocused.current = false;
    const now = document.activeElement;
    if (now === null || now === document.body) zone.current?.focus({ preventScroll: true });
  }, [read.more]);

  // « Réessayer » part avec la première page lue : s'il avait le focus, la liste le prend, ou le filtre
  // si elle est vide.
  const retryFocused = useRef(false);
  useEffect(() => {
    if (read.calls === null || !retryFocused.current) return;
    retryFocused.current = false;
    const now = document.activeElement;
    if (now === null || now === document.body) (zone.current ?? filterRef.current)?.focus({ preventScroll: true });
  }, [read.calls]);

  const none = FILTERS.find((f) => f.value === filter)?.none ?? "";
  let content;
  if (read.calls === null && read.error !== null) {
    content = (
      <div className="mm-token-calls-error">
        <FormError message={`L'activité n'a pas pu être lue. ${read.error}`} />
        <button
          type="button"
          className="mm-button mm-button-small"
          // Grisé sans être retiré pendant le nouvel essai : il garde le focus.
          aria-disabled={read.retrying || undefined}
          onFocus={() => {
            retryFocused.current = true;
          }}
          onBlur={() => {
            retryFocused.current = false;
          }}
          onClick={() => {
            if (!read.retrying) read.retry();
          }}
        >
          Réessayer
        </button>
      </div>
    );
  } else if (read.calls === null) {
    content = <p className="mm-settings-empty">Chargement…</p>;
  } else if (read.calls.length === 0) {
    content = <p className="mm-settings-empty">{none}</p>;
  } else {
    content = (
      // Elle défile en dedans : au clavier aussi, d'où son arrêt de tabulation.
      <div
        ref={zone}
        className="mm-token-calls-zone"
        role="region"
        aria-label={`Appels de ${quoted(token.name)}`}
        tabIndex={0}
      >
        <ul className="mm-token-calls">
          {read.calls.map((call) => (
            <CallLine key={call.id} call={call} />
          ))}
        </ul>
      </div>
    );
  }

  const lead = [
    "Le journal garde les mille derniers appels réussis du jeton et ses deux cents derniers appels refusés ou en erreur.",
  ];
  if (read.purged > 0) {
    lead.push(
      read.purged > 1
        ? `${count.format(read.purged)} appels plus anciens ne sont plus gardés.`
        : `${count.format(read.purged)} appel plus ancien n'est plus gardé.`,
    );
  }
  if (read.busyCount > 0) {
    lead.push(
      read.busyCount > 1
        ? `${count.format(read.busyCount)} appels refusés par le débit (trop d'appels à la minute) ne sont pas listés.`
        : `${count.format(read.busyCount)} appel refusé par le débit (trop d'appels à la minute) n'est pas listé.`,
    );
  }

  return (
    <div id={id} className="mm-token-activity">
      <p className="mm-token-activity-lead">{lead.join(" ")}</p>
      <p className="mm-token-activity-filter">
        <label htmlFor={filterId}>Issue</label>
        <select
          id={filterId}
          className="mm-select"
          value={filter}
          ref={filterRef}
          onChange={(event) => {
            // La liste relue sous cette issue garde au moins la hauteur qu'elle avait.
            const height = frame.current?.offsetHeight ?? 0;
            if (height > 0) setReserved(`${height}px`);
            setFilter(event.target.value as TokenCallOutcome | "all");
          }}
        >
          {FILTERS.map((f) => (
            <option key={f.value} value={f.value}>
              {f.label}
            </option>
          ))}
        </select>
      </p>
      <div ref={frame} className="mm-token-calls-frame" style={{ minHeight: reserved }}>
        {content}
        {read.calls !== null && (read.more || offered || read.error !== null) && (
          // Hors de la zone qui défile : la suite s'ajoute au-dessus, sans que le bouton descende sous
          // le pointeur (R1094). Le refus de la suite se dit sous lui, pour la même raison.
          <div className="mm-token-calls-foot">
            {read.more ? (
              <button
                type="button"
                className="mm-button mm-button-small"
                // Grisé sans être retiré pendant la demande : il garde le focus.
                aria-disabled={read.loadingMore || undefined}
                onFocus={() => {
                  moreFocused.current = true;
                }}
                onBlur={() => {
                  moreFocused.current = false;
                }}
                onClick={(event) => {
                  // Le second clic d'un double clic ne demande pas une seconde page.
                  if (event.detail > 1 || read.loadingMore) return;
                  read.loadMore();
                }}
              >
                Afficher plus d'appels
              </button>
            ) : (
              offered && <p className="mm-token-calls-done">Tous les appels gardés sont affichés.</p>
            )}
            {read.error !== null && <FormError message={`La suite n'a pas pu être lue. ${read.error}`} />}
          </div>
        )}
      </div>
    </div>
  );
}

/** Un appel : sa date, son issue (et le code d'un refus ou d'une erreur), l'outil, la page, les blocs. */
function CallLine({ call }: { call: TokenCall }) {
  const date = new Date(call.at);
  const valid = !Number.isNaN(date.getTime());
  return (
    <li className="mm-token-call" data-call-id={call.id}>
      <span className="mm-token-call-head">
        <time className="mm-token-call-at" dateTime={valid ? date.toISOString() : undefined}>
          {valid ? when.format(date) : "date inconnue"}
        </time>
        <span className={`mm-tag mm-token-call-outcome is-${call.outcome}`}>{OUTCOME_LABELS[call.outcome]}</span>
        {call.code !== null && (
          <span className="mm-token-call-code" dir="ltr">
            {call.code}
          </span>
        )}
      </span>
      <span className="mm-token-call-what">
        <span className="mm-token-call-tool mm-isolate" dir="auto">
          {call.tool}
        </span>
        {call.pageId !== null && (
          <span className="mm-token-call-page">
            {" · "}
            {call.pageTitle === null ? (
              "une page que tu ne vois pas"
            ) : (
              <>
                «&nbsp;
                <span className="mm-isolate" dir="auto">
                  {call.pageTitle || "Sans titre"}
                </span>
                &nbsp;»
              </>
            )}
          </span>
        )}
        {call.blocksTouched !== null && (
          <span className="mm-token-call-blocks">
            {" · "}
            {call.blocksTouched === 0
              ? "aucun bloc touché"
              : counted(call.blocksTouched, "bloc touché", "blocs touchés")}
          </span>
        )}
      </span>
    </li>
  );
}
