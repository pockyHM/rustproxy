import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { getConfig, updateConfig } from '../api/client';
import { useI18n } from '../i18n';

type ProxyConfig = {
  listen?: string;
  skip_ssl?: boolean;
  connect_timeout?: number;
  request_timeout?: number;
  [key: string]: unknown;
};

function Settings() {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saveSuccess, setSaveSuccess] = useState(false);

  const configQuery = useQuery({
    queryKey: ['config'],
    queryFn: async () => {
      const response = await getConfig();
      const data = response.data?.data ?? response.data;
      return data as ProxyConfig;
    },
  });

  const saveField = async (updates: Partial<ProxyConfig>) => {
    const data = configQuery.data;
    if (!data) return;
    setSaveError(null);
    setSaveSuccess(false);
    try {
      const updated = { ...data, ...updates };
      await updateConfig(updated);
      queryClient.setQueryData(['config'], updated);
      setSaveSuccess(true);
      setTimeout(() => setSaveSuccess(false), 2000);
    } catch (err) {
      setSaveError(err instanceof Error ? err.message : t.common.saveFail);
    }
  };

  if (configQuery.isLoading) {
    return (
      <div className="page">
        <div className="page-header">
          <h1 className="page-header__title">{t.settings.title}</h1>
        </div>
        <div className="loading-state">{t.common.loading}</div>
      </div>
    );
  }

  if (configQuery.isError) {
    return (
      <div className="page">
        <div className="page-header">
          <h1 className="page-header__title">{t.settings.title}</h1>
        </div>
        <div className="message message--error">{t.common.loadFail}</div>
      </div>
    );
  }

  const config = configQuery.data;

  return (
    <div className="page">
      <div className="page-header">
        <div>
          <h1 className="page-header__title">{t.settings.title}</h1>
          <p className="page-header__desc">{t.settings.desc}</p>
        </div>
      </div>

      {saveError && (
        <p className="message message--error" role="alert">{saveError}</p>
      )}
      {saveSuccess && (
        <p className="message message--success" role="status">{t.common.saveSuccess}</p>
      )}

      {/* Listen Address */}
      <div className="form-section" style={{ marginBottom: 'var(--space-5)' }}>
        <h2 className="form-section__title">{t.settings.listenSection}</h2>
        <div className="form-group">
          <label className="field-label" htmlFor="setting-listen">{t.settings.listenAddr}</label>
          <input
            id="setting-listen"
            type="text"
            defaultValue={config?.listen ?? '127.0.0.1:3000'}
            onBlur={(e) => saveField({ listen: e.target.value })}
            onKeyDown={(e) => { if (e.key === 'Enter') e.currentTarget.blur(); }}
            className="field-input"
            style={{ maxWidth: '24rem' }}
          />
          <p className="field-hint">{t.settings.listenHint}</p>
        </div>
      </div>

      {/* SSL */}
      <div className="form-section" style={{ marginBottom: 'var(--space-5)' }}>
        <h2 className="form-section__title">{t.settings.sslSection}</h2>
        <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--space-3)' }}>
          <button
            type="button"
            className={config?.skip_ssl ? 'btn-danger' : 'btn-secondary'}
            onClick={() => saveField({ skip_ssl: !config?.skip_ssl })}
          >
            {config?.skip_ssl ? t.settings.sslOff : t.settings.sslOn}
          </button>
          <span style={{ fontSize: 'var(--text-sm)', color: 'var(--color-gray-500)' }}>
            {config?.skip_ssl ? t.settings.sslOffDesc : t.settings.sslOnDesc}
          </span>
        </div>
      </div>

      {/* Timeouts */}
      <div className="form-section" style={{ marginBottom: 'var(--space-5)' }}>
        <h2 className="form-section__title">{t.settings.timeoutSection}</h2>
        <div className="form-group">
          <label className="field-label" htmlFor="setting-connect-timeout">{t.settings.connectTimeout}</label>
          <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--space-2)' }}>
            <input
              id="setting-connect-timeout"
              type="number"
              min="0"
              defaultValue={config?.connect_timeout ?? 10}
              onBlur={(e) => saveField({ connect_timeout: parseInt(e.target.value, 10) || 0 })}
              onKeyDown={(e) => { if (e.key === 'Enter') e.currentTarget.blur(); }}
              className="field-input"
              style={{ maxWidth: '8rem' }}
            />
            <span style={{ color: 'var(--color-gray-400)' }}>s</span>
          </div>
          <p className="field-hint">{t.settings.connectTimeoutHint}</p>
        </div>
        <div className="form-group">
          <label className="field-label" htmlFor="setting-request-timeout">{t.settings.requestTimeout}</label>
          <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--space-2)' }}>
            <input
              id="setting-request-timeout"
              type="number"
              min="0"
              defaultValue={config?.request_timeout ?? 60}
              onBlur={(e) => saveField({ request_timeout: parseInt(e.target.value, 10) || 0 })}
              onKeyDown={(e) => { if (e.key === 'Enter') e.currentTarget.blur(); }}
              className="field-input"
              style={{ maxWidth: '8rem' }}
            />
            <span style={{ color: 'var(--color-gray-400)' }}>s</span>
          </div>
          <p className="field-hint">{t.settings.requestTimeoutHint}</p>
        </div>
      </div>
    </div>
  );
}

export default Settings;
