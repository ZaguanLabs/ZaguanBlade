import { linter, Diagnostic } from "@codemirror/lint";
import { ZLPService } from "../../../services/zlp";
import { ZLPValidationError } from "../../../types/zlp";

// Map file extension to language ID for ZLP
function getLanguageId(filename: string): string {
    const ext = filename.split('.').pop()?.toLowerCase() || '';
    switch (ext) {
        case 'ts': case 'tsx': return 'typescript';
        case 'js': case 'jsx': return 'javascript';
        case 'rs': return 'rust';
        case 'py': return 'python';
        case 'go': return 'go';
        case 'json': return 'json';
        case 'html': return 'html';
        case 'css': return 'css';
        default: return 'plaintext';
    }
}

/**
 * Creates a CodeMirror linter extension backed by ZLP.
 */
export function zlpLinter(filename: string) {
    return linter(async (view) => {
        if (!filename) return [];

        const content = view.state.doc.toString();
        const language = getLanguageId(filename);

        // ZLP doesn't validate plaintext
        if (language === 'plaintext') return [];

        try {
            // Call ZLP Service
            const errors = await ZLPService.getDiagnostics(filename, content, language);

            // Convert ZLP errors to CodeMirror Diagnostics
            return errors.map((err: ZLPValidationError) => {
                // Determine CodeMirror 0-indexed range
                const startLine = view.state.doc.line(err.range.start.line);
                const endLine = view.state.doc.line(err.range.end.line);

                const from = Math.min(startLine.from + err.range.start.column, startLine.to);
                const to = Math.min(endLine.from + err.range.end.column, endLine.to);

                return {
                    from,
                    to,
                    severity: err.severity === 'error' ? 'error' : err.severity === 'warning' ? 'warning' : 'info',
                    message: err.message,
                    source: "zlp"
                } as Diagnostic;
            });

        } catch (e) {
            console.warn('[ZLP Linter] Failed to get diagnostics:', e);
            return [];
        }
    }, {
        // Debounce validation (CodeMirror supports this natively via delay property option, 
        // but explicit delay here is good too).
        delay: 500
    });
}
