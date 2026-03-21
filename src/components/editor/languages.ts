import type { Extension } from "@codemirror/state";

export async function loadLanguageExtension(filename?: string): Promise<Extension[]> {
    if (!filename) return [];

    const ext = filename.split(".").pop()?.toLowerCase();

    switch (ext) {
        case "rs": {
            const { rust } = await import("@codemirror/lang-rust");
            return [rust()];
        }

        case "js":
        case "mjs":
        case "cjs": {
            const { javascript } = await import("@codemirror/lang-javascript");
            return [javascript()];
        }
        case "jsx": {
            const { javascript } = await import("@codemirror/lang-javascript");
            return [javascript({ jsx: true })];
        }
        case "ts":
        case "mts":
        case "cts": {
            const { javascript } = await import("@codemirror/lang-javascript");
            return [javascript({ typescript: true })];
        }
        case "tsx": {
            const { javascript } = await import("@codemirror/lang-javascript");
            return [javascript({ jsx: true, typescript: true })];
        }

        case "py":
        case "pyw":
        case "pyi": {
            const { python } = await import("@codemirror/lang-python");
            return [python()];
        }

        case "json":
        case "jsonc":
        case "json5": {
            const { json } = await import("@codemirror/lang-json");
            return [json()];
        }

        case "css":
        case "scss":
        case "less": {
            const { css } = await import("@codemirror/lang-css");
            return [css()];
        }

        case "html":
        case "htm":
        case "xhtml":
        case "astro":
        case "vue":
        case "svelte": {
            const { html } = await import("@codemirror/lang-html");
            return [html()];
        }

        case "md":
        case "markdown":
        case "mdx": {
            const { markdown } = await import("@codemirror/lang-markdown");
            return [markdown()];
        }

        case "yaml":
        case "yml": {
            const { yaml } = await import("@codemirror/lang-yaml");
            return [yaml()];
        }

        case "c":
        case "h":
        case "cpp":
        case "cc":
        case "cxx":
        case "hpp":
        case "hxx":
        case "hh": {
            const { cpp } = await import("@codemirror/lang-cpp");
            return [cpp()];
        }

        case "go": {
            const { go } = await import("@codemirror/lang-go");
            return [go()];
        }

        case "toml":
        case "sh":
        case "bash":
        case "zsh":
        default:
            return [];
    }
}

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
