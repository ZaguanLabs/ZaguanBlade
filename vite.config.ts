import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath, URL } from "node:url";

const isNodeModulePackage = (id: string, packageName: string): boolean => {
    const normalizedId = id.replace(/\\/g, '/');
    return normalizedId.includes(`/node_modules/${packageName}/`)
        || normalizedId.endsWith(`/node_modules/${packageName}`)
        || normalizedId.includes(`/node_modules/.pnpm/`) && normalizedId.includes(`/node_modules/${packageName}/`);
};

// https://vitejs.dev/config/
export default defineConfig(async () => ({
    plugins: [
        react(),
        ...(process.env.BUNDLE_ANALYZE === '1'
            ? [(await import("rollup-plugin-visualizer")).visualizer({
                filename: 'benchmarks/bundle-stats.html',
                template: 'treemap',
                gzipSize: true,
                brotliSize: false,
                emitFile: false,
            })]
            : []),
    ],

    // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
    //
    // 1. prevent vite from obscuring rust errors
    clearScreen: false,
    // 2. tauri expects a fixed port, fail if that port is not available
    server: {
        port: 1420,
        strictPort: true,
        watch: {
            // 3. tell vite to ignore watching `src-tauri`
            ignored: ["**/src-tauri/**"],
        },
    },
    resolve: {
        preserveSymlinks: true,
        dedupe: [
            "react",
            "react-dom",
            "@codemirror/state",
            "@codemirror/view",
            "@codemirror/language",
            "@lezer/common",
            "@lezer/highlight",
            "@lezer/lr",
        ],
        alias: [
            { find: /^react$/, replacement: fileURLToPath(new URL("./node_modules/react/index.js", import.meta.url)) },
            { find: /^react\/jsx-runtime$/, replacement: fileURLToPath(new URL("./node_modules/react/jsx-runtime.js", import.meta.url)) },
            { find: /^react\/jsx-dev-runtime$/, replacement: fileURLToPath(new URL("./node_modules/react/jsx-dev-runtime.js", import.meta.url)) },
            { find: /^react-dom$/, replacement: fileURLToPath(new URL("./node_modules/react-dom/index.js", import.meta.url)) },
            { find: /^react-dom\/client$/, replacement: fileURLToPath(new URL("./node_modules/react-dom/client.js", import.meta.url)) },
            { find: "@", replacement: "/src" },
        ],
    },
    build: {
        chunkSizeWarningLimit: 1200,
        minify: 'terser' as const,
        terserOptions: {
            compress: {
                drop_console: false,
                drop_debugger: true,
                pure_funcs: ['console.debug']
            }
        },
        cssCodeSplit: true,
        rollupOptions: {
            output: {
                manualChunks: (id) => {
                    if (id.includes('node_modules')) {
                        if (
                            isNodeModulePackage(id, 'react') ||
                            isNodeModulePackage(id, 'react-dom') ||
                            isNodeModulePackage(id, 'scheduler') ||
                            id.includes('/react/jsx-runtime') ||
                            id.includes('/react/jsx-dev-runtime') ||
                            (id.includes('commonjs-proxy') && id.includes('react'))
                        ) {
                            return 'vendor-react';
                        }
                        // CodeMirror language packages: no manual chunk — each
                        // @codemirror/lang-* stays independently loadable via its
                        // own dynamic import, avoiding the all-languages chunk.
                        // CodeMirror core (state/view/language) is grouped together.
                        if (id.includes('@codemirror/lang-')) {
                            return undefined;
                        }
                        if (id.includes('@codemirror') || id.includes('codemirror') || id.includes('@lezer')) {
                            return 'vendor-codemirror';
                        }
                        // XTerm (Terminal)
                        if (id.includes('@xterm') || id.includes('xterm')) {
                            return 'vendor-xterm';
                        }
                        // Headless Tree (File Explorer)
                        if (id.includes('@headless-tree')) {
                            return 'vendor-tree';
                        }
                        // PDF.js (PDF Viewer) — no manual chunk; let it remain
                        // behind the dynamic import of PdfViewer so it is absent
                        // from the initial preload graph.
                        if (id.includes('pdfjs-dist')) {
                            return undefined;
                        }
                        if (
                            id.includes('/react-router') ||
                            id.includes('@remix-run') ||
                            id.includes('i18next')
                        ) {
                            return 'vendor-react';
                        }
                    }
                }
            }
        }
    }
}));
