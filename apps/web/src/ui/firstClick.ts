// Le second clic d'un double clic n'est pas une seconde demande. Un bouton qui crée quelque
// chose à chaque clic (une page) en créerait deux : la garde « création en cours » ne vaut
// que tant que le serveur n'a pas répondu, et il répond bien avant le second clic d'une main.
import type { MouseEvent } from "react";

/**
 * Un `onClick` qui n'appelle `act` que pour un clic simple ou un appui au clavier : `detail`
 * vaut 1 pour un clic, 0 pour Entrée ou Espace, 2 et plus pour les clics suivants d'un double
 * ou d'un triple clic, qui ne font rien.
 */
export function firstClick(act: () => void): (event: Pick<MouseEvent, "detail">) => void {
  return (event) => {
    if (event.detail <= 1) act();
  };
}
