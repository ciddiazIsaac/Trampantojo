'use client';

import { useState, useId } from 'react';
import { useRouter } from 'next/navigation';
import Link from 'next/link';
import { ArrowRight, AlertCircle } from 'lucide-react';
import AuthLayout from '../components/auth/AuthLayout';
import AuthInput from '../components/auth/AuthInput';
import PasswordStrength from '../components/auth/PasswordStrength';
import { setToken } from '../../lib/auth';
import type { AuthResponse, ApiErrorResponse } from '../../lib/auth';

interface FormErrors {
  email?: string;
  password?: string;
  confirm?: string;
}

function validateForm(email: string, password: string, confirm: string): FormErrors {
  const errors: FormErrors = {};
  if (!email) {
    errors.email = 'El email es requerido';
  } else if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) {
    errors.email = 'Ingresá un email válido';
  }
  if (!password) {
    errors.password = 'La contraseña es requerida';
  } else if (password.length < 8) {
    errors.password = 'Debe tener al menos 8 caracteres';
  }
  if (!confirm) {
    errors.confirm = 'Confirmá tu contraseña';
  } else if (password !== confirm) {
    errors.confirm = 'Las contraseñas no coinciden';
  }
  return errors;
}

export default function RegisterPage() {
  const router = useRouter();

  const emailId = useId();
  const passwordId = useId();
  const confirmId = useId();

  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [fieldErrors, setFieldErrors] = useState<FormErrors>({});
  const [serverError, setServerError] = useState('');
  const [loading, setLoading] = useState(false);

  const clearFieldError = (field: keyof FormErrors) =>
    setFieldErrors((p) => ({ ...p, [field]: undefined }));

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setServerError('');

    const errors = validateForm(email, password, confirm);
    if (Object.keys(errors).length > 0) {
      setFieldErrors(errors);
      return;
    }
    setFieldErrors({});

    setLoading(true);
    try {
      const res = await fetch('/api/auth/register', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email, password }),
      });

      if (!res.ok) {
        const data: ApiErrorResponse = await res.json().catch(() => ({ error: 'Error desconocido' }));
        throw new Error(data.error || 'Error al crear la cuenta');
      }

      const data: AuthResponse = await res.json();
      setToken(data.token);
      router.push('/dashboard');
    } catch (err: unknown) {
      setServerError(err instanceof Error ? err.message : 'Error de conexión. Intentá nuevamente.');
    } finally {
      setLoading(false);
    }
  };

  return (
    <AuthLayout
      title="Crear cuenta"
      subtitle="Registrate para obtener acceso a la API de Trampantojo."
    >
      {serverError && (
        <div className="auth-alert auth-alert-error" role="alert">
          <AlertCircle className="w-4 h-4 flex-shrink-0" aria-hidden="true" />
          <span>{serverError}</span>
        </div>
      )}

      <form onSubmit={handleSubmit} className="auth-form" noValidate>
        <AuthInput
          id={emailId}
          label="Email"
          type="email"
          autoComplete="email"
          placeholder="vos@ejemplo.com"
          value={email}
          onChange={(e) => { setEmail(e.target.value); clearFieldError('email'); }}
          error={fieldErrors.email}
          required
        />

        <div className="auth-field">
          <AuthInput
            id={passwordId}
            label="Contraseña"
            isPassword
            autoComplete="new-password"
            placeholder="Mínimo 8 caracteres"
            value={password}
            onChange={(e) => { setPassword(e.target.value); clearFieldError('password'); }}
            error={fieldErrors.password}
            required
          />
          <PasswordStrength password={password} />
        </div>

        <AuthInput
          id={confirmId}
          label="Confirmar contraseña"
          isPassword
          autoComplete="new-password"
          placeholder="Repetí tu contraseña"
          value={confirm}
          onChange={(e) => { setConfirm(e.target.value); clearFieldError('confirm'); }}
          error={fieldErrors.confirm}
          required
        />

        <button
          type="submit"
          disabled={loading}
          className="auth-submit-btn"
          id="register-submit-btn"
        >
          {loading ? (
            <span className="auth-spinner" aria-hidden="true" />
          ) : (
            <>
              Crear cuenta
              <ArrowRight className="w-4 h-4" aria-hidden="true" />
            </>
          )}
          {loading && <span className="sr-only">Creando cuenta…</span>}
        </button>
      </form>

      <p className="auth-switch-text">
        ¿Ya tenés cuenta?{' '}
        <Link href="/login" className="auth-switch-link">
          Iniciá sesión
        </Link>
      </p>
    </AuthLayout>
  );
}
