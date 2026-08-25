import { Suspense } from "react";
import Link from "next/link";
import ThreatSearchBox from "./components/ThreatSearchBox";
import DashboardStats from "./components/DashboardStats";

/** Skeleton que ocupa el espacio del dashboard mientras el Server Component carga */
function DashboardSkeleton() {
  return (
    <section className="dashboard-section" aria-busy="true" aria-label="Cargando estadísticas">
      <div className="dashboard-header">
        <div className="skeleton skeleton-title" />
      </div>
      <div className="kpi-grid">
        {[0, 1, 2].map((i) => (
          <div key={i} className="kpi-card">
            <div className="skeleton skeleton-kpi-value" />
            <div className="skeleton skeleton-kpi-label" />
          </div>
        ))}
      </div>
      <div className="skeleton skeleton-chart" />
    </section>
  );
}

export default function Home() {
  return (
    <>
      {/* Navbar minimalista */}
      <nav className="nav">
        <div className="container nav-inner">
          <Link href="/" className="nav-brand">
            <div className="nav-logo">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="#fff" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/>
              </svg>
            </div>
            <span className="nav-name">Trampantojo</span>
            <span className="nav-tagline">Threat Intel Latam</span>
          </Link>
          <span className="nav-badge">Beta</span>
        </div>
      </nav>

      <main className="page-main">
        {/* Hero Section — disponible inmediatamente, no bloquea el stream */}
        <header className="container hero">
          <div className="hero-content">
            <div className="hero-eyebrow">
              <span className="hero-eyebrow-dot" />
              Base de datos activa
            </div>
            <h1 className="hero-title">
              Inteligencia contra el fraude <br />
              <span className="hero-title-accent">en América Latina.</span>
            </h1>
            <p className="hero-subtitle">
              Verificá en tiempo real si un dominio, URL, IP o hash fue reportado
              como malicioso por fuentes oficiales (CSIRT) o corroborado por la
              comunidad.
            </p>

            <ThreatSearchBox />
          </div>
        </header>

        {/* Dashboard Section — streamed; muestra skeleton hasta que el Server Component resuelva */}
        <div className="container">
          <Suspense fallback={<DashboardSkeleton />}>
            <DashboardStats />
          </Suspense>
        </div>
      </main>

      {/* Footer */}
      <footer className="footer">
        <div className="container footer-inner">
          <p className="footer-text">
            © {new Date().getFullYear()} Trampantojo. Proyecto open-source.
          </p>
          <div className="footer-links">
            <a href="https://github.com/ciddiazIsaac/Trampantojo" target="_blank" rel="noopener noreferrer" className="footer-link">
              GitHub
            </a>
            <Link href="/docs" className="footer-link">
              API Docs
            </Link>
          </div>
        </div>
      </footer>
    </>
  );
}

