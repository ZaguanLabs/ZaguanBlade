import { EditorView } from "@codemirror/view";
import { Extension } from "@codemirror/state";

// Smooth cursor animation theme
export const smoothCursor: Extension = EditorView.theme({
    ".cm-cursor": {
        transition: "left 50ms ease-out, top 50ms ease-out",
    },
    
    // Cursor blink animation
    "@keyframes cm-cursor-blink": {
        "0%, 100%": { opacity: "1" },
        "50%": { opacity: "0" },
    },
    
    "&.cm-focused .cm-cursor": {
        animation: "cm-cursor-blink 1s ease-in-out infinite",
    },
});
