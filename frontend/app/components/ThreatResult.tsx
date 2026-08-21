/**
 * app/components/ThreatResult.tsx
 *
 * Componente de presentación pura — muestra el resultado de una verificación
 * de IOC con colores semánticos, TrustScore y campo "impersonates".
 * No tiene estado propio ni hace fetch; recibe los datos ya resueltos.
 */

import type { CheckResult } from "@/lib/api";

// ---------------------------------------------------------------------------
// Sub-componentes de icono
// ---------------------------------------------------------------------------

function IconThreat() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z" />
      <line x1="12" y1="9" x2="12" y2="13" />
      <line x1="12" y1="17" x2="12.01" y2="17" />
    </svg>
  );
}

function IconSafe() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <polyline points="20 6 9 17 4 12" />
    </svg>
  );
}

function IconWarn() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="10" />
      <line x1="12" y1="8" x2="12" y2="12" />
      <line x1="12" y1="16" x2="12.01" y2="16" />
    </svg>
  );
}

// ---------------------------------------------------------------------------
// TrustScore — barra visual de confianza
// ---------------------------------------------------------------------------

function TrustScore({ value }: { value: number }) {
  const pct = Math.round(value * 100);
  const barColor =
    value > 0.7
      ? "var(--threat-500)"
      : value > 0.4
      ? "var(--warn-500)"
      : "var(--safe-500)";

  return (
    <div className="trust-score">
      <div className="trust-score-label">
        <span>TrustScore</span>
        <strong style={{ color: barColor }}>{pct}%</strong>
      </div>
      <div className="trust-score-track">
        <div
          className="trust-score-fill"
          style={{ width: `${pct}%`, background: barColor }}
        />
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// ThreatResult — componente principal
// ---------------------------------------------------------------------------

export type ThreatResultVariant = "threat" | "safe";

interface ThreatResultProps {
  variant: ThreatResultVariant;
  data: CheckResult;
}

export default function ThreatResult({ variant, data }: ThreatResultProps) {
  const isThreat = variant === "threat";

  return (
    <div
      className={`result-card ${isThreat ? "result-threat" : "result-safe"}`}
      role={isThreat ? "alert" : "status"}
    >
      {/* Cabecera: badge + porcentaje de confianza textual */}
      <div className="result-header">
        <span
          className={`result-badge ${
            isThreat ? "result-badge-threat" : "result-badge-safe"
          }`}
        >
          {isThreat ? <IconThreat /> : <IconSafe />}
          {isThreat ? "Amenaza conocida" : "No encontrado"}
        </span>

        {data.trust_value !== null && (
          <span className="result-trust">
            Confianza: <strong>{Math.round(data.trust_value * 100)}%</strong>
          </span>
        )}
      </div>

      {/* Valor del IOC en monospace */}
      <p className="result-value">{data.value}</p>

      {/* Barra visual de TrustScore */}
      {data.trust_value !== null && (
        <TrustScore value={data.trust_value} />
      )}

      {/* Bloque de suplantación */}
      {isThreat && data.impersonates && (
        <div className="result-impersonates-block">
          <IconWarn />
          <p className="result-impersonates">
            Suplanta a <strong>{data.impersonates}</strong>
          </p>
        </div>
      )}

      {/* Mensaje limpio */}
      {!isThreat && (
        <p className="result-meta">
          No figura en la base de indicadores de amenaza activos.
        </p>
      )}
    </div>
  );
}
