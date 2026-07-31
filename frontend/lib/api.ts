/**
 * lib/api.ts
 *
 * Helpers server-only para hablar con la API Rust.
 * Este módulo NUNCA debe importarse desde un Client Component —
 * process.env.TRAMPANTOJO_API_KEY no existe en el bundle del browser.
 *
 * Contratos de caché por función:
 *   - getStats()       → revalidate: 60s  (dashboard, tolera lag de 1 min)
 *   - checkIndicator() → no-store         (verificación de amenaza, siempre fresca)
 */

const API_URL = process.env.TRAMPANTOJO_API_URL ?? "http://localhost:8080";
const API_KEY = process.env.TRAMPANTOJO_API_KEY ?? "";

// ---------------------------------------------------------------------------
// Tipos — espejo de los structs de trampantojo-core
// ---------------------------------------------------------------------------

export interface DailyStat {
  day: string;          // "YYYY-MM-DD"
  impersonates: string;
  ioc_type: string;
  events: number;
  actionable: number;
}

export interface CheckResult {
  value: string;
  is_known_threat: boolean;
  trust_value: number | null;
  impersonates: string | null;
}

// ---------------------------------------------------------------------------
// Helpers internos
// ---------------------------------------------------------------------------

function buildHeaders(): Record<string, string> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
  };
  if (API_KEY) {
    headers["X-API-Key"] = API_KEY;
  }
  return headers;
}

async function apiFetch<T>(
  path: string,
  options: RequestInit & { next?: { revalidate?: number; tags?: string[] } }
): Promise<T> {
  const url = `${API_URL}${path}`;
  const res = await fetch(url, {
    ...options,
    headers: {
      ...buildHeaders(),
      ...(options.headers as Record<string, string> ?? {}),
    },
  });

  if (!res.ok) {
    // Detalle interno va a logs del servidor, nunca al cliente.
    console.error(`[api] ${options.method ?? "GET"} ${url} -> ${res.status}`);
    throw new Error(`API error: ${res.status}`);
  }

  return res.json() as Promise<T>;
}

// ---------------------------------------------------------------------------
// Funciones públicas
// ---------------------------------------------------------------------------

/**
 * Obtiene estadísticas agregadas diarias de los últimos N días.
 * La respuesta se cachea 60 segundos — mil visitantes del dashboard
 * no generan mil requests a ClickHouse.
 */
export async function getStats(days: number = 7): Promise<DailyStat[]> {
  return apiFetch<DailyStat[]>(`/v1/stats?days=${days}`, {
    method: "GET",
    next: { revalidate: 60 },
  });
}

/**
 * Verifica si un indicador es una amenaza conocida.
 * Siempre fresco — si un IoC acaba de ser confirmado, el buscador
 * debe verlo inmediatamente. cache: 'no-store' lo garantiza.
 */
export async function checkIndicator(value: string): Promise<CheckResult> {
  const encoded = encodeURIComponent(value);
  return apiFetch<CheckResult>(`/v1/check?value=${encoded}`, {
    method: "GET",
    cache: "no-store",
  });
}
