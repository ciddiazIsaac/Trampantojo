/**
 * lib/auth.ts
 *
 * Helpers client-side para gestión de sesión vía JWT en localStorage.
 * Este módulo sólo corre en el browser — nunca importar desde Server Components.
 *
 * Estructura del JWT payload (Claims del backend Rust):
 *   sub     : email del usuario
 *   exp     : timestamp de expiración (Unix segundos)
 *   org_id  : UUID de la organización
 *   role    : "admin" | "member" | etc.
 */

const TOKEN_KEY = 'trampantojo_token';

export interface JwtPayload {
  sub: string;    // email
  exp: number;    // Unix timestamp (segundos)
  org_id: string;
  role: string;
}

export interface AuthUser {
  email: string;
  org_id: string;
  role: string;
  exp: number;
}

// ---------------------------------------------------------------------------
// Token storage
//
// El token se guarda en dos lugares:
//   1. localStorage — accesible desde JS del browser (lectura rápida)
//   2. cookie de sesión — accesible desde el Edge Middleware de Next.js para
//      protección de rutas en el servidor. No es httpOnly porque necesitamos
//      leerla desde el cliente también, pero sí SameSite=Strict.
// ---------------------------------------------------------------------------

const COOKIE_MAX_AGE = 60 * 60 * 24; // 24h en segundos

export function getToken(): string | null {
  if (typeof window === 'undefined') return null;
  return localStorage.getItem(TOKEN_KEY);
}

export function setToken(token: string): void {
  localStorage.setItem(TOKEN_KEY, token);
  // Espejo en cookie para el Edge Middleware
  document.cookie = `${TOKEN_KEY}=${encodeURIComponent(token)}; path=/; max-age=${COOKIE_MAX_AGE}; SameSite=Strict`;
}

export function clearToken(): void {
  localStorage.removeItem(TOKEN_KEY);
  // Expirar la cookie
  document.cookie = `${TOKEN_KEY}=; path=/; max-age=0; SameSite=Strict`;
}

// ---------------------------------------------------------------------------
// JWT parsing (sin verificar firma — sólo para lectura client-side)
// La verificación real ocurre en el backend en cada request autenticado.
// ---------------------------------------------------------------------------

function parseJwtPayload(token: string): JwtPayload | null {
  try {
    const [, payloadB64] = token.split('.');
    if (!payloadB64) return null;
    // Padding para base64url → base64 estándar
    const padded = payloadB64.replace(/-/g, '+').replace(/_/g, '/');
    const json = atob(padded);
    return JSON.parse(json) as JwtPayload;
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------------------
// Estado de autenticación
// ---------------------------------------------------------------------------

/**
 * Devuelve el usuario actual si hay un token válido (no expirado).
 * Devuelve null si no hay token o si ya expiró.
 */
export function getUser(): AuthUser | null {
  const token = getToken();
  if (!token) return null;

  const payload = parseJwtPayload(token);
  if (!payload) return null;

  const nowSecs = Math.floor(Date.now() / 1000);
  if (payload.exp < nowSecs) {
    // Token expirado — limpiar para evitar estado stale
    clearToken();
    return null;
  }

  return {
    email: payload.sub,
    org_id: payload.org_id,
    role: payload.role,
    exp: payload.exp,
  };
}

/**
 * Devuelve true si hay un token JWT no expirado en storage.
 */
export function isAuthenticated(): boolean {
  return getUser() !== null;
}

// ---------------------------------------------------------------------------
// Tipos de respuesta del backend
// ---------------------------------------------------------------------------

export interface AuthResponse {
  token: string;
  user_id: string;
  email: string;
  org_id: string;
  role: string;
}

export interface ApiErrorResponse {
  error: string;
}
