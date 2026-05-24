import type { Extension } from "@codemirror/state";
import { StreamLanguage, type StringStream } from "@codemirror/language";

const shellLikeDotFileNames = new Set([
    ".env",
    ".gitignore",
    ".dockerignore",
    ".npmignore",
    ".eslintignore",
    ".prettierignore",
    ".ignore",
]);

function basename(filename: string): string {
    return filename.split(/[\\/]/).pop()?.toLowerCase() ?? filename.toLowerCase();
}

function extension(filename: string): string | undefined {
    return basename(filename).split(".").pop()?.toLowerCase();
}

function isShellLikeDotFile(filename: string): boolean {
    const name = basename(filename);
    return shellLikeDotFileNames.has(name) || name.startsWith(".env.");
}

const shellLikeDotFileLanguage = StreamLanguage.define({
    name: "shell-like-dotfile",
    token(stream: StringStream) {
        if (stream.eatSpace()) {
            return null;
        }

        if (stream.sol() && stream.match(/#.*/)) {
            return "lineComment";
        }

        if (stream.sol() && stream.match("!")) {
            return "keyword";
        }

        if (stream.match(/[*?\[\]{}]+/)) {
            return "operator";
        }

        if (stream.match(/\/+/)) {
            return "separator";
        }

        stream.eatWhile(/[^#*!?\[\]{}\/\s]/);
        if (stream.current().length === 0) {
            stream.next();
        }
        return "string";
    },
});

export async function loadLanguageExtension(filename?: string): Promise<Extension[]> {
    if (!filename) return [];

    if (isShellLikeDotFile(filename)) {
        return [shellLikeDotFileLanguage];
    }

    const ext = extension(filename);

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

        case "php":
        case "phtml":
        case "php3":
        case "php4":
        case "php5":
        case "phps": {
            const { php } = await import("@codemirror/lang-php");
            return [php()];
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

    if (isShellLikeDotFile(filename)) {
        return "Shell";
    }

    const ext = extension(filename);

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
        case "php":
        case "phtml":
        case "php3":
        case "php4":
        case "php5":
        case "phps":
            return "PHP";
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
