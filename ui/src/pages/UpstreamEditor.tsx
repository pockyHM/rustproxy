import { FormEvent, useEffect, useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Link, useNavigate, useParams } from 'react-router-dom';
import { createUpstream, getUpstreams, updateUpstream } from '../api/client';
import { useI18n } from '../i18n';

type Target = {
  url: string;
  weight: number;
};

type Upstream = {
  name: string;
  targets: Target[];
};

type ApiResponse<T> = {
  success: boolean;
  data: T;
};

type TargetFormState = {
  url: string;
  weight: string;
};

type UpstreamFormState = {
  name: string;
  targets: TargetFormState[];
};

const unwrapApiData = <T,>(payload: T | ApiResponse<T>): T => {
  if (payload && typeof payload === 'object' && 'data' in payload) {
    return (payload as ApiResponse<T>).data;
  }
  return payload as T;
};

const WEIGHT_COLORS = ['var(--color-link)', 'var(--color-success)', 'var(--color-warning)', 'var(--color-gray-400)'];

const createEmptyForm = (): UpstreamFormState => ({
  name: '',
  targets: [{ url: '', weight: '100' }],
});

function UpstreamEditor() {
  const { id } = useParams();
  const navigate = useNavigate();
  const { t } = useI18n();
  const isNewUpstream = id === undefined || id === 'new';
  const [form, setForm] = useState<UpstreamFormState>(createEmptyForm);
  const [saveError, setSaveError] = useState<string | null>(null);

  const errorId = 'upstream-save-error';

  const upstreamsQuery = useQuery({
    queryKey: ['upstreams'],
    queryFn: async () => {
      const response = await getUpstreams();
      return unwrapApiData<Upstream[]>(response.data);
    },
    enabled: !isNewUpstream,
  });

  const existingUpstream = useMemo(() => {
    if (isNewUpstream || !upstreamsQuery.data) return undefined;
    return upstreamsQuery.data.find((upstream) => upstream.name === id);
  }, [id, isNewUpstream, upstreamsQuery.data]);

  useEffect(() => {
    if (existingUpstream) {
      setForm({
        name: existingUpstream.name,
        targets:
          existingUpstream.targets.length > 0
            ? existingUpstream.targets.map((target) => ({
                url: target.url,
                weight: String(target.weight),
              }))
            : [{ url: '', weight: '100' }],
      });
    }
  }, [existingUpstream]);

  const updateName = (name: string) => {
    setForm((currentForm) => ({ ...currentForm, name }));
  };

  const updateTarget = (index: number, field: keyof TargetFormState, value: string) => {
    setForm((currentForm) => ({
      ...currentForm,
      targets: currentForm.targets.map((target, targetIndex) =>
        targetIndex === index ? { ...target, [field]: value } : target,
      ),
    }));
  };

  const addTarget = () => {
    setForm((currentForm) => ({
      ...currentForm,
      targets: [...currentForm.targets, { url: '', weight: '100' }],
    }));
  };

  const removeTarget = (index: number) => {
    setForm((currentForm) => ({
      ...currentForm,
      targets: currentForm.targets.filter((_, targetIndex) => targetIndex !== index),
    }));
  };

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setSaveError(null);

    const upstreamName = isNewUpstream ? form.name.trim() : id ?? form.name.trim();
    const upstream: Upstream = {
      name: upstreamName,
      targets: form.targets.map((target) => ({
        url: target.url.trim(),
        weight: Number(target.weight),
      })),
    };

    try {
      if (isNewUpstream) {
        await createUpstream(upstream);
      } else {
        await updateUpstream(upstreamName, upstream);
      }
      navigate('/upstreams');
    } catch (error) {
      setSaveError(error instanceof Error ? error.message : t.common.saveFail);
    }
  };

  if (!isNewUpstream && upstreamsQuery.isLoading) {
    return (
      <div className="page">
        <div className="page-header">
          <div>
            <h1 className="page-header__title">{t.upstreamEditor.editTitle}</h1>
          </div>
        </div>
        <div className="loading-state">{t.common.loading}</div>
      </div>
    );
  }

  if (!isNewUpstream && upstreamsQuery.data && !existingUpstream) {
    return (
      <div className="page">
        <div className="page-header">
          <div>
            <h1 className="page-header__title">{t.upstreamEditor.editTitle}</h1>
          </div>
        </div>
        <div className="empty-state">
          {t.upstreamEditor.notFound}{' '}
          <Link to="/upstreams">
            {t.common.returnToList.replace('{page}', t.upstreamEditor.upstreamList)}
          </Link>
        </div>
      </div>
    );
  }

  const totalWeight = form.targets.reduce((sum, t) => sum + Number(t.weight || 0), 0);

  return (
    <div className="page">
      <div className="page-header">
        <div>
          <h1 className="page-header__title">
            {isNewUpstream ? t.upstreamEditor.createTitle : t.upstreamEditor.editTitle}
          </h1>
          <p className="page-header__desc">
            {isNewUpstream ? t.upstreamEditor.createDesc : t.upstreamEditor.editDesc}
          </p>
        </div>
      </div>

      {upstreamsQuery.isError && <div className="message message--error">{t.upstreamEditor.loadFail}</div>}
      {saveError && (
        <p id={errorId} className="message message--error" role="alert" aria-live="assertive">
          {saveError}
        </p>
      )}

      <form onSubmit={handleSubmit}>
        <div className="form-section" style={{ marginBottom: 'var(--space-5)' }}>
          <h2 className="form-section__title">{t.upstreamEditor.general}</h2>
          <div className="form-group">
            <label className="field-label" htmlFor="upstream-name">{t.upstreamEditor.upstreamName}</label>
            <input
              id="upstream-name"
              type="text"
              value={form.name}
              onChange={(event) => updateName(event.target.value)}
              className="field-input"
              required
              disabled={!isNewUpstream}
              style={{ maxWidth: '24rem' }}
            />
            <p className="field-hint">{t.upstreamEditor.upstreamNameHint}</p>
          </div>
        </div>

        <div className="form-section" style={{ marginBottom: 'var(--space-5)' }}>
          <h2 className="form-section__title">{t.upstreamEditor.targets}</h2>
          <div style={{ display: 'grid', gap: 'var(--space-3)' }}>
            {form.targets.map((target, index) => (
              <div key={index} className="condition-card" role="group" aria-label={`Target ${index + 1}`}>
                <div style={{ display: 'flex', gap: 'var(--space-3)', flexWrap: 'wrap', alignItems: 'flex-end' }}>
                  <div className="form-group" style={{ flex: '2 1 16rem', marginBottom: 0 }}>
                    <label className="field-label" htmlFor={`target-url-${index}`}>{t.upstreamEditor.url}</label>
                    <input
                      id={`target-url-${index}`}
                      type="url"
                      value={target.url}
                      onChange={(event) => updateTarget(index, 'url', event.target.value)}
                      className="field-input"
                      placeholder={t.upstreamEditor.urlPlaceholder}
                      required
                    />
                    <p className="field-hint">{t.upstreamEditor.urlHint}</p>
                  </div>
                  <div className="form-group" style={{ flex: '1 1 8rem', marginBottom: 0 }}>
                    <label className="field-label" htmlFor={`target-weight-${index}`}>{t.upstreamEditor.weight}</label>
                    <input
                      id={`target-weight-${index}`}
                      type="number"
                      min="0"
                      value={target.weight}
                      onChange={(event) => updateTarget(index, 'weight', event.target.value)}
                      className="field-input"
                      required
                    />
                    <p className="field-hint">{t.upstreamEditor.weightHint}</p>
                  </div>
                  <button
                    type="button"
                    className="btn-danger btn-sm"
                    onClick={() => removeTarget(index)}
                    disabled={form.targets.length === 1}
                    aria-label={`Remove target ${index + 1}`}
                  >
                    {t.common.remove}
                  </button>
                </div>
              </div>
            ))}
          </div>
          <button type="button" className="btn-secondary btn-sm" onClick={addTarget} style={{ marginTop: 'var(--space-4)' }}>
            {t.upstreamEditor.addTarget}
          </button>
        </div>

        {/* Weight distribution preview */}
        {form.targets.length > 1 && totalWeight > 0 && (
          <div className="preview-panel" style={{ marginBottom: 'var(--space-5)' }}>
            <h3 className="preview-panel__title">{t.upstreamEditor.weightPreview}</h3>
            <div className="weight-bar" style={{ marginBottom: 'var(--space-3)' }}>
              {form.targets.map((target, index) => (
                <div
                  key={index}
                  className="weight-bar__segment"
                  style={{
                    width: `${(Number(target.weight || 0) / totalWeight) * 100}%`,
                    background: WEIGHT_COLORS[index % WEIGHT_COLORS.length],
                  }}
                />
              ))}
            </div>
            <div style={{ display: 'flex', gap: 'var(--space-4)', flexWrap: 'wrap' }}>
              {form.targets.map((target, index) => (
                <span key={index} style={{ fontSize: 'var(--text-xs)', color: 'var(--color-gray-500)' }}>
                  <span style={{
                    display: 'inline-block',
                    width: 8,
                    height: 8,
                    borderRadius: '50%',
                    background: WEIGHT_COLORS[index % WEIGHT_COLORS.length],
                    marginRight: 'var(--space-1)',
                    verticalAlign: 'middle',
                  }} />
                  {target.url || `Target ${index + 1}`}: {totalWeight > 0 ? Math.round((Number(target.weight || 0) / totalWeight) * 100) : 0}%
                </span>
              ))}
            </div>
          </div>
        )}

        <div className="form-actions">
          <button type="submit" className="btn-primary">{t.upstreamEditor.saveUpstream}</button>
          <Link to="/upstreams" className="btn-ghost">{t.common.cancel}</Link>
        </div>
      </form>
    </div>
  );
}

export default UpstreamEditor;
