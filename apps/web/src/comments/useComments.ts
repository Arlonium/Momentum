// Les fils de commentaires d'une page (R134), tels que le serveur les rend
// (`GET /api/v1/pages/{id}/comments`), et ce qu'on y écrit d'ici. Rien n'en est écrit dans le
// document de la page : quelqu'un qui peut commenter sans pouvoir modifier commente.
//
// Quand les relire. Le serveur envoie la trame `{"type":"comments"}` sur la connexion de synchro
// de la page à chaque écriture validée, de qui que ce soit (R136) : les fils sont relus par
// l'API. Une trame peut se perdre pendant une coupure : ils le sont aussi quand la connexion
// revient, et à la première synchro de chaque session (une page qui passe en lecture seule en
// ouvre une neuve). Et après chacune des écritures d'ici, dont le résultat s'affiche aussitôt.
//
// Rien sur le chemin de l'ouverture de la page : la première lecture attend la première synchro
// de l'éditeur, et ne la retarde donc pas (budgets du §5.10).
//
// Les relectures sont groupées (`rereads.ts`, comme celles de l'historique) : une rafale de
// trames n'en fait qu'une, `COMMENTS_DEBOUNCE_MS` après la dernière et au plus tard
// `COMMENTS_MAX_WAIT_MS` après la première. Une relecture demandée pendant une lecture ne
// l'annule pas : elle est faite une fois, à sa fin. Ce qui est propre aux commentaires : une
// lecture partie avant une écriture d'ici qui a abouti peut dater d'avant elle ; sa réponse est
// laissée, et la lecture refaite. Une lecture qui échoue garde ce qui était affiché, dit
// pourquoi, et ne se refait pas d'elle-même : la prochaine trame, le retour de la connexion ou
// « Réessayer » la refont. Une lecture qui trouve la session perdue (`401`) est refaite au retour
// du compte (`afterLogin`), même si la connexion de synchro est restée ouverte. Tout est lié à la
// page : rien ne passe d'une page à l'autre, et plus rien n'est appelé après le démontage, ni au
// retour d'un compte attendu avant lui.
import type { Comment, Thread } from "@mementum/protocol";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ApiError,
  deleteComment,
  deleteThread,
  editComment,
  getComments,
  isSessionLost,
  openThread,
  replyToThread,
  resolveThread,
  setReaction,
} from "../api";
import type { CollabSession } from "../editor/collab";
import {
  REREAD_DEBOUNCE_MS,
  REREAD_MAX_WAIT_MS,
  scheduleRereads,
  SESSION_LOST,
  waitForAccount,
} from "../rereads";
import { messageOf } from "../session/forms";

/** Plusieurs trames qui se suivent ne font qu'une relecture, cette durée après la dernière. */
export const COMMENTS_DEBOUNCE_MS = REREAD_DEBOUNCE_MS;
/** Une relecture part au plus tard cette durée après la première trame qu'elle doit traiter. */
export const COMMENTS_MAX_WAIT_MS = REREAD_MAX_WAIT_MS;

/** `loading` : rien n'est encore lu ; `failed` : la dernière lecture a échoué. */
export type CommentsState = "loading" | "ready" | "failed";

export type Comments = {
  /** Les fils de la page, résolus compris, du plus ancien au plus récent ; `null` : pas encore lus. */
  threads: Thread[] | null;
  state: CommentsState;
  /** Le message de la dernière lecture si elle a échoué, tel que le serveur le dit. */
  error: string | null;
  /** Ouvre un fil sur un bloc, ou sur la page (`null`), avec son premier message. */
  open(blockId: string | null, body: string): Promise<Thread>;
  reply(threadId: string, body: string): Promise<Comment>;
  edit(threadId: string, commentId: string, body: string): Promise<Comment>;
  remove(threadId: string, commentId: string): Promise<void>;
  /**
   * Supprime tout le fil, avec les messages de tous (accès complet, R352). Un fil déjà parti
   * (`404`) n'est pas un refus : c'est ce que la personne voulait.
   */
  removeThread(threadId: string): Promise<void>;
  resolve(threadId: string, resolved: boolean): Promise<Thread>;
  /** Pose (`on`) ou retire la réaction de la personne. */
  react(threadId: string, commentId: string, emoji: string, on: boolean): Promise<Comment>;
  /** Relit les fils (« Réessayer »). */
  reload(): void;
};

/** Ce que le hook lit de la session de synchro de la page. */
export type CommentsSession = Pick<CollabSession, "synced" | "status" | "subscribe" | "onPageEvent">;

type Options = {
  /**
   * Le serveur a refusé une écriture d'ici (`403`, `404`, `409`) : le droit de la personne,
   * ou la page, ont pu changer depuis que l'écran les a lus. L'erreur est aussi rendue à
   * l'appelant, qui l'affiche telle quelle.
   */
  onRefused?: (error: ApiError) => void;
  /**
   * Tenue quand le compte revient, après une session perdue (`useAfterLogin`) : les fils sont
   * alors relus. Sans elle, une lecture refusée par `401` attend la trame suivante.
   */
  afterLogin?: () => Promise<void>;
};

type Loaded = {
  pageId: string;
  threads: Thread[] | null;
  state: CommentsState;
  error: string | null;
};

/** Un refus qui peut venir d'un droit, ou d'une page, qui a changé. */
function isRefusal(error: unknown): error is ApiError {
  return error instanceof ApiError && [403, 404, 409].includes(error.status);
}

function replaceComment(threads: Thread[], threadId: string, comment: Comment): Thread[] {
  return threads.map((t) =>
    t.id === threadId
      ? { ...t, comments: t.comments.map((c) => (c.id === comment.id ? comment : c)) }
      : t,
  );
}

export function useComments(
  pageId: string,
  session: CommentsSession | null,
  options: Options = {},
): Comments {
  const [loaded, setLoaded] = useState<Loaded>(() => ({
    pageId,
    threads: null,
    state: "loading",
    error: null,
  }));
  // Les rappels du dernier rendu : en changer ne relance rien.
  const callbacks = useRef(options);
  callbacks.current = options;
  /** Demande une relecture : tout de suite (`true`), ou groupée avec les trames qui suivent. */
  const request = useRef<(immediate: boolean) => void>(() => {});
  /** Les écritures d'ici qui ont abouti : une lecture partie avant l'une d'elles peut dater d'avant. */
  const writes = useRef(0);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    // Groupées, jamais annulées en route, rien après l'arrêt (`rereads.ts`).
    const rereads = scheduleRereads(async () => {
      const before = writes.current;
      try {
        const threads = await getComments(pageId);
        if (rereads.stopped) return;
        // Une écriture d'ici a abouti pendant la lecture : la réponse peut dater d'avant elle.
        if (writes.current !== before) return true;
        setLoaded({ pageId, threads, state: "ready", error: null });
      } catch (e) {
        if (rereads.stopped) return;
        // Session perdue : la connexion est demandée par-dessus la page (`api.ts`), et les fils
        // sont relus au retour du compte.
        if (isSessionLost(e)) return SESSION_LOST;
        setLoaded((current) => ({
          pageId,
          threads: current.pageId === pageId ? current.threads : null,
          state: "failed",
          error: messageOf(e),
        }));
      }
    }, waitForAccount(callbacks));
    request.current = rereads.request;

    return () => {
      rereads.stop();
      request.current = () => {};
    };
  }, [pageId]);

  // La session de synchro : ses trames, sa première synchro, ses retours.
  useEffect(() => {
    if (session === null) return;
    let synced = session.synced;
    let status = session.status;
    if (synced) request.current(true);
    const offFrames = session.onPageEvent((event) => {
      if (event === "comments") request.current(false);
    });
    const offStatus = session.subscribe(() => {
      const wasSynced = synced;
      const was = status;
      synced = session.synced;
      status = session.status;
      // La première synchro de cette session : la première lecture, ou celle d'une session
      // neuve, qui a pu manquer une trame pendant que l'ancienne se fermait.
      if (!wasSynced && synced) request.current(true);
      // La connexion revient : une trame a pu partir pendant la coupure.
      else if (synced && status === "connected" && was !== "connected") request.current(false);
    });
    return () => {
      offFrames();
      offStatus();
    };
  }, [session, pageId]);

  /**
   * Envoie une écriture ; si elle aboutit, son résultat s'affiche aussitôt (`apply`, qui ne
   * double rien : une relecture a pu le rendre avant). Dans tous les cas, les fils sont relus.
   */
  const write = useCallback(
    async <T>(send: () => Promise<T>, apply: (threads: Thread[], result: T) => Thread[]) => {
      try {
        const result = await send();
        writes.current += 1;
        setLoaded((current) =>
          current.pageId === pageId && current.threads !== null
            ? { ...current, threads: apply(current.threads, result) }
            : current,
        );
        return result;
      } catch (e) {
        if (isRefusal(e) && mounted.current) callbacks.current.onRefused?.(e);
        throw e;
      } finally {
        request.current(true);
      }
    },
    [pageId],
  );

  const open = useCallback(
    (blockId: string | null, body: string) =>
      write(
        () => openThread(pageId, { blockId, body }),
        (threads, created) =>
          threads.some((t) => t.id === created.id) ? threads : [...threads, created],
      ),
    [pageId, write],
  );
  const reply = useCallback(
    (threadId: string, body: string) =>
      write(
        () => replyToThread(pageId, threadId, { body }),
        (threads, comment) =>
          threads.map((t) =>
            t.id === threadId && !t.comments.some((c) => c.id === comment.id)
              ? { ...t, comments: [...t.comments, comment] }
              : t,
          ),
      ),
    [pageId, write],
  );
  const edit = useCallback(
    (threadId: string, commentId: string, body: string) =>
      write(
        () => editComment(pageId, threadId, commentId, { body }),
        (threads, comment) => replaceComment(threads, threadId, comment),
      ),
    [pageId, write],
  );
  const remove = useCallback(
    (threadId: string, commentId: string) =>
      write(
        () => deleteComment(pageId, threadId, commentId),
        (threads) =>
          threads.flatMap((t) => {
            if (t.id !== threadId) return [t];
            const comments = t.comments.filter((c) => c.id !== commentId);
            // Le fil part avec son dernier message, comme sur le serveur.
            return comments.length === 0 ? [] : [{ ...t, comments }];
          }),
      ),
    [pageId, write],
  );
  const removeThread = useCallback(
    (threadId: string) =>
      write(
        async () => {
          try {
            await deleteThread(pageId, threadId);
          } catch (e) {
            // Supprimé ailleurs entre-temps : il est parti, comme voulu. Rien à dire, ni la page à
            // relire ; la liste l'est, comme après toute écriture (R362).
            if (e instanceof ApiError && e.status === 404) return;
            throw e;
          }
        },
        (threads) => threads.filter((t) => t.id !== threadId),
      ),
    [pageId, write],
  );
  const resolve = useCallback(
    (threadId: string, resolved: boolean) =>
      write(
        () => resolveThread(pageId, threadId, { resolved }),
        (threads, changed) => threads.map((t) => (t.id === changed.id ? changed : t)),
      ),
    [pageId, write],
  );
  const react = useCallback(
    (threadId: string, commentId: string, emoji: string, on: boolean) =>
      write(
        () => setReaction(pageId, threadId, commentId, emoji, on),
        (threads, comment) => replaceComment(threads, threadId, comment),
      ),
    [pageId, write],
  );
  const reload = useCallback(() => request.current(true), []);

  const current = loaded.pageId === pageId ? loaded : null;
  const threads = current?.threads ?? null;
  const state = current?.state ?? "loading";
  const error = current?.error ?? null;
  return useMemo(
    () => ({ threads, state, error, open, reply, edit, remove, removeThread, resolve, react, reload }),
    [threads, state, error, open, reply, edit, remove, removeThread, resolve, react, reload],
  );
}
