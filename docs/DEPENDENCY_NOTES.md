# Dependency Notes

## pdfjs-dist

`pdfjs-dist` is intentionally pinned to `5.7.284`.

Do not update `pdfjs-dist` to v6 without a dedicated PDF viewer migration and manual regression pass across the supported Tauri webviews. The v6 line removes APIs used by the current viewer, including `PDFDocumentProxy.destroy()`, and raises browser/runtime assumptions that are risky for a system-webview desktop app.

Before reconsidering this pin, validate at minimum:

- Windows WebView2, macOS WKWebView, and Linux WebKitGTK builds.
- Long PDFs, scanned/image-heavy PDFs, encrypted PDFs, malformed PDFs, CJK/font-heavy PDFs, and any XFA/form PDFs we intend to support.
- Viewer load cancellation, file switching, page rendering, memory cleanup, worker loading, and repeated open/close cycles.
