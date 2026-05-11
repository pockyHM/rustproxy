import { FormEvent, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { setup } from '../api/client';
import { useI18n } from '../i18n';

export default function Setup() {
  const { t } = useI18n();
  const navigate = useNavigate();
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setError(null);

    if (password !== confirmPassword) {
      setError(t.auth.passwordMismatch);
      return;
    }
    if (password.length < 6) {
      setError(t.auth.passwordTooShort);
      return;
    }

    setIsSubmitting(true);

    try {
      await setup(username, password);
      navigate('/login');
    } catch (err: unknown) {
      const msg =
        err instanceof Error ? err.message : t.auth.setupFailed;
      setError(msg);
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <div className="login-page">
      <div className="login-card">
        <h1 className="login-card__title">{t.app.title}</h1>
        <p className="login-card__subtitle">{t.auth.setupDesc}</p>

        {error && (
          <p className="message message--error" role="alert">
            {error}
          </p>
        )}

        <form onSubmit={handleSubmit}>
          <div className="form-group">
            <label className="field-label" htmlFor="setup-username">
              {t.auth.username}
            </label>
            <input
              id="setup-username"
              type="text"
              value={username}
              onChange={(event) => setUsername(event.target.value)}
              className="field-input"
              required
              autoComplete="username"
            />
          </div>

          <div className="form-group">
            <label className="field-label" htmlFor="setup-password">
              {t.auth.password}
            </label>
            <input
              id="setup-password"
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              className="field-input"
              required
              minLength={6}
              autoComplete="new-password"
            />
            <p className="field-hint">{t.auth.passwordHint}</p>
          </div>

          <div className="form-group">
            <label className="field-label" htmlFor="setup-confirm">
              {t.auth.confirmPassword}
            </label>
            <input
              id="setup-confirm"
              type="password"
              value={confirmPassword}
              onChange={(event) => setConfirmPassword(event.target.value)}
              className="field-input"
              required
              autoComplete="new-password"
            />
          </div>

          <button
            type="submit"
            className="btn-primary"
            disabled={isSubmitting}
            style={{ width: '100%', marginTop: 'var(--space-4)' }}
          >
            {isSubmitting ? t.common.loading : t.auth.createAdmin}
          </button>
        </form>
      </div>
    </div>
  );
}
