import React, { useEffect, useRef, useState } from 'react';
import * as pdfjsLib from 'pdfjs-dist';
import pdfjsWorkerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url';
import { ChevronLeft, ChevronRight, ZoomIn, ZoomOut, Maximize2, FileText } from 'lucide-react';
import { readFile } from '@tauri-apps/plugin-fs';

// Use the Vite-bundled worker URL. In Tauri's custom protocol (tauri://),
// PDF.js's origin check fails and it wraps the URL in a blob with
// `await import(url)` — which works correctly with Vite's hashed asset URLs.
pdfjsLib.GlobalWorkerOptions.workerSrc = new URL(pdfjsWorkerUrl, import.meta.url).href;

interface PdfViewerProps {
    filePath: string;
}

export const PdfViewer: React.FC<PdfViewerProps> = ({ filePath }) => {
    const canvasRef = useRef<HTMLCanvasElement>(null);
    const containerRef = useRef<HTMLDivElement>(null);
    const [pdfDoc, setPdfDoc] = useState<pdfjsLib.PDFDocumentProxy | null>(null);
    const [currentPage, setCurrentPage] = useState(1);
    const [numPages, setNumPages] = useState(0);
    const [scale, setScale] = useState(1.5);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        async function loadPdf() {
            if (!filePath) return;

            setLoading(true);
            setError(null);

            try {
                console.log('[PDF] Loading file:', filePath);
                
                // Read the PDF file as binary data using tauri-plugin-fs
                const fileData = await readFile(filePath);
                console.log('[PDF] File read successfully, size:', fileData.length, 'bytes');
                
                // Load the PDF from the binary data (fileData is already Uint8Array)
                const loadingTask = pdfjsLib.getDocument({
                    data: fileData,
                    useWorkerFetch: false,
                    isEvalSupported: false,
                    useSystemFonts: true,
                });
                const pdf = await loadingTask.promise;
                console.log('[PDF] PDF loaded successfully, pages:', pdf.numPages);
                
                setPdfDoc(pdf);
                setNumPages(pdf.numPages);
                setCurrentPage(1);
                setLoading(false);
            } catch (err) {
                console.error('[PDF] Error loading PDF:', err);
                setError(err instanceof Error ? err.message : 'Failed to load PDF');
                setLoading(false);
            }
        }

        loadPdf();

        return () => {
            if (pdfDoc) {
                pdfDoc.destroy();
            }
        };
    }, [filePath]);

    useEffect(() => {
        async function renderPage() {
            if (!pdfDoc || !canvasRef.current) return;

            try {
                const page = await pdfDoc.getPage(currentPage);
                const viewport = page.getViewport({ scale });
                const canvas = canvasRef.current;
                const context = canvas.getContext('2d');

                if (!context) return;

                canvas.height = viewport.height;
                canvas.width = viewport.width;

                const renderContext = {
                    canvasContext: context,
                    viewport: viewport,
                };

                await page.render(renderContext as any).promise;
            } catch (err) {
                console.error('Error rendering page:', err);
                setError(err instanceof Error ? err.message : 'Failed to render page');
            }
        }

        renderPage();
    }, [pdfDoc, currentPage, scale]);

    const goToPrevPage = () => {
        if (currentPage > 1) {
            setCurrentPage(currentPage - 1);
        }
    };

    const goToNextPage = () => {
        if (currentPage < numPages) {
            setCurrentPage(currentPage + 1);
        }
    };

    const zoomIn = () => {
        setScale(prev => Math.min(prev + 0.25, 3));
    };

    const zoomOut = () => {
        setScale(prev => Math.max(prev - 0.25, 0.5));
    };

    const fitToWidth = () => {
        if (containerRef.current && pdfDoc) {
            const containerWidth = containerRef.current.clientWidth - 40;
            pdfDoc.getPage(currentPage).then(page => {
                const viewport = page.getViewport({ scale: 1 });
                const newScale = containerWidth / viewport.width;
                setScale(newScale);
            });
        }
    };

    if (loading) {
        return (
            <div className="h-full flex items-center justify-center bg-[var(--bg-app)]">
                <div className="flex flex-col items-center gap-3">
                    <div className="animate-spin w-8 h-8 border-2 border-[var(--accent-primary)] border-t-transparent rounded-full" />
                    <p className="text-sm text-[var(--fg-secondary)]">Loading PDF...</p>
                </div>
            </div>
        );
    }

    if (error) {
        return (
            <div className="h-full flex items-center justify-center bg-[var(--bg-app)]">
                <div className="flex flex-col items-center gap-3 max-w-md text-center">
                    <FileText className="w-12 h-12 text-red-400 opacity-50" />
                    <p className="text-sm text-red-400">Failed to load PDF</p>
                    <p className="text-xs text-[var(--fg-tertiary)] font-mono">{error}</p>
                </div>
            </div>
        );
    }

    return (
        <div className="h-full flex flex-col bg-[var(--bg-app)]">
            {/* Toolbar */}
            <div className="flex items-center justify-between px-4 py-2 border-b border-[var(--border-subtle)] bg-[var(--bg-surface)]">
                <div className="flex items-center gap-2">
                    <button
                        onClick={goToPrevPage}
                        disabled={currentPage <= 1}
                        className="p-1.5 rounded hover:bg-[var(--bg-surface-hover)] disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
                        title="Previous page"
                    >
                        <ChevronLeft className="w-4 h-4 text-[var(--fg-secondary)]" />
                    </button>
                    <span className="text-xs text-[var(--fg-secondary)] font-mono min-w-[80px] text-center">
                        {currentPage} / {numPages}
                    </span>
                    <button
                        onClick={goToNextPage}
                        disabled={currentPage >= numPages}
                        className="p-1.5 rounded hover:bg-[var(--bg-surface-hover)] disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
                        title="Next page"
                    >
                        <ChevronRight className="w-4 h-4 text-[var(--fg-secondary)]" />
                    </button>
                </div>

                <div className="flex items-center gap-2">
                    <button
                        onClick={zoomOut}
                        className="p-1.5 rounded hover:bg-[var(--bg-surface-hover)] transition-colors"
                        title="Zoom out"
                    >
                        <ZoomOut className="w-4 h-4 text-[var(--fg-secondary)]" />
                    </button>
                    <span className="text-xs text-[var(--fg-secondary)] font-mono min-w-[50px] text-center">
                        {Math.round(scale * 100)}%
                    </span>
                    <button
                        onClick={zoomIn}
                        className="p-1.5 rounded hover:bg-[var(--bg-surface-hover)] transition-colors"
                        title="Zoom in"
                    >
                        <ZoomIn className="w-4 h-4 text-[var(--fg-secondary)]" />
                    </button>
                    <button
                        onClick={fitToWidth}
                        className="p-1.5 rounded hover:bg-[var(--bg-surface-hover)] transition-colors"
                        title="Fit to width"
                    >
                        <Maximize2 className="w-4 h-4 text-[var(--fg-secondary)]" />
                    </button>
                </div>
            </div>

            {/* PDF Canvas */}
            <div 
                ref={containerRef}
                className="flex-1 overflow-auto flex items-start justify-center p-5 bg-[var(--bg-app)]"
            >
                <canvas
                    ref={canvasRef}
                    className="shadow-2xl"
                    style={{ maxWidth: '100%', height: 'auto' }}
                />
            </div>
        </div>
    );
};
