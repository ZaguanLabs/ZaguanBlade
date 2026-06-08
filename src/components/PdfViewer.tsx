import React, { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import * as pdfjsLib from 'pdfjs-dist';
import type { DocumentInitParameters } from 'pdfjs-dist/types/src/display/api';
import pdfjsWorkerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url';
import { ChevronLeft, ChevronRight, ZoomIn, ZoomOut, Maximize2, FileText } from 'lucide-react';
import { readFile } from '@tauri-apps/plugin-fs';
import { formatUnknownBackendError } from '../utils/backendErrors';
import { useDisplaySettings } from '../contexts/DisplaySettingsContext';

// Use the Vite-bundled worker URL. In Tauri's custom protocol (tauri://),
// PDF.js's origin check fails and it wraps the URL in a blob with
// `await import(url)` — which works correctly with Vite's hashed asset URLs.
pdfjsLib.GlobalWorkerOptions.workerSrc = new URL(pdfjsWorkerUrl, import.meta.url).href;

interface PdfViewerProps {
    filePath: string;
}

export const PdfViewer: React.FC<PdfViewerProps> = ({ filePath }) => {
    const { t } = useTranslation();
    const { editorFontSize } = useDisplaySettings();
    const canvasRefs = useRef<Map<number, HTMLCanvasElement>>(new Map());
    const containerRef = useRef<HTMLDivElement>(null);
    const [pdfDoc, setPdfDoc] = useState<pdfjsLib.PDFDocumentProxy | null>(null);
    const [currentPage, setCurrentPage] = useState(1);
    const [numPages, setNumPages] = useState(0);
    const [scale, setScale] = useState(1.5);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        setScale(Math.max(0.5, Math.min(3, 1.5 * (editorFontSize / 14))));
    }, [editorFontSize]);

    useEffect(() => {
        let isCancelled = false;

        async function loadPdf() {
            if (!filePath) return;

            setLoading(true);
            setError(null);
            setPdfDoc(null);
            setNumPages(0);
            setCurrentPage(1);
            canvasRefs.current.clear();

            try {
                console.debug('[PDF] Loading file:', filePath);
                
                // Read the PDF file as binary data using tauri-plugin-fs
                const fileData = await readFile(filePath);
                console.debug('[PDF] File read successfully, size:', fileData.length, 'bytes');
                
                // Load the PDF from the binary data (fileData is already Uint8Array)
                const documentParams: DocumentInitParameters & { isEvalSupported?: boolean } = {
                    data: fileData,
                    useWorkerFetch: false,
                    isEvalSupported: false,
                    useSystemFonts: true,
                };
                const loadingTask = pdfjsLib.getDocument(documentParams);
                const pdf = await loadingTask.promise;
                console.debug('[PDF] PDF loaded successfully, pages:', pdf.numPages);

                if (isCancelled) {
                    await pdf.destroy();
                    return;
                }
                
                setPdfDoc(pdf);
                setNumPages(pdf.numPages);
                setLoading(false);
            } catch (err) {
                if (isCancelled) return;
                console.error('[PDF] Error loading PDF:', err);
                setError(`${t('pdf.loadFailed')}: ${formatUnknownBackendError(err)}`);
                setLoading(false);
            }
        }

        loadPdf();

        return () => {
            isCancelled = true;
        };
    }, [filePath, t]);

    useEffect(() => {
        return () => {
            if (pdfDoc) {
                void pdfDoc.destroy();
            }
        };
    }, [pdfDoc]);

    useEffect(() => {
        let isCancelled = false;
        const renderTasks: pdfjsLib.RenderTask[] = [];

        async function renderPages() {
            if (!pdfDoc || numPages === 0) return;

            try {
                for (let pageNumber = 1; pageNumber <= numPages; pageNumber += 1) {
                    if (isCancelled) return;

                    const canvas = canvasRefs.current.get(pageNumber);
                    if (!canvas) continue;

                    const page = await pdfDoc.getPage(pageNumber);
                    if (isCancelled) return;

                    const viewport = page.getViewport({ scale });
                    const context = canvas.getContext('2d');

                    if (!context) continue;

                    canvas.height = viewport.height;
                    canvas.width = viewport.width;

                    const renderTask = page.render({
                        canvasContext: context,
                        viewport,
                    } as any);
                    renderTasks.push(renderTask);
                    await renderTask.promise;
                    page.cleanup();
                }
            } catch (err) {
                if (isCancelled || err instanceof Error && err.name === 'RenderingCancelledException') return;
                console.error('[PDF] Error rendering pages:', err);
                setError(`${t('pdf.renderFailed')}: ${formatUnknownBackendError(err)}`);
            }
        }

        renderPages();

        return () => {
            isCancelled = true;
            renderTasks.forEach(task => task.cancel());
        };
    }, [pdfDoc, numPages, scale, t]);

    useEffect(() => {
        if (!containerRef.current || numPages === 0) return;

        const observer = new IntersectionObserver(
            entries => {
                const visiblePage = entries
                    .filter(entry => entry.isIntersecting)
                    .sort((a, b) => b.intersectionRatio - a.intersectionRatio)[0];
                const pageNumber = Number(visiblePage?.target.getAttribute('data-page-number'));

                if (Number.isInteger(pageNumber)) {
                    setCurrentPage(pageNumber);
                }
            },
            {
                root: containerRef.current,
                threshold: [0.25, 0.5, 0.75],
            }
        );

        canvasRefs.current.forEach(canvas => observer.observe(canvas));

        return () => observer.disconnect();
    }, [numPages]);

    const scrollToPage = (pageNumber: number) => {
        const canvas = canvasRefs.current.get(pageNumber);
        canvas?.scrollIntoView({ block: 'start', behavior: 'smooth' });
        setCurrentPage(pageNumber);
    };

    const goToPrevPage = () => {
        if (currentPage > 1) {
            scrollToPage(currentPage - 1);
        }
    };

    const goToNextPage = () => {
        if (currentPage < numPages) {
            scrollToPage(currentPage + 1);
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
            <div className="h-full flex items-center justify-center bg-(--bg-app)">
                <div role="status" aria-live="polite" className="flex flex-col items-center gap-3">
                    <div aria-hidden="true" className="animate-spin w-8 h-8 border-2 border-(--accent-ai) border-t-transparent rounded-full" />
                    <p className="text-sm text-(--fg-secondary)">{t('pdf.loading')}</p>
                </div>
            </div>
        );
    }

    if (error) {
        return (
            <div className="h-full flex items-center justify-center bg-(--bg-app)">
                <div role="alert" className="flex flex-col items-center gap-3 max-w-md text-center">
                    <FileText className="w-12 h-12 text-(--state-danger) opacity-50" aria-hidden="true" />
                    <p className="text-sm text-(--state-danger)">{t('pdf.loadFailed')}</p>
                    <p className="text-xs text-(--fg-tertiary) font-mono">{error}</p>
                </div>
            </div>
        );
    }

    return (
        <div className="h-full flex flex-col bg-(--bg-app)" data-font-zoom-scope="editor">
            {/* Toolbar */}
            <div className="flex items-center justify-between px-4 py-2 border-b border-(--border-subtle) bg-(--bg-surface)">
                <div className="flex items-center gap-2">
                    <button
                        type="button"
                        onClick={goToPrevPage}
                        disabled={currentPage <= 1}
                        className="p-1.5 rounded-[calc(var(--panel-radius)*0.35)] hover:bg-(--bg-surface-hover) disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
                        title={t('pdf.previousPage')}
                        aria-label={t('pdf.previousPage')}
                    >
                        <ChevronLeft className="w-4 h-4 text-(--fg-secondary)" aria-hidden="true" />
                    </button>
                    <span className="text-xs text-(--fg-secondary) font-mono min-w-[80px] text-center">
                        {currentPage} / {numPages}
                    </span>
                    <button
                        type="button"
                        onClick={goToNextPage}
                        disabled={currentPage >= numPages}
                        className="p-1.5 rounded-[calc(var(--panel-radius)*0.35)] hover:bg-(--bg-surface-hover) disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
                        title={t('pdf.nextPage')}
                        aria-label={t('pdf.nextPage')}
                    >
                        <ChevronRight className="w-4 h-4 text-(--fg-secondary)" aria-hidden="true" />
                    </button>
                </div>

                <div className="flex items-center gap-2">
                    <button
                        type="button"
                        onClick={zoomOut}
                        className="p-1.5 rounded-[calc(var(--panel-radius)*0.35)] hover:bg-(--bg-surface-hover) transition-colors"
                        title={t('pdf.zoomOut')}
                        aria-label={t('pdf.zoomOut')}
                    >
                        <ZoomOut className="w-4 h-4 text-(--fg-secondary)" aria-hidden="true" />
                    </button>
                    <span className="text-xs text-(--fg-secondary) font-mono min-w-[50px] text-center">
                        {Math.round(scale * 100)}%
                    </span>
                    <button
                        type="button"
                        onClick={zoomIn}
                        className="p-1.5 rounded-[calc(var(--panel-radius)*0.35)] hover:bg-(--bg-surface-hover) transition-colors"
                        title={t('pdf.zoomIn')}
                        aria-label={t('pdf.zoomIn')}
                    >
                        <ZoomIn className="w-4 h-4 text-(--fg-secondary)" aria-hidden="true" />
                    </button>
                    <button
                        type="button"
                        onClick={fitToWidth}
                        className="p-1.5 rounded-[calc(var(--panel-radius)*0.35)] hover:bg-(--bg-surface-hover) transition-colors"
                        title={t('pdf.fitToWidth')}
                        aria-label={t('pdf.fitToWidth')}
                    >
                        <Maximize2 className="w-4 h-4 text-(--fg-secondary)" aria-hidden="true" />
                    </button>
                </div>
            </div>

            {/* PDF Pages */}
            <div 
                ref={containerRef}
                className="flex-1 overflow-auto bg-(--bg-app)"
            >
                <div className="flex flex-col items-center gap-5 p-5">
                    {Array.from({ length: numPages }, (_, index) => {
                        const pageNumber = index + 1;

                        return (
                            <canvas
                                key={pageNumber}
                                ref={canvas => {
                                    if (canvas) {
                                        canvasRefs.current.set(pageNumber, canvas);
                                    } else {
                                        canvasRefs.current.delete(pageNumber);
                                    }
                                }}
                                data-page-number={pageNumber}
                                role="img"
                                aria-label={t('pdf.pageCanvas', { page: pageNumber, total: numPages })}
                                className="shadow-(--shadow-xl)"
                                style={{ maxWidth: '100%', height: 'auto' }}
                            />
                        );
                    })}
                </div>
            </div>
        </div>
    );
};
