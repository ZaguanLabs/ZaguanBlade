import React from 'react';
import { ChevronRight } from 'lucide-react';

interface BreadcrumbProps {
    filePath: string;
    workspaceRoot?: string;
}

export const Breadcrumb: React.FC<BreadcrumbProps> = ({ filePath, workspaceRoot }) => {
    // Get relative path from workspace root
    const getRelativePath = () => {
        if (!workspaceRoot) return filePath;
        
        // Remove workspace root from file path
        const relativePath = filePath.startsWith(workspaceRoot)
            ? filePath.slice(workspaceRoot.length)
            : filePath;
        
        // Remove leading slash
        return relativePath.startsWith('/') ? relativePath.slice(1) : relativePath;
    };

    const relativePath = getRelativePath();
    const segments = relativePath.split('/').filter(Boolean);

    if (segments.length === 0) return null;

    return (
        <div className="flex items-center gap-1.5 px-4 py-2 my-2 bg-[var(--bg-app)] border-b border-[var(--border-default)] shadow-[var(--shadow-sm)] text-xs text-[var(--fg-tertiary)] font-mono overflow-x-auto whitespace-nowrap">
            {segments.map((segment, index) => (
                <React.Fragment key={index}>
                    {index > 0 && (
                        <ChevronRight className="w-3.5 h-3.5 shrink-0 opacity-50" />
                    )}
                    <span
                        className={`${
                            index === segments.length - 1
                                ? 'text-[var(--fg-secondary)] font-medium'
                                : 'text-[var(--fg-tertiary)]'
                        }`}
                    >
                        {segment}
                    </span>
                </React.Fragment>
            ))}
        </div>
    );
};
