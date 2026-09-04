/**
 * middleware.ts — Next.js Edge Middleware
 *
 * Protege rutas que requieren autenticación y redirige usuarios ya autenticados
 * fuera de las páginas de auth.
 *
 * Lógica de redirección:
 *   - /dashboard/* sin token → /login
 *   - /login | /register | /forgot-password con token válido → /dashboard
 *
 * Nota: el middleware corre en el Edge Runtime (sin acceso a Node.js APIs).
 * No podemos verificar la firma del JWT aquí — sólo chequeamos presencia y
 * expiración. La verificación real ocurre en cada request al backend Rust.
 */

import { NextRequest, NextResponse } from 'next/server';

const TOKEN_KEY = 'trampantojo_token';

/** Rutas que requieren autenticación */
const PROTECTED_PATHS = ['/dashboard'];

/** Rutas de auth que redirigen a /dashboard si ya estás autenticado */
const AUTH_PATHS = ['/login', '/register', '/forgot-password'];

function getTokenFromRequest(request: NextRequest): string | null {
  // Intentar cookie primero (para compatibilidad futura), luego no hay otra forma
  // en Edge Runtime — localStorage no es accesible. El token JWT viaja como cookie
  // cuando el usuario navega entre páginas (set por el cliente vía JS en el primer
  // request post-login, o leído de localStorage y pasado como cookie en futuros requests).
  //
  // En la implementación actual usamos localStorage (accesible sólo en el browser),
  // por lo que el middleware no puede leer el token directamente. Usamos una cookie
  // "espejo" que el cliente setea junto con localStorage.
  return request.cookies.get(TOKEN_KEY)?.value ?? null;
}

function isTokenExpired(token: string): boolean {
  try {
    const [, payloadB64] = token.split('.');
    if (!payloadB64) return true;
    const padded = payloadB64.replace(/-/g, '+').replace(/_/g, '/');
    const payload = JSON.parse(atob(padded)) as { exp?: number };
    if (!payload.exp) return true;
    return payload.exp < Math.floor(Date.now() / 1000);
  } catch {
    return true;
  }
}

export function middleware(request: NextRequest) {
  const { pathname } = request.nextUrl;

  const token = getTokenFromRequest(request);
  const isAuthenticated = token !== null && !isTokenExpired(token);

  // Rutas protegidas sin autenticación → /login
  const isProtected = PROTECTED_PATHS.some((p) => pathname.startsWith(p));
  if (isProtected && !isAuthenticated) {
    const loginUrl = new URL('/login', request.url);
    // Guardamos el destino para redirigir después del login
    loginUrl.searchParams.set('from', pathname);
    return NextResponse.redirect(loginUrl);
  }

  // Páginas de auth con sesión activa → /dashboard
  const isAuthPage = AUTH_PATHS.some((p) => pathname === p);
  if (isAuthPage && isAuthenticated) {
    return NextResponse.redirect(new URL('/dashboard', request.url));
  }

  return NextResponse.next();
}

export const config = {
  matcher: [
    /*
     * Aplicar middleware a todas las rutas excepto:
     * - _next/static (assets estáticos)
     * - _next/image (optimización de imágenes)
     * - favicon.ico
     * - api/* (rutas de Next.js API / proxy al backend)
     */
    '/((?!_next/static|_next/image|favicon.ico|api/).*)',
  ],
};
