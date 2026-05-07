import { FormEvent, useEffect, useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Link, useNavigate, useParams } from 'react-router-dom';
import { createRule, getRules, getUpstreams, updateRule } from '../api/client';
import ConditionBuilder from '../components/ConditionBuilder';
import type { RuleCondition } from '../components/ConditionBuilder';

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
      condition_type: condition.condition_type,
      operator: condition.operator,
    };

    if (condition.condition_type === 'Jwt') {
      next.claim_path = condition.claim_path ?? '';
    } else {
      next.key = condition.key ?? '';
    }

    if (condition.operator !== 'Exists') {
      next.value = condition.value ?? '';
    }

    return next;
  });

function RuleEditor() {
  const { id } = useParams();
  const navigate = useNavigate();
  const isNewRule = id === undefined || id === 'new';
  const [form, setForm] = useState<RuleFormState>(createEmptyForm);
  const [saveError, setSaveError] = useState<string | null>(null);

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
    if (isNewRule || !rulesQuery.data) {
      return undefined;
    }

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
      setSaveError(error instanceof Error ? error.message : 'Unable to save rule.');
    }
  };

  if (!isNewRule && rulesQuery.isLoading) {
    return (
      <section>
        <h2>Edit Rule</h2>
        <p>Loading rule...</p>
      </section>
    );
  }

  if (!isNewRule && rulesQuery.data && !existingRule) {
    return (
      <section>
        <h2>Edit Rule</h2>
        <p>Rule not found.</p>
        <p>
          Return to the <Link to="/rules">rule list</Link>.
        </p>
      </section>
    );
  }

  return (
    <section>
      <h2>{isNewRule ? 'Create Rule' : `Edit Rule: ${id}`}</h2>
      <p>{isNewRule ? 'Define a new routing rule.' : 'Update routing rule conditions and upstream targets.'}</p>

      {rulesQuery.isError && <p>Unable to load rule data.</p>}
      {upstreamsQuery.isError && <p>Unable to load upstream options.</p>}
      {saveError && <p>{saveError}</p>}

      <form onSubmit={handleSubmit} style={{ display: 'grid', gap: '1rem', maxWidth: '48rem' }}>
        {isNewRule && (
          <label style={{ display: 'grid', gap: '0.25rem' }}>
            Rule ID
            <input type="text" value={form.id} onChange={(event) => updateField('id', event.target.value)} required />
          </label>
        )}

        <label style={{ display: 'grid', gap: '0.25rem' }}>
          Name
          <input type="text" value={form.name} onChange={(event) => updateField('name', event.target.value)} required />
        </label>

        <label style={{ display: 'grid', gap: '0.25rem' }}>
          Priority
          <input
            type="number"
            value={form.priority}
            onChange={(event) => updateField('priority', event.target.value)}
            required
          />
        </label>

        <ConditionBuilder conditions={form.conditions} onChange={(conditions) => updateField('conditions', conditions)} />

        <label style={{ display: 'grid', gap: '0.25rem' }}>
          Upstream
          <select value={form.upstream} onChange={(event) => updateField('upstream', event.target.value)} required>
            <option value="" disabled>
              Select upstream
            </option>
            {upstreamsQuery.data?.map((upstream) => (
              <option key={upstream.name} value={upstream.name}>
                {upstream.name}
              </option>
            ))}
          </select>
        </label>

        <label style={{ display: 'grid', gap: '0.25rem' }}>
          Weight
          <input
            type="number"
            min="0"
            value={form.weight}
            onChange={(event) => updateField('weight', event.target.value)}
            required
          />
        </label>

        <div style={{ display: 'flex', gap: '1rem', alignItems: 'center' }}>
          <button type="submit">Save rule</button>
          <Link to="/rules">Cancel</Link>
        </div>
      </form>
    </section>
  );
}

export default RuleEditor;
