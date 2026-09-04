'use client';

import { useState, useId } from 'react';
import Link from 'next/link';
import { ArrowRight, CheckCircle2, AlertCircle, Mail } from 'lucide-react';
import AuthLayout from '../components/auth/AuthLayout';
import AuthInput from '../components/auth/AuthInput';

type PageState = 'idle' | 'loading' | 'success' | 'error';

interface ApiErrorResponse {
  error: string;
}

export default function ForgotPasswordPage() {
  const emailId = useId();

  const [email, setEmail] = useState('');
  const [emailError, setEmailError] = useState('');
  const [serverError, setServerError] = useState('');
  const [state, setState] = useState<PageState>('idle');

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setServerError('');
    setEmailError('');

    if (!email) {
      setEmailError('El email es requerido');
      return;
    }
    if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) {
      setEmailError('Ingresá un email válido');
      return;
    }

    setState('loading');
    try {
      const res = await fetch('/api/auth/forgot-password', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email }),
      });

      if (!res.ok) {
        // En caso de error real del servidor (5xx), mostramos error
        const data: ApiErrorResponse = await res.json().catch(() => ({ error: 'Error desconocido' }));
        throw new Error(data.error || 'Error al procesar la solicitud');
      }

      // Éxito — el backend siempre responde 200 (no filtramos existencia de email)
      setState('success');
    } catch (err: unknown) {
      setServerError(err instanceof Error ? err.message : 'Error de conexión. Intentá nuevamente.');
      setState('error');
    }
  };

  if (state === 'success') {
    return (
      <AuthLayout
        title="Revisá tu email"
        subtitle="Si ese email existe en nuestra base de datos, recibirás un enlace de recuperación en breve."
      >
        <div className="auth-success-state">
          <div className="auth-success-icon-wrapper" aria-hidden="true">
            <CheckCircle2 className="w-10 h-10" />
          </div>
          <div className="auth-success-body">
            <p className="auth-success-detail">
              Enviamos las instrucciones a{' '}
              <strong className="auth-success-email">{email}</strong>.
              Revisá también tu carpeta de spam.
            </p>
          </div>
        </div>

        <div className="auth-form" style={{ marginTop: '1.5rem' }}>
          <Link href="/login" className="auth-submit-btn" style={{ textDecoration: 'none', display: 'flex', justifyContent: 'center' }}>
            Volver al login
            <ArrowRight className="w-4 h-4 ml-2" aria-hidden="true" />
          </Link>
        </div>
      </AuthLayout>
    );
  }

  return (
    <AuthLayout
      title="Recuperar contraseña"
      subtitle="Ingresá tu email y te enviaremos un enlace para restablecer tu contraseña."
    >
      {(state === 'error' || serverError) && (
        <div className="auth-alert auth-alert-error" role="alert">
          <AlertCircle className="w-4 h-4 flex-shrink-0" aria-hidden="true" />
          <span>{serverError || 'Ocurrió un error. Intentá nuevamente.'}</span>
        </div>
      )}

      <form onSubmit={handleSubmit} className="auth-form" noValidate>
        <div className="auth-field">
          <AuthInput
            id={emailId}
            label="Email"
            type="email"
            autoComplete="email"
            placeholder="vos@ejemplo.com"
            value={email}
            onChange={(e) => { setEmail(e.target.value); setEmailError(''); }}
            error={emailError}
            required
          />
        </div>

        <button
          type="submit"
          disabled={state === 'loading'}
          className="auth-submit-btn"
          id="forgot-password-submit-btn"
        >
          {state === 'loading' ? (
            <span className="auth-spinner" aria-hidden="true" />
          ) : (
            <>
              <Mail className="w-4 h-4" aria-hidden="true" />
              Enviar enlace de recuperación
            </>
          )}
          {state === 'loading' && <span className="sr-only">Enviando…</span>}
        </button>
      </form>

      <p className="auth-switch-text">
        ¿Recordaste tu contraseña?{' '}
        <Link href="/login" className="auth-switch-link">
          Iniciá sesión
        </Link>
      </p>
    </AuthLayout>
  );
}
