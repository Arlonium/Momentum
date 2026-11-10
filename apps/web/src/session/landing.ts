// Où arrive un compte tout juste créé par une invitation sur une page : sur cette page.
//
// Avec une session, accepter une invitation rend l'espace et la page où aller (`Accepted`,
// `InvitePage`). L'inscription, elle, accepte l'invitation dans la foulée et ne rend que le
// compte (`Me`) : l'id de la page n'y est pas, et le lien ne le porte pas non plus. Mais le
// compte neuf n'a que l'espace où on l'invite, et n'y voit que la page de l'invitation et ses
// sous-pages : c'est la seule qui soit à la racine de ce qu'il voit.
import type { Me } from "@mementum/protocol";
import { getTree } from "../api";

/**
 * La page où mène l'invitation que `me` vient d'accepter en s'inscrivant. `null` : on ne le
 * sait pas avec certitude (l'arbre ne se lit pas, ou il n'a pas une seule page à sa racine) ;
 * l'accueil de l'espace la montre alors dans « Partagé ».
 */
export async function invitedPage(me: Me): Promise<string | null> {
  if (me.spaces.length !== 1) return null;
  try {
    const { nodes } = await getTree(me.spaces[0].id);
    const tops = nodes.filter((node) => node.parentId === null);
    return tops.length === 1 ? tops[0].id : null;
  } catch {
    // L'inscription est faite : elle n'échoue pas pour cela.
    return null;
  }
}
