// Un jeton d'API tout juste créé (lot 5, Task C1) : son texte, à copier (le serveur n'en garde que le
// hachage, et ne le rendra plus), et de quoi brancher Claude Code.
//
// **La commande ne porte jamais le jeton** (R1252, R1576) : collé dans une ligne de commande, il
// resterait dans l'historique du shell. Il passe par la variable `MEMENTUM_TOKEN`, que la personne
// pose elle-même, et les gestes viennent dans l'ordre où le presse-papiers les sert (R1462) :
// 1. copier et lancer la ligne qui lit la variable (elle attend le jeton, avec une invite, sans
//    l'afficher sauf sous cmd) ;
// 2. copier le jeton, le coller, Entrée ;
// 3. copier et lancer la ligne qui branche Mementum, à l'adresse que le navigateur connaît (`origin`),
//    l'en-tête nommant la variable.
// Copier le jeton d'abord, puis une commande, ferait coller la commande à la place du jeton. Les
// guillemets sont ceux que chaque shell demande pour que `${MEMENTUM_TOKEN}` arrive tel quel à Claude
// Code : apostrophes sous bash, zsh et PowerShell (entre guillemets, bash y mettrait le jeton, et
// PowerShell sa propre variable, vide) ; guillemets sous cmd (une apostrophe n'y groupe rien). Une
// adresse qui porte un caractère que ce shell lirait n'est pas mise dans sa commande (R1620).
//
// Le shell proposé d'abord suit le système (Windows : PowerShell ; ailleurs : bash ou zsh, R1464).
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

/** Le caractère qui ouvre et ferme une chaîne littérale dans chaque shell. */
const QUOTES: Record<Shell, string> = { bash: "'", powershell: "'", cmd: '"' };

export type Commands = {
  /** Lit le jeton dans la variable, avec une invite, sans l'afficher (sauf sous cmd). */
  read: string;
  /** La même chose pour Windows PowerShell 5.1, qui n'a pas `-MaskInput`. */
  readOld?: string;
  /** Branche Mementum à Claude Code, l'en-tête nommant la variable. */
  add: string;
};

/**
 * Le caractère de `origin` que le shell `shell` lirait entre ses guillemets : le sien, `%`, `!`, un
 * accent grave, une espace ou un caractère de contrôle ; `null` : aucun. Une adresse d'instance n'en
 * porte pas d'ordinaire (`location.origin`) : c'est une garde.
 */
export function unsafeIn(shell: Shell, origin: string): string | null {
  for (const char of origin) {
    if (char === QUOTES[shell] || "%!`".includes(char) || char.trim() === "" || char.charCodeAt(0) < 32) {
      return char;
    }
  }
  return null;
}

/** Les lignes du shell `shell`, pour l'instance servie à `origin` ; `null` si l'adresse ne s'y écrit pas. */
export function commandsFor(shell: Shell, origin: string): Commands | null {
  if (unsafeIn(shell, origin) !== null) return null;
  const quote = QUOTES[shell];
  const add = `claude mcp add --transport http mementum ${quote}${origin}/mcp${quote} --header ${quote}Authorization: Bearer \${${VARIABLE}}${quote}`;
  switch (shell) {
    case "bash":
      return { read: `printf 'Jeton : '; read -rs ${VARIABLE}; echo; export ${VARIABLE}`, add };
    case "powershell":
      return {
        read: `$env:${VARIABLE} = Read-Host -Prompt 'Jeton' -MaskInput`,
        readOld: `$env:${VARIABLE} = [Net.NetworkCredential]::new('', (Read-Host -Prompt 'Jeton' -AsSecureString)).Password`,
        add,
      };
    case "cmd":
      return { read: `set /p ${VARIABLE}=Jeton : `, add };
  }
}

/** Le shell proposé d'abord : celui du système, d'après le navigateur. */
export function defaultShell(): Shell {
  const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
  const platform = nav.userAgentData?.platform || navigator.platform || "";
  return /^win/i.test(platform) ? "powershell" : "bash";
}

type Props = {
  token: string;
  /** L'adresse de l'instance, telle que le navigateur la connaît (`location.origin`). */
  origin: string;
};

export function TokenCreated({ token, origin }: Props) {
  const shellName = useId();
  const [shell, setShell] = useState<Shell>(defaultShell);
  const commands = commandsFor(shell, origin);
  const label = SHELLS.find((item) => item.id === shell)?.label ?? shell;
  return (
    <>
      <div className="mm-invite-created">
        <p className="mm-invite-warning">
          <WarningIcon />
          <strong>Ce jeton ne sera plus affiché.</strong>
        </p>
        <p className="mm-token-keep">
          Garde-le dans un gestionnaire de mots de passe : il faudra le recoller une fois par shell, avant
          d'y lancer Claude Code.
        </p>
      </div>
      <section className="mm-token-connect" aria-labelledby={`${shellName}-title`}>
        <h3 id={`${shellName}-title`} className="mm-token-connect-title">
          Brancher Claude Code
        </h3>
        <p className="mm-token-hint">
          Le jeton ne va pas dans la commande, où l'historique du shell le garderait : il passe par la
          variable <code>{VARIABLE}</code>, posée dans le shell qui lance Claude Code.
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
        {commands === null ? (
          <>
            <p className="mm-token-note" role="status">
              L'adresse de cette instance porte un caractère que {label} lirait («&nbsp;
              {unsafeIn(shell, origin)}&nbsp;») : la commande n'est pas montrée pour ce shell. Écris-la à la
              main, ou choisis-en un autre.
            </p>
            <TokenField token={token} first />
          </>
        ) : (
          <ol className="mm-token-steps">
            <li>
              Dans le shell qui lancera Claude Code, lance cette ligne : elle attend le jeton
              {shell === "cmd" ? "." : ", sans l'afficher."}
              <CommandLine key={commands.read} text={commands.read} name="Copier la ligne 1" first />
              {commands.readOld !== undefined && (
                <>
                  <span className="mm-token-note">
                    Windows PowerShell 5.1, celui que Windows installe, n'a pas <code>-MaskInput</code> :
                    prends plutôt celle-ci.
                  </span>
                  <CommandLine
                    key={commands.readOld}
                    text={commands.readOld}
                    name="Copier la ligne 1, pour Windows PowerShell 5.1"
                  />
                </>
              )}
            </li>
            <li>
              Copie le jeton, colle-le dans le shell, puis Entrée.
              <TokenField token={token} />
            </li>
            <li>
              Puis branche Mementum, dans le même shell :
              <CommandLine key={commands.add} text={commands.add} name="Copier la ligne 2" />
              <span className="mm-token-note">
                Sans <code>--scope</code>, Claude Code ne branche Mementum que dans le dossier où tu lances
                la commande ; avec <code>--scope user</code>, il le branche partout.
              </span>
            </li>
          </ol>
        )}
        <p className="mm-token-note">
          Le presse-papiers peut garder le jeton (son historique, Win+V sous Windows).
          {shell === "cmd" && " Sous cmd, il reste aussi affiché dans la fenêtre : ferme-la une fois Claude Code branché."}
        </p>
      </section>
    </>
  );
}

/**
 * Le jeton, à copier : montré ici, et nulle part ailleurs. `first` : le premier geste de la fenêtre,
 * quand aucune commande ne le précède.
 */
function TokenField({ token, first = false }: { token: string; first?: boolean }) {
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
    <span className="mm-token-field">
      <span className="mm-settings-row mm-token-row">
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
          data-first-gesture={first ? "" : undefined}
          onClick={() => void copy()}
        >
          {copied ? <CheckIcon /> : <LinkIcon />}
          {copied ? "Jeton copié" : "Copier le jeton"}
        </button>
      </span>
      <span className="mm-visually-hidden" role="status">
        {copied ? "Jeton copié dans le presse-papiers." : ""}
      </span>
      {copyError && <span className="mm-invite-copy-error">{copyError}</span>}
    </span>
  );
}

/**
 * Une ligne de commande, et de quoi la copier seule (`name` : le nom du bouton, un par ligne). Rendue
 * sous la clé de son texte : la ligne d'un autre shell repart de « Copier », jamais de « Copiée », qui
 * dirait copiée une ligne qui ne l'est pas. `first` : le premier geste de la fenêtre.
 */
function CommandLine({ text, name, first = false }: { text: string; name: string; first?: boolean }) {
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
        data-first-gesture={first ? "" : undefined}
        onClick={() => void copy()}
        aria-label={copied ? `${name} : copiée` : name}
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
