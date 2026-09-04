'use client';

import Link from 'next/link';
import { ReactNode } from 'react';

interface AuthLayoutProps {
  children: ReactNode;
  title: string;
  subtitle: string;
}

export default function AuthLayout({ children, title, subtitle }: AuthLayoutProps) {
  return (
    <div className="auth-page">
      {/* Fondo animado */}
      <div className="auth-bg" aria-hidden="true">
        <div className="auth-bg-orb auth-bg-orb-1" />
        <div className="auth-bg-orb auth-bg-orb-2" />
        <div className="auth-bg-grid" />
      </div>

      {/* Card central */}
      <div className="auth-card-wrapper">
        {/* Logo */}
        <Link href="/" className="auth-logo" aria-label="Volver al inicio">
          <div className="auth-logo-icon">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="#fff" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/>
            </svg>
          </div>
          <span className="auth-logo-name">Trampantojo</span>
        </Link>

        {/* Card */}
        <div className="auth-card">
          <div className="auth-card-header">
            <h1 className="auth-card-title">{title}</h1>
            <p className="auth-card-subtitle">{subtitle}</p>
          </div>
          {children}
        </div>

        {/* Footer */}
        <p className="auth-footer-text">
          © {new Date().getFullYear()} Trampantojo &mdash; Threat Intelligence para América Latina
        </p>
      </div>
    </div>
  );
}
