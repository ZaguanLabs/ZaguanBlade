import React, { useState, useCallback, useMemo } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import remarkBreaks from 'remark-breaks';
import { Prism as SyntaxHighlighter } from 'react-syntax-highlighter';
import { oneDark } from 'react-syntax-highlighter/dist/esm/styles/prism';
import { Copy, Check } from 'lucide-react';

interface MarkdownRendererProps {
    content: string;
    className?: string;
}

// Stable theme object - defined outside component to prevent recreation
const customTheme = {
    ...oneDark,
    'pre[class*="language-"]': {
        ...oneDark['pre[class*="language-"]'],
        background: 'var(--markdown-block-bg)',
        margin: 0,
        padding: '1rem',
        fontSize: '12px',
        lineHeight: '1.5',
    },
    'code[class*="language-"]': {
        ...oneDark['code[class*="language-"]'],
        background: 'transparent',
        fontSize: '12px',
        lineHeight: '1.5',
    },
};

// Stable style objects - defined outside to prevent recreation
const codeBlockCustomStyle = {
    margin: 0,
    background: 'transparent',
    padding: '0.75rem 1rem',
};

const codeTagStyle = {
    fontFamily: 'var(--font-mono)',
};

interface CodeBlockProps {
    language: string;
    value: string;
}

// Memoized CodeBlock - only re-renders when language or value changes
const CodeBlock = React.memo<CodeBlockProps>(({ language, value }) => {
    const [copied, setCopied] = useState(false);

    const handleCopy = useCallback(async () => {
        try {
            await navigator.clipboard.writeText(value);
            setCopied(true);
            setTimeout(() => setCopied(false), 2000);
        } catch (err) {
            console.error('Failed to copy code:', err);
        }
    }, [value]);

    const displayLanguage = language || 'text';

    return (
        <div className="group relative my-3 rounded-lg overflow-hidden border border-(--markdown-border) bg-(--markdown-block-bg)">
            {/* Header with language label and copy button */}
            <div className="flex items-center justify-between px-3 py-1.5 border-b border-(--markdown-border)" style={{ backgroundColor: 'var(--markdown-block-header-bg)' }}>
                <span className="text-[10px] font-mono text-(--markdown-marker) uppercase tracking-wider">
                    {displayLanguage}
                </span>
                <button
                    onClick={handleCopy}
                    className="flex items-center gap-1 px-2 py-0.5 rounded text-[10px] text-(--markdown-marker) hover:text-(--markdown-heading) transition-colors"
                    style={{ backgroundColor: copied ? 'color-mix(in srgb, var(--accent-primary) 14%, transparent)' : undefined }}
                    title="Copy code"
                >
                    {copied ? (
                        <>
                            <Check className="w-3 h-3 text-(--markdown-link)" />
                            <span className="text-(--markdown-link)">Copied</span>
                        </>
                    ) : (
                        <>
                            <Copy className="w-3 h-3" />
                            <span>Copy</span>
                        </>
                    )}
                </button>
            </div>

            {/* Code content */}
            <div className="overflow-x-auto">
                <SyntaxHighlighter
                    language={language || 'text'}
                    style={customTheme}
                    customStyle={codeBlockCustomStyle}
                    codeTagProps={{ style: codeTagStyle }}
                >
                    {value}
                </SyntaxHighlighter>
            </div>
        </div>
    );
});
CodeBlock.displayName = 'CodeBlock';

// Simple inline code - no need for heavy memoization
const InlineCode: React.FC<{ children: React.ReactNode }> = ({ children }) => (
    <code className="px-1.5 py-0.5 rounded bg-(--markdown-inline-code-bg) text-(--markdown-inline-code-fg) text-[11px] font-mono">
        {children}
    </code>
);

// Stable remark plugins array - defined outside to prevent recreation
const remarkPlugins = [remarkGfm, remarkBreaks];

// Stable components object - defined outside to prevent recreation on every render
// This is CRITICAL for performance - ReactMarkdown does deep comparison
const markdownComponents = {
    // Code blocks
    code({ className, children }: { className?: string; children?: React.ReactNode }) {
        const match = /language-(\w+)/.exec(className || '');
        const language = match ? match[1] : '';
        const value = String(children).replace(/\n$/, '');

        // Check if this is a code block (has language or multiple lines)
        const isCodeBlock = match || value.includes('\n');

        if (isCodeBlock) {
            return <CodeBlock language={language} value={value} />;
        }

        return <InlineCode>{children}</InlineCode>;
    },

    // Paragraphs
    p({ children }: { children?: React.ReactNode }) {
        return (
            <p className="text-[12px] font-medium text-(--markdown-body) leading-relaxed my-2 first:mt-0 last:mb-0">
                {children}
            </p>
        );
    },

    // Headings
    h1({ children }: { children?: React.ReactNode }) {
        return (
            <h1 className="text-[15px] font-semibold text-(--markdown-heading) mt-4 mb-2 first:mt-0 border-b border-(--markdown-border) pb-1">
                {children}
            </h1>
        );
    },
    h2({ children }: { children?: React.ReactNode }) {
        return (
            <h2 className="text-[14px] font-semibold text-(--markdown-heading) mt-4 mb-2 first:mt-0 border-b border-(--markdown-border) pb-1">
                {children}
            </h2>
        );
    },
    h3({ children }: { children?: React.ReactNode }) {
        return (
            <h3 className="text-[13px] font-semibold text-(--markdown-heading) mt-3 mb-1.5 first:mt-0">
                {children}
            </h3>
        );
    },
    h4({ children }: { children?: React.ReactNode }) {
        return (
            <h4 className="text-[12px] font-semibold text-(--markdown-heading) mt-2 mb-1 first:mt-0">
                {children}
            </h4>
        );
    },

    // Lists
    ul({ children }: { children?: React.ReactNode }) {
        return (
            <ul className="my-2 ml-4 space-y-1 list-disc marker:text-(--markdown-marker)">
                {children}
            </ul>
        );
    },
    ol({ children }: { children?: React.ReactNode }) {
        return (
            <ol className="my-2 ml-4 space-y-1 list-decimal marker:text-(--markdown-marker)">
                {children}
            </ol>
        );
    },
    li({ children }: { children?: React.ReactNode }) {
        return (
            <li className="text-[12px] font-medium text-(--markdown-body) leading-relaxed pl-1">
                {children}
            </li>
        );
    },

    // Links
    a({ href, children }: { href?: string; children?: React.ReactNode }) {
        return (
            <a
                href={href}
                target="_blank"
                rel="noopener noreferrer"
                className="text-(--markdown-link) hover:text-(--markdown-link-hover) hover:underline transition-colors"
            >
                {children}
            </a>
        );
    },

    // Strong/Bold
    strong({ children }: { children?: React.ReactNode }) {
        return <strong className="font-semibold text-(--markdown-strong)">{children}</strong>;
    },

    // Emphasis/Italic
    em({ children }: { children?: React.ReactNode }) {
        return <em className="italic text-(--markdown-body)">{children}</em>;
    },

    // Blockquotes
    blockquote({ children }: { children?: React.ReactNode }) {
        return (
            <blockquote className="my-3 pl-3 border-l-2 text-(--markdown-body) italic" style={{ borderLeftColor: 'color-mix(in srgb, var(--markdown-link) 50%, transparent)' }}>
                {children}
            </blockquote>
        );
    },

    // Horizontal rule
    hr() {
        return <hr className="my-4 border-(--markdown-border)" />;
    },

    // Tables
    table({ children }: { children?: React.ReactNode }) {
        return (
            <div className="my-3 overflow-x-auto rounded-lg border border-(--markdown-border)">
                <table className="w-full text-[12px]">
                    {children}
                </table>
            </div>
        );
    },
    thead({ children }: { children?: React.ReactNode }) {
        return (
            <thead className="border-b border-(--markdown-border)" style={{ backgroundColor: 'var(--markdown-block-header-bg)' }}>
                {children}
            </thead>
        );
    },
    tbody({ children }: { children?: React.ReactNode }) {
        return <tbody className="divide-y divide-(--markdown-border)">{children}</tbody>;
    },
    tr({ children }: { children?: React.ReactNode }) {
        return (
            <tr className="transition-colors hover:bg-(--markdown-table-hover)">
                {children}
            </tr>
        );
    },
    th({ children }: { children?: React.ReactNode }) {
        return (
            <th className="px-3 py-2 text-left font-semibold text-(--markdown-heading)">
                {children}
            </th>
        );
    },
    td({ children }: { children?: React.ReactNode }) {
        return (
            <td className="px-3 py-2 text-(--markdown-body)">
                {children}
            </td>
        );
    },

    // Images
    img({ src, alt }: { src?: string; alt?: string }) {
        return (
            <img
                src={src}
                alt={alt || ''}
                className="my-3 max-w-full rounded-lg border border-(--markdown-border)"
            />
        );
    },
};

const MarkdownRendererComponent: React.FC<MarkdownRendererProps> = ({ content, className = '' }) => {
    return (
        <div className={`markdown-content select-text ${className}`}>
            <ReactMarkdown
                remarkPlugins={remarkPlugins}
                components={markdownComponents}
            >
                {content}
            </ReactMarkdown>
        </div>
    );
};

// Custom comparison - only re-render if content actually changed
export const MarkdownRenderer = React.memo(MarkdownRendererComponent, (prevProps, nextProps) => {
    return prevProps.content === nextProps.content && prevProps.className === nextProps.className;
});
export default MarkdownRenderer;
