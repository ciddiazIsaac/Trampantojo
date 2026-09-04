'use client';

import { forwardRef, InputHTMLAttributes, useState } from 'react';
import { Eye, EyeOff } from 'lucide-react';

interface AuthInputProps extends InputHTMLAttributes<HTMLInputElement> {
  label: string;
  error?: string;
  /** Si es true, agrega el toggle de visibilidad (para campos password) */
  isPassword?: boolean;
}

const AuthInput = forwardRef<HTMLInputElement, AuthInputProps>(
  ({ label, error, isPassword = false, type, id, ...props }, ref) => {
    const [showPassword, setShowPassword] = useState(false);

    const inputType = isPassword ? (showPassword ? 'text' : 'password') : type;

    return (
      <div className="auth-field">
        <label htmlFor={id} className="auth-label">
          {label}
        </label>
        <div className="auth-input-wrapper">
          <input
            ref={ref}
            id={id}
            type={inputType}
            className={`auth-input${error ? ' auth-input-error' : ''}${isPassword ? ' auth-input-password' : ''}`}
            aria-invalid={!!error}
            aria-describedby={error ? `${id}-error` : undefined}
            {...props}
          />
          {isPassword && (
            <button
              type="button"
              className="auth-input-eye"
              onClick={() => setShowPassword((v) => !v)}
              aria-label={showPassword ? 'Ocultar contraseña' : 'Mostrar contraseña'}
              tabIndex={-1}
            >
              {showPassword ? (
                <EyeOff className="w-4 h-4" aria-hidden="true" />
              ) : (
                <Eye className="w-4 h-4" aria-hidden="true" />
              )}
            </button>
          )}
        </div>
        {error && (
          <p id={`${id}-error`} className="auth-field-error" role="alert">
            {error}
          </p>
        )}
      </div>
    );
  }
);

AuthInput.displayName = 'AuthInput';

export default AuthInput;
