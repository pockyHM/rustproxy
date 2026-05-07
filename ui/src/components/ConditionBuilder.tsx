export type ConditionType = 'header' | 'cookie' | 'jwt';
export type ConditionOperator = 'exact' | 'regex' | 'exists' | 'contains';

export type RuleCondition = {
  type: ConditionType;
  key?: string;
  claim_path?: string;
  operator: ConditionOperator;
  value?: string;
};

type ConditionBuilderProps = {
  conditions: RuleCondition[];
  onChange: (conditions: RuleCondition[]) => void;
};

const conditionTypes: ConditionType[] = ['header', 'cookie', 'jwt'];
const operators: ConditionOperator[] = ['exact', 'regex', 'exists', 'contains'];

const labels: Record<ConditionType | ConditionOperator, string> = {
  header: 'Header',
  cookie: 'Cookie',
  jwt: 'JWT',
  exact: 'exact',
  regex: 'regex',
  exists: 'exists',
  contains: 'contains',
};

const createCondition = (): RuleCondition => ({
  type: 'header',
  key: '',
  operator: 'exact',
  value: '',
});

function normalizeCondition(condition: RuleCondition): RuleCondition {
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
}

function ConditionBuilder({ conditions, onChange }: ConditionBuilderProps) {
  const updateCondition = (index: number, updates: Partial<RuleCondition>) => {
    const nextConditions = conditions.map((condition, currentIndex) => {
      if (currentIndex !== index) {
        return condition;
      }

      return normalizeCondition({ ...condition, ...updates });
    });

    onChange(nextConditions);
  };

  const addCondition = () => {
    onChange([...conditions, createCondition()]);
  };

  const removeCondition = (index: number) => {
    onChange(conditions.filter((_, currentIndex) => currentIndex !== index));
  };

  return (
    <fieldset style={{ border: '1px solid #ddd', borderRadius: '0.5rem', padding: '1rem' }}>
      <legend>Conditions</legend>

      {conditions.length === 0 && <p>No conditions configured. Add one to define when this rule matches.</p>}

      <div style={{ display: 'grid', gap: '1rem' }}>
        {conditions.map((condition, index) => {
          const normalizedCondition = normalizeCondition(condition);
          const isJwt = normalizedCondition.type === 'jwt';
          const isExists = normalizedCondition.operator === 'exists';

          return (
            <div
              key={index}
              style={{
                border: '1px solid #eee',
                borderRadius: '0.5rem',
                display: 'grid',
                gap: '0.75rem',
                padding: '1rem',
              }}
            >
              <div style={{ display: 'flex', gap: '1rem', flexWrap: 'wrap' }}>
                <label style={{ display: 'grid', gap: '0.25rem' }}>
                  Type
                  <select
                    value={normalizedCondition.type}
                    onChange={(event) =>
                      updateCondition(index, {
                        type: event.target.value as ConditionType,
                        key: event.target.value === 'jwt' ? undefined : normalizedCondition.key,
                        claim_path: event.target.value === 'jwt' ? normalizedCondition.claim_path : undefined,
                      })
                    }
                  >
                    {conditionTypes.map((conditionType) => (
                      <option key={conditionType} value={conditionType}>
                        {labels[conditionType]}
                      </option>
                    ))}
                  </select>
                </label>

                {isJwt ? (
                  <label style={{ display: 'grid', gap: '0.25rem' }}>
                    Claim path
                    <input
                      type="text"
                      value={normalizedCondition.claim_path ?? ''}
                      onChange={(event) => updateCondition(index, { claim_path: event.target.value })}
                      placeholder="roles.0"
                    />
                  </label>
                ) : (
                  <label style={{ display: 'grid', gap: '0.25rem' }}>
                    Key
                    <input
                      type="text"
                      value={normalizedCondition.key ?? ''}
                      onChange={(event) => updateCondition(index, { key: event.target.value })}
                      placeholder={normalizedCondition.type === 'header' ? 'Host' : 'session'}
                    />
                  </label>
                )}

                <label style={{ display: 'grid', gap: '0.25rem' }}>
                  Operator
                  <select
                    value={normalizedCondition.operator}
                    onChange={(event) => updateCondition(index, { operator: event.target.value as ConditionOperator })}
                  >
                    {operators.map((operator) => (
                      <option key={operator} value={operator}>
                        {labels[operator]}
                      </option>
                    ))}
                  </select>
                </label>

                {!isExists && (
                  <label style={{ display: 'grid', gap: '0.25rem' }}>
                    Value
                    <input
                      type="text"
                      value={normalizedCondition.value ?? ''}
                      onChange={(event) => updateCondition(index, { value: event.target.value })}
                    />
                  </label>
                )}
              </div>

              <div>
                <button type="button" onClick={() => removeCondition(index)}>
                  Remove condition
                </button>
              </div>
            </div>
          );
        })}
      </div>

      <button type="button" onClick={addCondition} style={{ marginTop: '1rem' }}>
        Add condition
      </button>
    </fieldset>
  );
}

export default ConditionBuilder;
