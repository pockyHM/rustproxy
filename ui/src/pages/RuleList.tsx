import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { deleteRule, getRules } from '../api/client';
import { useI18n } from '../i18n';
import type { RuleCondition } from '../types/conditions';

type Rule = {
  id: string;
  name: string;
  priority: number;
  conditions: RuleCondition[];
  upstream: string;
  weight: number;
};

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

const badgeClass = (type: RuleCondition['type']): string => {
  const map: Record<string, string> = {
    header: 'badge--header',
    cookie: 'badge--cookie',
    jwt: 'badge--jwt',
  };
  return map[type] ?? 'badge--default';
};

function RuleList() {
  const { t } = useI18n();
  const [search, setSearch] = useState('');
  const [typeFilter, setTypeFilter] = useState<string>('all');

  const rulesQuery = useQuery({
    queryKey: ['rules'],
    queryFn: async () => {
      const response = await getRules();
      return unwrapApiData<Rule[]>(response.data);
    },
  });

  const filteredRules = useMemo(() => {
    if (!rulesQuery.data) return [];
    let rules = rulesQuery.data;

    if (search.trim()) {
      const q = search.toLowerCase();
      rules = rules.filter((r) =>
        r.name.toLowerCase().includes(q) || r.id.toLowerCase().includes(q)
      );
    }

    if (typeFilter !== 'all') {
      rules = rules.filter((r) =>
        r.conditions.some((c) => c.type === typeFilter)
      );
    }

    return rules;
  }, [rulesQuery.data, search, typeFilter]);

  const totalConditions = useMemo(
    () => (rulesQuery.data ?? []).reduce((sum, r) => sum + r.conditions.length, 0),
    [rulesQuery.data],
  );

  const handleDelete = async (ruleId: string, ruleName: string) => {
    const confirmed = window.confirm(
      t.common.confirmDelete.replace('{name}', ruleName),
    );
    if (!confirmed) return;
    await deleteRule(ruleId);
    await rulesQuery.refetch();
  };

  return (
    <div className="page">
      <div className="page-header">
        <div>
          <h1 className="page-header__title">{t.rules.title}</h1>
          <p className="page-header__desc">{t.rules.desc}</p>
        </div>
        <Link to="/rules/new" className="btn-primary">{t.rules.newRule}</Link>
      </div>

      {rulesQuery.isLoading && <div className="loading-state">{t.common.loading}</div>}
      {rulesQuery.isError && <div className="empty-state">{t.common.loadFail}</div>}

      {rulesQuery.data && rulesQuery.data.length > 0 && (
        <>
          {/* Summary */}
          <div className="summary-row">
            <span className="summary-stat">
              <strong>{rulesQuery.data.length}</strong> {t.rules.totalRules}
            </span>
            <span className="summary-stat">
              <strong>{totalConditions}</strong> {t.rules.totalConditions}
            </span>
          </div>

          {/* Search & Filter */}
          <div className="search-bar">
            <input
              type="text"
              className="search-input"
              placeholder={t.rules.search}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
            <select
              className="filter-select"
              value={typeFilter}
              onChange={(e) => setTypeFilter(e.target.value)}
            >
              <option value="all">{t.rules.filterAll}</option>
              <option value="header">{t.rules.filterHeader}</option>
              <option value="cookie">{t.rules.filterCookie}</option>
              <option value="jwt">{t.rules.filterJwt}</option>
            </select>
          </div>

          {/* Table */}
          <table className="data-table">
            <thead>
              <tr>
                <th>{t.rules.name}</th>
                <th>{t.rules.priority}</th>
                <th>{t.rules.conditions}</th>
                <th>{t.rules.upstream}</th>
                <th>{t.rules.weight}</th>
                <th>{t.rules.actions}</th>
              </tr>
            </thead>
            <tbody>
              {filteredRules.map((rule) => (
                <tr key={rule.id}>
                  <td style={{ fontWeight: 'var(--weight-medium)' }}>{rule.name}</td>
                  <td>
                    <span className="badge badge--default">{rule.priority}</span>
                  </td>
                  <td>
                    {rule.conditions.length > 0 ? (
                      <div className="badge-list">
                        {rule.conditions.map((condition, index) => (
                          <span key={index} className={`badge ${badgeClass(condition.type)}`}>
                            {condition.type === 'jwt' ? condition.claim_path : condition.key}
                            {' '}
                            {condition.operator}
                            {condition.operator !== 'exists' && condition.value ? ` ${condition.value}` : ''}
                          </span>
                        ))}
                      </div>
                    ) : (
                      <span style={{ color: 'var(--color-gray-400)' }}>{t.rules.alwaysMatches}</span>
                    )}
                  </td>
                  <td>{rule.upstream}</td>
                  <td>{rule.weight}</td>
                  <td>
                    <div className="table-actions">
                      <Link to={`/rules/${rule.id}`} className="btn-ghost btn-sm">{t.common.edit}</Link>
                      <button
                        type="button"
                        className="btn-danger btn-sm"
                        onClick={() => handleDelete(rule.id, rule.name)}
                        aria-label={`Delete rule ${rule.name}`}
                      >
                        {t.common.delete}
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>

          {filteredRules.length === 0 && (
            <div className="empty-state" style={{ padding: 'var(--space-8)' }}>
              {t.common.noData}
            </div>
          )}
        </>
      )}

      {rulesQuery.data?.length === 0 && (
        <div className="empty-state">
          {t.rules.empty}
          <div className="empty-state__action">
            <Link to="/rules/new" className="btn-primary">{t.rules.createFirst}</Link>
          </div>
        </div>
      )}
    </div>
  );
}

export default RuleList;
