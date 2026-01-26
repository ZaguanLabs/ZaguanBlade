import { scrollPastEnd as cmScrollPastEnd } from "@codemirror/view";

// Re-export the official CodeMirror scrollPastEnd extension
// Allows scrolling past the end of the document for better editing experience
export const scrollPastEnd = cmScrollPastEnd();
