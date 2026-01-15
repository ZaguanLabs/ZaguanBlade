import { EditorView } from "@codemirror/view";
import { Extension } from "@codemirror/state";

// Allow scrolling past the end of the document
// This gives a cleaner look and makes it easier to edit the last lines
export const scrollPastEnd: Extension = EditorView.theme({
    ".cm-content": {
        paddingBottom: "50vh",
    },
});
