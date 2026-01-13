import React, { Suspense } from 'react';
import ReactDOM from 'react-dom/client';
import { BrowserRouter } from 'react-router-dom';
import App from './App';
import './index.css';
import './i18n'; // Initialize i18n
import "@fontsource/fira-code"; // Defaults to weight 400
import "@fontsource/fira-code/500.css"; // Medium
import "@fontsource/fira-code/600.css"; // Semi-bold
import "@fontsource/fira-code/700.css"; // Bold

ReactDOM.createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
        <BrowserRouter>
            <Suspense fallback={<div className="h-screen w-screen bg-[var(--bg-app)]" />}>
                <App />
            </Suspense>
        </BrowserRouter>
    </React.StrictMode>
);
