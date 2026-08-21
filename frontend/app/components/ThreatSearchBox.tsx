"use client";

/**
 * app/components/ThreatSearchBox.tsx
 *
 * Client Component — el único lugar del frontend con estado de UI.
 * Hace fetch a /api/check (el Route Handler proxy), nunca a la API Rust
 * directamente. La API key nunca llega al browser.
 */

import { useState, useRef } from "react";
import type { CheckResult } from "@/lib/api";
import ThreatResult from "./ThreatResult";

type SearchState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "threat"; data: CheckResult }
  | { status: "safe"; data: CheckResult }
  | { status: "error"; message: string };

export default function ThreatSearchBox() {
  const [query, setQuery] = useState("");
  const [state, setState] = useState<SearchState>({ status: "idle" });
  const inputRef = useRef<HTMLInputElement>(null);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    const value = query.trim();
    if (!value) return;

    setState({ status: "loading" });

    try {
      const res = await fetch(
        `/api/check?value=${encodeURIComponent(value)}`
      );

      if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        setState({
          status: "error",
          message: (body as { error?: string }).error ?? "Error al verificar el indicador",
        });
        return;
      }

      const data = (await res.json()) as CheckResult;
      setState({
        status: data.is_known_threat ? "threat" : "safe",
        data,
      });
    } catch {
      setState({
        status: "error",
        message: "No se pudo conectar al servicio de verificación",
      });
    }
  }

  function handleReset() {
    setQuery("");
    setState({ status: "idle" });
    inputRef.current?.focus();
  }

  return (
    <div className="search-box">
      <form onSubmit={handleSubmit} className="search-form">
        <div className="search-input-group">
          <div className="search-icon" aria-hidden="true">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <circle cx="11" cy="11" r="8" />
              <line x1="21" y1="21" x2="16.65" y2="16.65" />
            </svg>
          </div>
          <input
            ref={inputRef}
            id="threat-search-input"
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Ingresá un dominio, URL, IP o hash…"
            className="search-input"
            autoComplete="off"
            autoCorrect="off"
            autoCapitalize="off"
            spellCheck={false}
            aria-label="Indicador a verificar"
          />
          {query && (
            <button
              type="button"
              onClick={handleReset}
              className="search-clear-btn"
              aria-label="Limpiar búsqueda"
            >
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <line x1="18" y1="6" x2="6" y2="18" />
                <line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </button>
          )}
        </div>
        <button
          id="threat-search-submit"
          type="submit"
          disabled={!query.trim() || state.status === "loading"}
          className="search-submit-btn"
        >
          {state.status === "loading" ? (
            <span className="search-spinner" aria-label="Verificando…" />
          ) : (
            "Verificar"
          )}
        </button>
      </form>

      {/* Resultado — delegado a ThreatResult */}
      {(state.status === "threat" || state.status === "safe") && (
        <ThreatResult variant={state.status} data={state.data} />
      )}

      {state.status === "error" && (
        <div className="result-card result-error" role="alert">
          <span className="result-badge result-badge-error">Error</span>
          <p className="result-meta">{state.message}</p>
        </div>
      )}
    </div>
  );
}
