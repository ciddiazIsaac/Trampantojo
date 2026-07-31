/**
 * app/api/check/route.ts
 *
 * Route Handler proxy para el buscador.
 *
 * El navegador llama a GET /api/check?value=<indicador>.
 * Este handler corre en el servidor de Next.js, añade la API Key
 * interna, y reenvía al endpoint /v1/check de la API Rust.
 *
 * La API Key NUNCA sale al browser — vive solo en process.env server-side.
 * El tráfico del propio frontend nunca compite con el rate limit anónimo
 * (5 req/s) porque usa la key de alta capacidad (100 req/s).
 */

import { NextRequest } from "next/server";
import { checkIndicator } from "@/lib/api";

export async function GET(request: NextRequest) {
  const { searchParams } = request.nextUrl;
  const value = searchParams.get("value")?.trim();

  if (!value) {
    return Response.json(
      { error: "El parámetro 'value' es obligatorio" },
      { status: 400 }
    );
  }

  try {
    const result = await checkIndicator(value);
    return Response.json(result);
  } catch (err) {
    // El detalle del error (URL interna, status code de la API Rust) queda
    // en los logs del servidor. El cliente solo ve un 502 genérico.
    console.error("[/api/check] error contactando la API:", err);
    return Response.json(
      { error: "No se pudo contactar el servicio de verificación" },
      { status: 502 }
    );
  }
}
