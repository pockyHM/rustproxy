import { FormEvent, useEffect, useMemo, useState } from 'react';
import { dump, load } from 'js-yaml';
import { getConfig, updateConfig } from '../api/client';
import { useI18n } from '../i18n';

type ApiResponse<T> = {
  success: boolean;
  data: T;
};

const unwrapApiData = <T,>(payload: T | ApiResponse<T>): T => {
  if (payload && typeof payload === 'object' && 'data' in payload) {
    return (payload as ApiResponse<T>).data;
  }
  return payload as T;
};

function ConfigEditor() {
  const { t } = useI18n();
  const [yamlText, setYamlText] = useState('');
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const errorId = 'config-error';
  const successId = 'config-success';

  useEffect(() => {
    let isMounted = true;

    const loadConfig = async () => {
      setIsLoading(true);
      setError(null);
      setMessage(null);

      try {
        const response = await getConfig();
        const config = unwrapApiData<unknown>(response.data);

        if (isMounted) {
          setYamlText(dump(config, { noRefs: true }));
        }
      } catch (loadError) {
        if (isMounted) {
          setError(loadError instanceof Error ? loadError.message : t.config.loadFail);
        }
      } finally {
        if (isMounted) {
          setIsLoading(false);
        }
      }
    };

    void loadConfig();

    return () => {
      isMounted = false;
    };
  }, [t]);

  const stats = useMemo(() => {
    const lines = yamlText.split('\n').length;
    let rules = 0;
    let upstreams = 0;
    try {
      const parsed = load(yamlText) as Record<string, unknown>;
      const r = parsed?.rules;
      const u = parsed?.upstreams;
      if (Array.isArray(r)) rules = r.length;
      else if (r && typeof r === 'object') rules = Object.keys(r).length;
      if (Array.isArray(u)) upstreams = u.length;
      else if (u && typeof u === 'object') upstreams = Object.keys(u).length;
    } catch {
      // YAML parse error, show 0
    }
    return { lines, rules, upstreams };
  }, [yamlText]);

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setIsSaving(true);
    setError(null);
    setMessage(null);

    try {
      const parsedConfig = load(yamlText);
      await updateConfig(parsedConfig);
      setMessage(t.config.saved);
    } catch (saveError) {
      setError(saveError instanceof Error ? saveError.message : t.common.saveFail);
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <div className="page">
      <div className="page-header">
        <div>
          <h1 className="page-header__title">{t.config.title}</h1>
          <p className="page-header__desc">{t.config.desc}</p>
        </div>
      </div>

      {isLoading && <div className="loading-state">{t.common.loading}</div>}

      {message && (
        <p id={successId} className="message message--success" role="status" aria-live="polite">
          {message}
        </p>
      )}
      {error && (
        <p id={errorId} className="message message--error" role="alert" aria-live="assertive">
          {error}
        </p>
      )}

      {!isLoading && (
        <form onSubmit={handleSubmit}>
          {/* Config Stats */}
          <div className="config-stats" style={{ marginBottom: 'var(--space-4)' }}>
            <span className="config-stat"><strong>{stats.lines}</strong> {t.config.lines}</span>
            <span className="config-stat"><strong>{stats.rules}</strong> {t.config.rules}</span>
            <span className="config-stat"><strong>{stats.upstreams}</strong> {t.config.upstreams}</span>
          </div>

          <div className="form-section">
            <h2 className="form-section__title">{t.config.rawYaml}</h2>
            <p className="field-hint" style={{ marginBottom: 'var(--space-4)' }}>{t.config.yamlHint}</p>
            <div className="form-group">
              <textarea
                id="yaml-editor"
                value={yamlText}
                onChange={(event) => setYamlText(event.target.value)}
                className="field-textarea"
                rows={24}
                spellCheck={false}
                aria-describedby={error ? errorId : undefined}
                aria-invalid={error ? 'true' : undefined}
              />
            </div>
          </div>

          <div className="form-actions">
            <button type="submit" className="btn-primary" disabled={isSaving} aria-busy={isSaving}>
              {isSaving ? t.config.saving : t.config.saveConfig}
            </button>
          </div>
        </form>
      )}
    </div>
  );
}

export default ConfigEditor;
