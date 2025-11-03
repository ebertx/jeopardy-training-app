'use client';

import Link from 'next/link';
import { signOut } from 'next-auth/react';
import { useState } from 'react';

interface NavigationProps {
  title: string;
  username?: string;
  userRole?: string;
  bgColor?: string;
}

export default function Navigation({ title, username, userRole, bgColor = 'bg-jeopardy-blue' }: NavigationProps) {
  const [isMenuOpen, setIsMenuOpen] = useState(false);

  return (
    <div className={`${bgColor} text-white p-4`}>
      <div className="max-w-6xl mx-auto">
        <div className="flex justify-between items-center">
          <h1 className="text-xl md:text-2xl font-bold">{title}</h1>

          {/* Hamburger menu button for mobile */}
          <button
            onClick={() => setIsMenuOpen(!isMenuOpen)}
            className="md:hidden flex flex-col gap-1 p-2"
            aria-label="Toggle menu"
          >
            <span className={`block w-6 h-0.5 bg-white transition-transform ${isMenuOpen ? 'rotate-45 translate-y-1.5' : ''}`}></span>
            <span className={`block w-6 h-0.5 bg-white transition-opacity ${isMenuOpen ? 'opacity-0' : ''}`}></span>
            <span className={`block w-6 h-0.5 bg-white transition-transform ${isMenuOpen ? '-rotate-45 -translate-y-1.5' : ''}`}></span>
          </button>

          {/* Desktop navigation */}
          <div className="hidden md:flex items-center gap-6">
            {username && <span className="text-sm">Welcome, {username}!</span>}
            <Link href="/quiz" className="hover:underline">Quiz</Link>
            <Link href="/review" className="hover:underline">Review</Link>
            <Link href="/mastered" className="hover:underline">Mastered</Link>
            <Link href="/study" className="hover:underline">Study</Link>
            <Link href="/dashboard" className="hover:underline">Dashboard</Link>
            <Link href="/settings" className="hover:underline">Settings</Link>
            {userRole === 'admin' && (
              <Link href="/admin" className="hover:underline text-yellow-300 font-semibold">Admin</Link>
            )}
            <button onClick={() => signOut()} className="hover:underline">Logout</button>
          </div>
        </div>

        {/* Mobile navigation menu */}
        {isMenuOpen && (
          <div className="md:hidden mt-4 flex flex-col gap-3 pb-2">
            {username && <span className="text-sm py-2 border-b border-white/20">Welcome, {username}!</span>}
            <Link href="/quiz" className="hover:underline py-2" onClick={() => setIsMenuOpen(false)}>Quiz</Link>
            <Link href="/review" className="hover:underline py-2" onClick={() => setIsMenuOpen(false)}>Review</Link>
            <Link href="/mastered" className="hover:underline py-2" onClick={() => setIsMenuOpen(false)}>Mastered</Link>
            <Link href="/study" className="hover:underline py-2" onClick={() => setIsMenuOpen(false)}>Study</Link>
            <Link href="/dashboard" className="hover:underline py-2" onClick={() => setIsMenuOpen(false)}>Dashboard</Link>
            <Link href="/settings" className="hover:underline py-2" onClick={() => setIsMenuOpen(false)}>Settings</Link>
            {userRole === 'admin' && (
              <Link href="/admin" className="hover:underline py-2 text-yellow-300 font-semibold" onClick={() => setIsMenuOpen(false)}>Admin</Link>
            )}
            <button onClick={() => signOut()} className="hover:underline py-2 text-left">Logout</button>
          </div>
        )}
      </div>
    </div>
  );
}
