"use client";

/**
 * app/components/StatsChart.tsx
 *
 * Client Component — recibe DailyStat[] como prop desde DashboardStats
 * (Server Component). No hace ningún fetch propio.
 *
 * Implementado con SVG nativo para eliminar dependencias de charting
 * (~150-250KB de bundle). Para el MVP un gráfico de barras agrupadas
 * por día es suficiente para mostrar tendencias.
 */

import type { DailyStat } from "@/lib/api";

interface Props {
  stats: DailyStat[];
}

// Paleta por tipo de indicador — colores semánticos coherentes con el resto del UI
const IOC_TYPE_COLORS: Record<string, string> = {
  domain:       "var(--color-chart-domain)",
  url:          "var(--color-chart-url)",
  ip_address:   "var(--color-chart-ip)",
  phone_number: "var(--color-chart-phone)",
  file_hash:    "var(--color-chart-hash)",
};

const DEFAULT_COLOR = "var(--color-chart-default)";

function getColor(type: string): string {
  return IOC_TYPE_COLORS[type] ?? DEFAULT_COLOR;
}

function formatDay(dayStr: string): string {
  // "2026-07-22" -> "22 Jul"
  const [, month, day] = dayStr.split("-");
  const months = ["Ene","Feb","Mar","Abr","May","Jun","Jul","Ago","Sep","Oct","Nov","Dic"];
  return `${parseInt(day)} ${months[parseInt(month) - 1]}`;
}

export default function StatsChart({ stats }: Props) {
  if (stats.length === 0) {
    return (
      <div className="chart-empty">
        <p>Sin datos para el período seleccionado.</p>
      </div>
    );
  }

  // Agrupar por día para las barras del eje X
  const dayMap = new Map<string, DailyStat[]>();
  for (const stat of stats) {
    const existing = dayMap.get(stat.day) ?? [];
    existing.push(stat);
    dayMap.set(stat.day, existing);
  }

  // Días ordenados cronológicamente
  const days = Array.from(dayMap.keys()).sort();

  // Tipos de indicador únicos en el dataset
  const types = Array.from(new Set(stats.map((s) => s.ioc_type)));

  // Máximo de eventos por día (para escalar la altura de las barras)
  const maxEvents = Math.max(
    1,
    ...days.map((d) =>
      (dayMap.get(d) ?? []).reduce((sum, s) => sum + s.events, 0)
    )
  );

  const CHART_H = 160;
  const BAR_GAP = 6;
  const CHART_W_PER_DAY = 52;
  const CHART_W = days.length * CHART_W_PER_DAY;

  return (
    <div className="chart-wrapper">
      <div className="chart-legend">
        {types.map((t) => (
          <span key={t} className="chart-legend-item">
            <span
              className="chart-legend-dot"
              style={{ background: getColor(t) }}
            />
            {t.replace("_", " ")}
          </span>
        ))}
      </div>

      <div className="chart-scroll">
        <svg
          width={CHART_W}
          height={CHART_H + 28}
          role="img"
          aria-label="Gráfico de eventos por día y tipo de indicador"
        >
          {days.map((day, di) => {
            const dayStats = dayMap.get(day) ?? [];
            const x = di * CHART_W_PER_DAY + BAR_GAP;
            const barW = CHART_W_PER_DAY - BAR_GAP * 2;
            let yOffset = CHART_H;

            return (
              <g key={day}>
                {dayStats.map((s) => {
                  const barH = Math.max(
                    2,
                    Math.round((s.events / maxEvents) * CHART_H)
                  );
                  yOffset -= barH;
                  return (
                    <rect
                      key={s.ioc_type}
                      x={x}
                      y={yOffset}
                      width={barW}
                      height={barH}
                      fill={getColor(s.ioc_type)}
                      rx={2}
                      className="chart-bar"
                    >
                      <title>
                        {formatDay(day)} · {s.ioc_type}: {s.events} eventos, {s.actionable} accionables
                      </title>
                    </rect>
                  );
                })}
                {/* Etiqueta del día */}
                <text
                  x={x + barW / 2}
                  y={CHART_H + 20}
                  textAnchor="middle"
                  className="chart-label"
                >
                  {formatDay(day)}
                </text>
              </g>
            );
          })}
        </svg>
      </div>
    </div>
  );
}
