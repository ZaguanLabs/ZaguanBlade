import { Routes, Route, Navigate } from 'react-router-dom';
import { AppLayout } from './components/Layout';

export default function App() {
    return (
        <Routes>
            <Route path="/" element={<AppLayout />} />
            <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
    );
}
