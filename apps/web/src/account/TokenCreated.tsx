// Un jeton d'API tout juste créé (lot 5, Task C1) : son texte, à copier tout de suite (le serveur
// n'en garde que le hachage, et ne le rendra plus), et de quoi brancher Claude Code.
//
// **La commande ne porte jamais le jeton** (R1252, R1576) : collé dans une ligne de commande, il
// resterait dans l'historique du shell. Elle se fait en deux lignes, une par shell, au choix : la
// première lit le jeton dans la variable `MEMENTUM_TOKEN` sans l'afficher, la seconde branche
// Mementum à l'adresse que le navigateur connaît (`origin`) et nomme la variable dans l'en-tête, avec
// les guillemets que ce shell demande pour que `${MEMENTUM_TOKEN}` arrive tel quel à Claude Code :
// - bash et zsh : entre apostrophes (entre guillemets, le shell y mettrait le jeton, et la
//   configuration de Claude Code le garderait en clair) ;
// - PowerShell : entre apostrophes aussi (entre guillemets, PowerShell développerait sa propre
//   variable `$MEMENTUM_TOKEN`, d'ordinaire vide) ; `Read-Host -MaskInput` demande PowerShell 7.1, et
//   la ligne de Windows PowerShell 5.1 est donnée à côté ;
// - cmd : entre guillemets (cmd ne développe que `%…%`, et une apostrophe n'y groupe rien) ; il n'a
//   pas de saisie masquée, et l'écran le dit.
// Les deux lignes se copient une à une : collées ensemble, la seconde serait lue comme le jeton.
//
// Le texte du jeton ne vit que dans le composant qui le montre (`token`, tenu par `CreateToken`) :
// ni stockage du navigateur, ni console, ni adresse.
import { useId, useRef, useState } from "react";
import { CheckIcon, LinkIcon, WarningIcon } from "../icons";

export type Shell = "bash" | "powershell" | "cmd";

export const SHELLS: readonly { id: Shell; label: string }[] = [
  { id: "bash", label: "bash ou zsh" },
  { id: "powershell", label: "PowerShell" },
  { id: "cmd", label: "cmd" },
];

/** Le nom de la variable d'environnement qui porte le jeton. */
const VARIABLE = "MEMENTUM_TOKEN";

export type Commands = {
  /** Lit le jeton dans la variable, sans l'afficher (sauf sous cmd). */
  read: string;
  /** La même chose pour Windows PowerShell 5.1, qui n'a pas `-MaskInput`. */
  readOld?: string;
  /** Branche Mementum à Claude Code, l'en-tête nommant la variable. */
  add: string;
};

/** Les deux lignes du shell `shell`, pour l'instance servie à `origin`. */
export function commandsFor(shell: Shell, origin: string): Commands {
  const header = `Authorization: Bearer \${${VARIABLE}}`;
  const add = (quote: string) =>
    `claude mcp add --transport http mementum ${quote}${origin}/mcp${quote} --header ${quote}${header}${quote}`;
  switch (shell) {
    case "bash":
      return { read: `read -rs ${VARIABLE} && export ${VARIABLE}`, add: add("'") };
    case "powershell":
      return {
        read: `$env:${VARIABLE} = Read-Host -MaskInput`,
        readOld: `$env:${VARIABLE} = [Net.NetworkCredential]::new('', (Read-Host -AsSecureString)).Password`,
        add: add("'"),
      };
    case "cmd":
      return { read: `set /p ${VARIABLE}=Jeton : `, add: add('"') };
  }
}

type Props = {
  token: string;
  /** L'adresse de l'instance, telle que le navigateur la connaît (`location.origin`). */
  origin: string;
};

export function TokenCreated({ token, origin }: Props) {
  const shellName = useId();
  const [shell, setShell] = useState<Shell>("bash");
  const commands = commandsFor(shell, origin);
  return (
    <>
      <TokenField token={token} />
      <section className="mm-token-connect" aria-labelledby={`${shellName}-title`}>
        <h3 id={`${shellName}-title`} className="mm-token-connect-title">
          Brancher Claude Code
        </h3>
        <p className="mm-token-hint">
          Le jeton ne va pas dans la commande, où l'historique du shell le garderait : il passe par la
          variable <code>{VARIABLE}</code>. Elle doit être posée dans le shell qui lance Claude Code, à
          chaque lancement de Claude Code.
        </p>
        <div role="radiogroup" aria-label="Ton shell" className="mm-token-shells">
          {SHELLS.map((item) => (
            <label key={item.id} className="mm-token-shell">
              <input
                type="radio"
                name={`${shellName}-shell`}
                value={item.id}
                checked={shell === item.id}
                onChange={() => setShell(item.id)}
              />
              {item.label}
            </label>
          ))}
        </div>
        <ol className="mm-token-steps">
          <li>
            {shell === "cmd"
              ? "Lance cette ligne, colle le jeton, puis Entrée :"
              : "Lance cette ligne, colle le jeton (il ne s'affiche pas), puis Entrée :"}
            <CommandLine key={commands.read} text={commands.read} />
            {commands.readOld !== undefined && (
              <>
                <span className="mm-token-note">
                  Windows PowerShell 5.1, celui que Windows installe, n'a pas <code>-MaskInput</code> :
                  prends plutôt celle-ci.
                </span>
                <CommandLine key={commands.readOld} text={commands.readOld} />
              </>
            )}
            {shell === "cmd" && (
              <span className="mm-token-note">
                cmd n'a pas de saisie masquée : le jeton s'affiche pendant que tu le tapes. Si
                quelqu'un peut voir ton écran, prends PowerShell.
              </span>
            )}
          </li>
          <li>
            Puis branche Mementum, dans le même shell :
            <CommandLine key={commands.add} text={commands.add} />
          </li>
        </ol>
      </section>
    </>
  );
}

/** Le jeton, à copier : montré ici, et nulle part ailleurs. */
function TokenField({ token }: { token: string }) {
  const fieldRef = useRef<HTMLInputElement>(null);
  const [copied, setCopied] = useState(false);
  const [copyError, setCopyError] = useState<string | null>(null);

  const copy = async () => {
    setCopyError(null);
    try {
      // Absent hors d'un contexte sécurisé (instance servie en `http` sur le réseau local).
      await navigator.clipboard.writeText(token);
      setCopied(true);
    } catch {
      fieldRef.current?.focus();
      fieldRef.current?.select();
      setCopyError("Le navigateur a refusé la copie : le jeton est sélectionné, copie-le avec Ctrl+C.");
    }
  };

  return (
    <div className="mm-invite-created">
      <p className="mm-invite-warning">
        <WarningIcon />
        <strong>Ce jeton ne sera plus affiché : copie-le maintenant.</strong>
      </p>
      <div className="mm-settings-row mm-token-row">
        <input
          ref={fieldRef}
          className="mm-input mm-token-text"
          aria-label="Jeton d'API"
          readOnly
          spellCheck={false}
          autoComplete="off"
          value={token}
          onFocus={(e) => e.currentTarget.select()}
        />
        <button
          type="button"
          className="mm-button mm-button-primary mm-button-icon"
          data-copy-token=""
          onClick={() => void copy()}
        >
          {copied ? <CheckIcon /> : <LinkIcon />}
          {copied ? "Jeton copié" : "Copier le jeton"}
        </button>
      </div>
      <p className="mm-visually-hidden" role="status">
        {copied ? "Jeton copié dans le presse-papiers." : ""}
      </p>
      {copyError && <p className="mm-invite-copy-error">{copyError}</p>}
    </div>
  );
}

/**
 * Une ligne de commande, et de quoi la copier seule. Rendue sous la clé de son texte : la ligne d'un
 * autre shell repart de « Copier », jamais de « Copiée », qui dirait copiée une ligne qui ne l'est pas.
 */
function CommandLine({ text }: { text: string }) {
  const codeRef = useRef<HTMLElement>(null);
  const [copied, setCopied] = useState(false);
  const [copyError, setCopyError] = useState(false);

  const copy = async () => {
    setCopyError(false);
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
    } catch {
      // La ligne sélectionnée, pour Ctrl+C.
      const code = codeRef.current;
      const selection = window.getSelection();
      if (code && selection) {
        const range = document.createRange();
        range.selectNodeContents(code);
        selection.removeAllRanges();
        selection.addRange(range);
      }
      setCopyError(true);
    }
  };

  return (
    <span className="mm-token-line">
      <code ref={codeRef} className="mm-token-command">
        {text}
      </code>
      <button
        type="button"
        className="mm-button mm-button-small"
        onClick={() => void copy()}
        aria-label={copied ? "Ligne copiée" : "Copier la ligne"}
      >
        {copied ? "Copiée" : "Copier"}
      </button>
      {copyError && (
        <span className="mm-invite-copy-error">
          Le navigateur a refusé la copie : la ligne est sélectionnée, copie-la avec Ctrl+C.
        </span>
      )}
    </span>
  );
}
