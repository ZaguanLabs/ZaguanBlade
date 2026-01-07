'use client';
import dynamic from 'next/dynamic';

const AppLayout = dynamic(() => import('../components/Layout').then(mod => mod.AppLayout), { ssr: false });

export default function Home() {
  return (
    <main className="h-screen w-screen bg-black">
      <AppLayout />
    </main>
  );
}
