import { Extension } from "@codemirror/state";
import { rust } from "@codemirror/lang-rust";
import { javascript } from "@codemirror/lang-javascript";
import { python } from "@codemirror/lang-python";
import { json } from "@codemirror/lang-json";
import { css } from "@codemirror/lang-css";
import { html } from "@codemirror/lang-html";
import { markdown } from "@codemirror/lang-markdown";
import { yaml } from "@codemirror/lang-yaml";
import { cpp } from "@codemirror/lang-cpp";
import { go } from "@codemirror/lang-go";

// Map file extensions to language support
export function getLanguageExtension(filename?: string): Extension[] {
    if (!filename) return [];
    
    const ext = filename.split(".").pop()?.toLowerCase();
    
    switch (ext) {
        // Rust
        case "rs":
            return [rust()];
        
        // JavaScript/TypeScript
        case "js":
        case "mjs":
        case "cjs":
            return [javascript()];
        case "jsx":
            return [javascript({ jsx: true })];
        case "ts":
        case "mts":
        case "cts":
            return [javascript({ typescript: true })];
        case "tsx":
            return [javascript({ jsx: true, typescript: true })];
        
        // Python
        case "py":
        case "pyw":
        case "pyi":
            return [python()];
        
        // JSON
        case "json":
        case "jsonc":
        case "json5":
            return [json()];
        
        // CSS/SCSS/LESS
        case "css":
        case "scss":
        case "less":
            return [css()];
        
        // HTML (including Astro, Vue, Svelte templates)
        case "html":
        case "htm":
        case "xhtml":
        case "astro":
        case "vue":
        case "svelte":
            return [html()];
        
        // Markdown
        case "md":
        case "markdown":
        case "mdx":
            return [markdown()];
        
        // YAML
        case "yaml":
        case "yml":
            return [yaml()];
        
        // C/C++
        case "c":
        case "h":
            return [cpp()];
        case "cpp":
        case "cc":
        case "cxx":
        case "hpp":
        case "hxx":
        case "hh":
            return [cpp()];
        
        // Go
        case "go":
            return [go()];
        
        // Config files (treat as JSON or YAML)
        case "toml":
            return []; // No TOML support yet, fallback to plain text
        
        // Shell scripts
        case "sh":
        case "bash":
        case "zsh":
            return []; // No shell support yet
        
        // Default: no language support
        default:
            return [];
    }
}

// Get language name for display
export function getLanguageName(filename?: string): string {
    if (!filename) return "Plain Text";
    
    const ext = filename.split(".").pop()?.toLowerCase();
    
    switch (ext) {
        case "rs":
            return "Rust";
        case "js":
        case "mjs":
        case "cjs":
            return "JavaScript";
        case "jsx":
            return "JavaScript (JSX)";
        case "ts":
        case "mts":
        case "cts":
            return "TypeScript";
        case "tsx":
            return "TypeScript (TSX)";
        case "py":
        case "pyw":
        case "pyi":
            return "Python";
        case "json":
        case "jsonc":
        case "json5":
            return "JSON";
        case "css":
            return "CSS";
        case "scss":
            return "SCSS";
        case "less":
            return "LESS";
        case "html":
        case "htm":
        case "xhtml":
            return "HTML";
        case "astro":
            return "Astro";
        case "vue":
            return "Vue";
        case "svelte":
            return "Svelte";
        case "md":
        case "markdown":
            return "Markdown";
        case "mdx":
            return "MDX";
        case "yaml":
        case "yml":
            return "YAML";
        case "c":
        case "h":
            return "C";
        case "cpp":
        case "cc":
        case "cxx":
        case "hpp":
        case "hxx":
        case "hh":
            return "C++";
        case "go":
            return "Go";
        case "toml":
            return "TOML";
        case "sh":
        case "bash":
            return "Bash";
        case "zsh":
            return "Zsh";
        default:
            return "Plain Text";
    }
}
