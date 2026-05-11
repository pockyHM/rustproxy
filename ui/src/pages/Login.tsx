import { FormEvent, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useAuth } from '../auth/AuthContext';
import { login as loginApi } from '../api/client';
import { useI18n } from '../i18n';

export default function Login() {
  const { t } = useI18n();
  const { login } = useAuth();
  const navigate = useNavigate();
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setError(null);
    setIsSubmitting(true);

    try {
      const response = await loginApi(username, password);
      const data = response.data?.data ?? response.data;
      login(data.token);
      navigate('/');
    } catch (err: unknown) {
      const msg =
        err instanceof Error ? err.message : t.auth.loginFailed;
      setError(msg);
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <div className="login-page">
      <div className="login-card">
        <h1 className="login-card__title">{t.app.title}</h1>
        <p className="login-card__subtitle">{t.app.tag}</p>

        {error && (
          <p className="message message--error" role="alert">
            {error}
          </p>
        )}

        <form onSubmit={handleSubmit}>
          <div className="form-group">
            <label className="field-label" htmlFor="login-username">
              {t.auth.username}
            </label>
            <input
              id="login-username"
              type="text"
              value={username}
              onChange={(event) => setUsername(event.target.value)}
              className="field-input"
              required
              autoComplete="username"
            />
          </div>

          <div className="form-group">
            <label className="field-label" htmlFor="login-password">
              {t.auth.password}
            </label>
            <input
              id="login-password"
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              className="field-input"
              required
              autoComplete="current-password"
            />
          </div>

          <button
            type="submit"
            className="btn-primary"
            disabled={isSubmitting}
            style={{ width: '100%', marginTop: 'var(--space-4)' }}
          >
            {isSubmitting ? t.common.loading : t.auth.login}
          </button>
        </form>
      </div>
    </div>
  );
}
