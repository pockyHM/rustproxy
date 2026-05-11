import { useCallback, memo } from 'react';
import { useI18n } from '../i18n';
import type { ConditionType, ConditionOperator, RuleCondition } from '../types/conditions';

type ConditionBuilderProps = {
  conditions: RuleCondition[];
  onChange: (conditions: RuleCondition[]) => void;
};

const conditionTypes: ConditionType[] = ['header', 'cookie', 'jwt'];
const operators: ConditionOperator[] = ['exact', 'regex', 'exists', 'contains'];

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

type ConditionCardProps = {
  condition: RuleCondition;
  index: number;
  onUpdate: (index: number, updates: Partial<RuleCondition>) => void;
  onRemove: (index: number) => void;
};

const ConditionCard = memo(function ConditionCard({
  condition,
  index,
  onUpdate,
  onRemove,
}: ConditionCardProps) {
  const { t } = useI18n();
  const normalizedCondition = normalizeCondition(condition);
  const isJwt = normalizedCondition.type === 'jwt';
  const isExists = normalizedCondition.operator === 'exists';

  const hintKey = isJwt ? 'jwtHint' : normalizedCondition.type === 'cookie' ? 'cookieHint' : 'headerHint';

  return (
    <div className="condition-card" role="group" aria-label={`Condition ${index + 1}`}>
      <p className="field-hint" style={{ margin: 0 }}>
        {t.conditions[hintKey]}
      </p>
      <div className="field-row">
        <div className="form-group" style={{ flex: '1 1 8rem' }}>
          <label className="field-label" htmlFor={`condition-type-${index}`}>{t.conditions.type}</label>
          <select
            id={`condition-type-${index}`}
            value={normalizedCondition.type}
            onChange={(event) =>
              onUpdate(index, {
                type: event.target.value as ConditionType,
                key: event.target.value === 'jwt' ? undefined : normalizedCondition.key,
                claim_path: event.target.value === 'jwt' ? normalizedCondition.claim_path : undefined,
              })
            }
            className="field-select"
          >
            {conditionTypes.map((ct) => (
              <option key={ct} value={ct}>
                {ct.toUpperCase()}
              </option>
            ))}
          </select>
        </div>

        {isJwt ? (
          <div className="form-group" style={{ flex: '1 1 10rem' }}>
            <label className="field-label" htmlFor={`condition-claim-${index}`}>{t.conditions.claimPath}</label>
            <input
              id={`condition-claim-${index}`}
              type="text"
              value={normalizedCondition.claim_path ?? ''}
              onChange={(event) => onUpdate(index, { claim_path: event.target.value })}
              className="field-input"
              placeholder="roles.0"
            />
            <p className="field-hint">{t.conditions.claimPathHint}</p>
          </div>
        ) : (
          <div className="form-group" style={{ flex: '1 1 10rem' }}>
            <label className="field-label" htmlFor={`condition-key-${index}`}>{t.conditions.key}</label>
            <input
              id={`condition-key-${index}`}
              type="text"
              value={normalizedCondition.key ?? ''}
              onChange={(event) => onUpdate(index, { key: event.target.value })}
              className="field-input"
              placeholder={normalizedCondition.type === 'header' ? 'Host' : 'session'}
            />
            <p className="field-hint">
              {normalizedCondition.type === 'header' ? t.conditions.keyHintHeader : t.conditions.keyHintCookie}
            </p>
          </div>
        )}

        <div className="form-group" style={{ flex: '1 1 8rem' }}>
          <label className="field-label" htmlFor={`condition-op-${index}`}>{t.conditions.operator}</label>
          <select
            id={`condition-op-${index}`}
            value={normalizedCondition.operator}
            onChange={(event) => onUpdate(index, { operator: event.target.value as ConditionOperator })}
            className="field-select"
          >
            {operators.map((operator) => (
              <option key={operator} value={operator}>
                {operator}
              </option>
            ))}
          </select>
        </div>

        {!isExists && (
          <div className="form-group" style={{ flex: '1 1 10rem' }}>
            <label className="field-label" htmlFor={`condition-value-${index}`}>{t.conditions.value}</label>
            <input
              id={`condition-value-${index}`}
              type="text"
              value={normalizedCondition.value ?? ''}
              onChange={(event) => onUpdate(index, { value: event.target.value })}
              className="field-input"
            />
          </div>
        )}
      </div>

      <button
        type="button"
        className="btn-danger btn-sm"
        onClick={() => onRemove(index)}
        aria-label={`${t.conditions.remove} ${index + 1}`}
      >
        {t.conditions.remove}
      </button>
    </div>
  );
});

function ConditionBuilder({ conditions, onChange }: ConditionBuilderProps) {
  const { t } = useI18n();

  const updateCondition = useCallback(
    (index: number, updates: Partial<RuleCondition>) => {
      const nextConditions = conditions.map((condition, currentIndex) => {
        if (currentIndex !== index) return condition;
        return normalizeCondition({ ...condition, ...updates });
      });
      onChange(nextConditions);
    },
    [conditions, onChange],
  );

  const addCondition = useCallback(() => {
    onChange([...conditions, createCondition()]);
  }, [conditions, onChange]);

  const removeCondition = useCallback(
    (index: number) => {
      onChange(conditions.filter((_, currentIndex) => currentIndex !== index));
    },
    [conditions, onChange],
  );

  return (
    <div className="form-section">
      <h2 className="form-section__title">{t.conditions.title}</h2>

      {conditions.length === 0 && (
        <p style={{ color: 'var(--color-gray-400)', fontSize: 'var(--text-sm)', margin: 0 }}>
          {t.conditions.empty}
        </p>
      )}

      <div style={{ display: 'grid', gap: 'var(--space-3)' }}>
        {conditions.map((condition, index) => (
          <ConditionCard
            key={index}
            condition={condition}
            index={index}
            onUpdate={updateCondition}
            onRemove={removeCondition}
          />
        ))}
      </div>

      <button type="button" className="btn-secondary btn-sm" onClick={addCondition} style={{ marginTop: 'var(--space-4)' }}>
        {t.conditions.addCondition}
      </button>
    </div>
  );
}

export default ConditionBuilder;
