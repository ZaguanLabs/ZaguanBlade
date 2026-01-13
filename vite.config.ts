import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// https://vitejs.dev/config/
export default defineConfig(async () => ({
    plugins: [react()],

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
        alias: {
            "@": "/src",
        },
    },
    build: {
        chunkSizeWarningLimit: 800,
        rollupOptions: {
            output: {
                manualChunks: (id) => {
                    if (id.includes('node_modules')) {
                        // Syntax Highlighting (Large & isolated)
                        if (id.includes('react-syntax-highlighter') || id.includes('refractor')) {
                            return 'vendor-syntax';
                        }
                        // Markdown & Unified Ecosystem (Large & complex)
                        if (id.includes('react-markdown') || id.includes('remark') || id.includes('unified') || id.includes('unist') || id.includes('vfile') || id.includes('micromark') || id.includes('mdast')) {
                            return 'vendor-markdown';
                        }
                        // CodeMirror (Editor)
                        if (id.includes('@codemirror') || id.includes('codemirror')) {
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

                        // Core Framework & Common Libs
                        // We group React, Router, i18n, and Icons together to avoid circular dependencies
                        if (
                            id.includes('/react/') ||
                            id.includes('/react-dom/') ||
                            id.includes('/react-router') ||
                            id.includes('@remix-run') ||
                            id.includes('/scheduler/') ||
                            id.includes('i18next') ||
                            id.includes('lucide-react')
                        ) {
                            return 'vendor-react';
                        }

                        // Apps specific Tauri packages can stay in default vendor or be chunked if safe, 
                        // but to fix black screen we avoid aggressive splitting here.
                    }
                }
            }
        }
    }
}));
