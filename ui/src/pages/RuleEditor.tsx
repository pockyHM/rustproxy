import { FormEvent, useEffect, useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Link, useNavigate, useParams } from 'react-router-dom';
import { createRule, getRules, getUpstreams, updateRule } from '../api/client';
import { useI18n } from '../i18n';
import ConditionBuilder from '../components/ConditionBuilder';
import type { RuleCondition } from '../types/conditions';

type Rule = {
  id: string;
  name: string;
  priority: number;
  conditions: RuleCondition[];
  upstream: string;
  weight: number;
};

type Upstream = {
  name: string;
  targets: { url: string; weight: number }[];
};

type ApiResponse<T> = {
  success: boolean;
  data: T;
};

type RuleFormState = {
  id: string;
  name: string;
  priority: string;
  conditions: RuleCondition[];
  upstream: string;
  weight: string;
};

const unwrapApiData = <T,>(payload: T | ApiResponse<T>): T => {
  if (payload && typeof payload === 'object' && 'data' in payload) {
    return (payload as ApiResponse<T>).data;
  }
  return payload as T;
};

const createEmptyForm = (): RuleFormState => ({
  id: '',
  name: '',
  priority: '0',
  conditions: [],
  upstream: '',
  weight: '100',
});

const sanitizeConditions = (conditions: RuleCondition[]): RuleCondition[] =>
  conditions.map((condition) => {
    const next: RuleCondition = {
      type: condition.type,
      operator: condition.operator,
    };
    if (condition.type === 'jwt') {
      next.claim_path = condition.claim_path ?? '';
    } else {
      next.key = condition.key ?? '';
    }
    if (condition.operator !== 'exists') {
      next.value = condition.value ?? '';
    }
    return next;
  });

function RuleEditor() {
  const { id } = useParams();
  const navigate = useNavigate();
  const { t } = useI18n();
  const isNewRule = id === undefined || id === 'new';
  const [form, setForm] = useState<RuleFormState>(createEmptyForm);
  const [saveError, setSaveError] = useState<string | null>(null);

  const errorId = 'rule-save-error';

  const rulesQuery = useQuery({
    queryKey: ['rules'],
    queryFn: async () => {
      const response = await getRules();
      return unwrapApiData<Rule[]>(response.data);
    },
    enabled: !isNewRule,
  });

  const upstreamsQuery = useQuery({
    queryKey: ['upstreams'],
    queryFn: async () => {
      const response = await getUpstreams();
      return unwrapApiData<Upstream[]>(response.data);
    },
  });

  const existingRule = useMemo(() => {
    if (isNewRule || !rulesQuery.data) return undefined;
    return rulesQuery.data.find((rule) => rule.id === id);
  }, [id, isNewRule, rulesQuery.data]);

  useEffect(() => {
    if (existingRule) {
      setForm({
        id: existingRule.id,
        name: existingRule.name,
        priority: String(existingRule.priority),
        conditions: existingRule.conditions ?? [],
        upstream: existingRule.upstream,
        weight: String(existingRule.weight),
      });
    }
  }, [existingRule]);

  useEffect(() => {
    if (isNewRule && !form.upstream && upstreamsQuery.data && upstreamsQuery.data.length > 0) {
      setForm((currentForm) => ({ ...currentForm, upstream: upstreamsQuery.data[0].name }));
    }
  }, [form.upstream, isNewRule, upstreamsQuery.data]);

  const updateField = (field: keyof RuleFormState, value: string | RuleCondition[]) => {
    setForm((currentForm) => ({ ...currentForm, [field]: value }));
  };

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setSaveError(null);

    const rule: Rule = {
      id: isNewRule ? form.id.trim() : id ?? form.id,
      name: form.name.trim(),
      priority: Number(form.priority),
      conditions: sanitizeConditions(form.conditions),
      upstream: form.upstream,
      weight: Number(form.weight),
    };

    try {
      if (isNewRule) {
        await createRule(rule);
      } else {
        await updateRule(rule.id, rule);
      }
      navigate('/rules');
    } catch (error) {
      setSaveError(error instanceof Error ? error.message : t.common.saveFail);
    }
  };

  if (!isNewRule && rulesQuery.isLoading) {
    return (
      <div className="page">
        <div className="page-header">
          <div>
            <h1 className="page-header__title">{t.ruleEditor.editTitle}</h1>
          </div>
        </div>
        <div className="loading-state">{t.common.loading}</div>
      </div>
    );
  }

  if (!isNewRule && rulesQuery.data && !existingRule) {
    return (
      <div className="page">
        <div className="page-header">
          <div>
            <h1 className="page-header__title">{t.ruleEditor.editTitle}</h1>
          </div>
        </div>
        <div className="empty-state">
          {t.ruleEditor.notFound}{' '}
          <Link to="/rules">
            {t.common.returnToList.replace('{page}', t.ruleEditor.ruleList)}
          </Link>
        </div>
      </div>
    );
  }

  const hasConditions = form.conditions.length > 0;

  return (
    <div className="page">
      <div className="page-header">
        <div>
          <h1 className="page-header__title">
            {isNewRule ? t.ruleEditor.createTitle : t.ruleEditor.editTitle}
          </h1>
          <p className="page-header__desc">
            {isNewRule ? t.ruleEditor.createDesc : t.ruleEditor.editDesc}
          </p>
        </div>
      </div>

      {rulesQuery.isError && <div className="message message--error">{t.ruleEditor.loadFail}</div>}
      {upstreamsQuery.isError && <div className="message message--error">{t.ruleEditor.upstreamLoadFail}</div>}
      {saveError && (
        <p id={errorId} className="message message--error" role="alert" aria-live="assertive">
          {saveError}
        </p>
      )}

      <form onSubmit={handleSubmit} aria-describedby={saveError ? errorId : undefined}>
        <div className="form-section" style={{ marginBottom: 'var(--space-5)' }}>
          <h2 className="form-section__title">{t.ruleEditor.general}</h2>

          {isNewRule && (
            <div className="form-group">
              <label className="field-label" htmlFor="rule-id">{t.ruleEditor.ruleId}</label>
              <input
                id="rule-id"
                type="text"
                value={form.id}
                onChange={(event) => updateField('id', event.target.value)}
                className="field-input"
                required
                style={{ maxWidth: '24rem' }}
              />
              <p className="field-hint">{t.ruleEditor.ruleIdHint}</p>
            </div>
          )}

          <div className="form-group">
            <label className="field-label" htmlFor="rule-name">{t.ruleEditor.name}</label>
            <input
              id="rule-name"
              type="text"
              value={form.name}
              onChange={(event) => updateField('name', event.target.value)}
              className="field-input"
              required
              style={{ maxWidth: '32rem' }}
            />
            <p className="field-hint">{t.ruleEditor.nameHint}</p>
          </div>

          <div style={{ display: 'flex', gap: 'var(--space-4)', flexWrap: 'wrap' }}>
            <div className="form-group" style={{ flex: '1 1 10rem', maxWidth: '14rem' }}>
              <label className="field-label" htmlFor="rule-priority">{t.ruleEditor.priority}</label>
              <input
                id="rule-priority"
                type="number"
                value={form.priority}
                onChange={(event) => updateField('priority', event.target.value)}
                className="field-input"
                required
              />
              <p className="field-hint">{t.ruleEditor.priorityHint}</p>
            </div>
            <div className="form-group" style={{ flex: '1 1 10rem', maxWidth: '14rem' }}>
              <label className="field-label" htmlFor="rule-weight">{t.ruleEditor.weight}</label>
              <input
                id="rule-weight"
                type="number"
                min="0"
                value={form.weight}
                onChange={(event) => updateField('weight', event.target.value)}
                className="field-input"
                required
              />
              <p className="field-hint">{t.ruleEditor.weightHint}</p>
            </div>
          </div>
        </div>

        <div style={{ marginBottom: 'var(--space-5)' }}>
          <ConditionBuilder conditions={form.conditions} onChange={(conditions) => updateField('conditions', conditions)} />
        </div>

        <div className="form-section" style={{ marginBottom: 'var(--space-5)' }}>
          <h2 className="form-section__title">{t.ruleEditor.upstreamSection}</h2>
          <div className="form-group">
            <label className="field-label" htmlFor="rule-upstream">{t.ruleEditor.targetUpstream}</label>
            <select
              id="rule-upstream"
              value={form.upstream}
              onChange={(event) => updateField('upstream', event.target.value)}
              className="field-select"
              required
              style={{ maxWidth: '24rem' }}
            >
              <option value="" disabled>{t.ruleEditor.selectUpstream}</option>
              {upstreamsQuery.data?.map((upstream) => (
                <option key={upstream.name} value={upstream.name}>{upstream.name}</option>
              ))}
            </select>
          </div>
        </div>

        {/* Preview */}
        <div className="preview-panel" style={{ marginBottom: 'var(--space-5)' }}>
          <h3 className="preview-panel__title">{t.ruleEditor.preview}</h3>
          <p className="preview-panel__text">
            {hasConditions
              ? <>
                  {t.ruleEditor.previewMatches}{' '}
                  {form.conditions.map((c, i) => (
                    <span key={i}>
                      <strong>{c.type === 'jwt' ? c.claim_path : c.key}</strong> {c.operator}
                      {c.operator !== 'exists' && c.value ? ` "${c.value}"` : ''}
                      {i < form.conditions.length - 1 && ` ${t.ruleEditor.previewAnd} `}
                    </span>
                  ))}
                  {' '}{t.ruleEditor.previewRoutes}{' '}
                  <strong>{form.upstream || '?'}</strong>{' '}
                  {t.ruleEditor.previewWithWeight.replace('{weight}', form.weight)}
                </>
              : <>
                  {t.ruleEditor.previewAlways}{' '}
                  <strong>{form.upstream || '?'}</strong>{' '}
                  {t.ruleEditor.previewWithWeight.replace('{weight}', form.weight)}
                </>
            }
            <br />
            <span style={{ fontSize: 'var(--text-xs)', color: 'var(--color-gray-400)' }}>
              {t.ruleEditor.previewPriority.replace('{priority}', form.priority)}
            </span>
          </p>
        </div>

        <div className="form-actions">
          <button type="submit" className="btn-primary">{t.ruleEditor.saveRule}</button>
          <Link to="/rules" className="btn-ghost">{t.common.cancel}</Link>
        </div>
      </form>
    </div>
  );
}

export default RuleEditor;
