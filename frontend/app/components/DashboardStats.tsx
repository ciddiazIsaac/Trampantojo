/**
 * app/components/DashboardStats.tsx
 *
 * Server Component — llama a /v1/stats desde el servidor de Next.js.
 * La respuesta se cachea 60 segundos (definido en lib/api.ts).
 * Si la API Rust no está disponible, renderiza un fallback silencioso
 * en vez de romper la página completa.
 */

import { getStats, type DailyStat } from "@/lib/api";
import StatsChart from "./StatsChart";

function computeKpis(stats: DailyStat[]) {
  const totalEvents = stats.reduce((s, r) => s + r.events, 0);
  const totalActionable = stats.reduce((s, r) => s + r.actionable, 0);
  const uniqueEntities = new Set(
    stats.map((r) => r.impersonates).filter(Boolean)
  ).size;
  return { totalEvents, totalActionable, uniqueEntities };
}

function KpiCard({
  label,
  value,
  accent,
}: {
  label: string;
  value: string | number;
  accent?: "threat" | "neutral";
}) {
  return (
    <div className={`kpi-card${accent ? ` kpi-card-${accent}` : ""}`}>
      <span className="kpi-value">{value}</span>
      <span className="kpi-label">{label}</span>
    </div>
  );
}

export default async function DashboardStats() {
  let stats: DailyStat[] = [];
  let fetchError = false;

  try {
    stats = await getStats(7);
  } catch {
    fetchError = true;
  }

  const { totalEvents, totalActionable, uniqueEntities } = computeKpis(stats);

  return (
    <section className="dashboard-section" aria-label="Estadísticas de amenazas">
      <div className="dashboard-header">
        <h2 className="dashboard-title">Actividad — últimos 7 días</h2>
        {fetchError && (
          <span className="dashboard-error-badge" role="status">
            API no disponible
          </span>
        )}
      </div>

      <div className="kpi-grid">
        <KpiCard label="Eventos registrados" value={totalEvents.toLocaleString("es-CL")} />
        <KpiCard
          label="Indicadores accionables"
          value={totalActionable.toLocaleString("es-CL")}
          accent={totalActionable > 0 ? "threat" : "neutral"}
        />
        <KpiCard
          label="Entidades suplantadas"
          value={uniqueEntities}
        />
      </div>

      <StatsChart stats={stats} />
    </section>
  );
}
