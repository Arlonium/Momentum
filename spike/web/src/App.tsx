import { useEffect } from "react";
import "@blocknote/core/fonts/inter.css";
import "@blocknote/mantine/style.css";
import { withCollaboration } from "@blocknote/core/yjs";
import { useCreateBlockNote } from "@blocknote/react";
import { BlockNoteView } from "@blocknote/mantine";
import { schema } from "./schema";
import { createCollab, fixtureName } from "./collab";
import allBlocks from "../../fixtures/blocknote/all-blocks.input.json";

// Créé une seule fois par chargement de page (indépendant du double rendu de React).
const collab = createCollab();

export function App() {
  const editor = useCreateBlockNote(
    withCollaboration({
      schema,
      collaboration: { fragment: collab.fragment, user: collab.user, provider: collab.provider },
    }),
  );

  useEffect(() => {
    const m: any = Object.assign(collab, { editor }); // même objet : tSynced reste à jour
    (window as any).__mementum = m;
    if (fixtureName !== "all-blocks") return;
    const load = () => {
      editor.replaceBlocks(editor.document, allBlocks as any);
      m.fixtureReady = true;
    };
    if (collab.provider.synced) load();
    else collab.provider.once("sync", (synced: boolean) => synced && load());
  }, [editor]);

  return (
    <div style={{ maxWidth: 760, margin: "40px auto" }}>
      <BlockNoteView editor={editor} />
    </div>
  );
}
