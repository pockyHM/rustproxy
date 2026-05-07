import { useQuery } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { deleteRule, getRules } from '../api/client';
import type { RuleCondition } from '../components/ConditionBuilder';

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

const formatCondition = (condition: RuleCondition): string => {
  const target = condition.type === 'jwt' ? condition.claim_path : condition.key;
  const value = condition.operator === 'exists' ? '' : ` ${condition.value ?? ''}`;
  return `${condition.type} ${target ?? ''} ${condition.operator}${value}`.trim();
};

function RuleList() {
  const rulesQuery = useQuery({
    queryKey: ['rules'],
    queryFn: async () => {
      const response = await getRules();
      return unwrapApiData<Rule[]>(response.data);
    },
  });

  const handleDelete = async (ruleId: string) => {
    await deleteRule(ruleId);
    await rulesQuery.refetch();
  };

  return (
    <section>
      <div style={{ display: 'flex', justifyContent: 'space-between', gap: '1rem', alignItems: 'center' }}>
        <div>
          <h2>Rules</h2>
          <p>View and manage traffic routing rules for RustProxy.</p>
        </div>
        <Link to="/rules/new">New Rule</Link>
      </div>

      {rulesQuery.isLoading && <p>Loading rules...</p>}
      {rulesQuery.isError && <p>Unable to load rules.</p>}

      {rulesQuery.data && rulesQuery.data.length === 0 && <p>No rules configured yet.</p>}

      {rulesQuery.data && rulesQuery.data.length > 0 && (
        <table style={{ borderCollapse: 'collapse', width: '100%' }}>
          <thead>
            <tr>
              <th style={{ borderBottom: '1px solid #ddd', textAlign: 'left', padding: '0.5rem' }}>Name</th>
              <th style={{ borderBottom: '1px solid #ddd', textAlign: 'left', padding: '0.5rem' }}>Priority</th>
              <th style={{ borderBottom: '1px solid #ddd', textAlign: 'left', padding: '0.5rem' }}>Conditions</th>
              <th style={{ borderBottom: '1px solid #ddd', textAlign: 'left', padding: '0.5rem' }}>Upstream</th>
              <th style={{ borderBottom: '1px solid #ddd', textAlign: 'left', padding: '0.5rem' }}>Actions</th>
            </tr>
          </thead>
          <tbody>
            {rulesQuery.data.map((rule) => (
              <tr key={rule.id}>
                <td style={{ borderBottom: '1px solid #eee', padding: '0.5rem' }}>{rule.name}</td>
                <td style={{ borderBottom: '1px solid #eee', padding: '0.5rem' }}>{rule.priority}</td>
                <td style={{ borderBottom: '1px solid #eee', padding: '0.5rem' }}>
                  {rule.conditions.length > 0 ? (
                    <ul style={{ margin: 0, paddingLeft: '1.25rem' }}>
                      {rule.conditions.map((condition, index) => (
                        <li key={index}>{formatCondition(condition)}</li>
                      ))}
                    </ul>
                  ) : (
                    'No conditions'
                  )}
                </td>
                <td style={{ borderBottom: '1px solid #eee', padding: '0.5rem' }}>{rule.upstream}</td>
                <td style={{ borderBottom: '1px solid #eee', padding: '0.5rem' }}>
                  <div style={{ display: 'flex', gap: '0.75rem', alignItems: 'center' }}>
                    <Link to={`/rules/${rule.id}`}>Edit</Link>
                    <button type="button" onClick={() => handleDelete(rule.id)}>
                      Delete
                    </button>
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}

export default RuleList;
