export type ConditionType = 'header' | 'cookie' | 'jwt';
export type ConditionOperator = 'exact' | 'regex' | 'exists' | 'contains';

export type RuleCondition = {
  type: ConditionType;
  key?: string;
  claim_path?: string;
  operator: ConditionOperator;
  value?: string;
};