'use client';

interface PasswordStrengthProps {
  password: string;
}

interface StrengthLevel {
  label: string;
  color: string;
  segments: number; // 1-4
}

function getStrength(password: string): StrengthLevel {
  if (!password) return { label: '', color: 'transparent', segments: 0 };

  let score = 0;
  if (password.length >= 8) score++;
  if (password.length >= 12) score++;
  if (/[A-Z]/.test(password) && /[a-z]/.test(password)) score++;
  if (/[0-9]/.test(password)) score++;
  if (/[^A-Za-z0-9]/.test(password)) score++;

  if (score <= 1) return { label: 'Muy débil', color: 'var(--threat-500)', segments: 1 };
  if (score === 2) return { label: 'Débil',     color: '#f59e0b',             segments: 2 };
  if (score === 3) return { label: 'Regular',   color: '#eab308',             segments: 3 };
  if (score === 4) return { label: 'Fuerte',    color: 'var(--safe-500)',      segments: 4 };
  return               { label: 'Muy fuerte', color: 'var(--accent-500)',    segments: 4 };
}

export default function PasswordStrength({ password }: PasswordStrengthProps) {
  const strength = getStrength(password);

  if (!password) return null;

  return (
    <div className="pwd-strength" aria-live="polite" aria-atomic="true">
      <div className="pwd-strength-bars">
        {[1, 2, 3, 4].map((i) => (
          <div
            key={i}
            className="pwd-strength-bar"
            style={{
              background: i <= strength.segments ? strength.color : 'var(--bg-elevated)',
              transition: 'background 0.3s ease',
            }}
          />
        ))}
      </div>
      <span className="pwd-strength-label" style={{ color: strength.color }}>
        {strength.label}
      </span>
    </div>
  );
}
