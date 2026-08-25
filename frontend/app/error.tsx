"use client";

/**
 * app/error.tsx
 *
 * Error Boundary del App Router de Next.js.
 * Se activa cuando cualquier Server o Client Component del árbol lanza
 * una excepción no capturada. Ofrece un botón "Reintentar" que llama a
 * reset() para volver a renderizar el segmento.
 *
 * Ref: https://nextjs.org/docs/app/api-reference/file-conventions/error
 */

import { useEffect } from "react";

interface ErrorPageProps {
  error: Error & { digest?: string };
  reset: () => void;
}

export default function GlobalError({ error, reset }: ErrorPageProps) {
  useEffect(() => {
    // En producción esto iría a un servicio de logging (Sentry, etc.)
    console.error("[Trampantojo] Error no capturado:", error);
  }, [error]);

  return (
    <div className="error-page" role="alert">
      <div className="error-page-inner">
        {/* Icono de escudo roto */}
        <div className="error-page-icon" aria-hidden="true">
          <svg
            width="48"
            height="48"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
            <line x1="8" y1="12" x2="16" y2="12" />
          </svg>
        </div>

        <h1 className="error-page-title">Algo salió mal</h1>
        <p className="error-page-message">
          Ocurrió un error inesperado al cargar la página. Podés intentar
          nuevamente o volver más tarde.
        </p>

        {/* digest ayuda a correlacionar el error en los logs del servidor */}
        {error.digest && (
          <p className="error-page-digest">
            Código de referencia: <code>{error.digest}</code>
          </p>
        )}

        <div className="error-page-actions">
          <button className="error-page-retry-btn" onClick={reset}>
            Reintentar
          </button>
          <a href="/" className="error-page-home-link">
            Volver al inicio
          </a>
        </div>
      </div>
    </div>
  );
}
