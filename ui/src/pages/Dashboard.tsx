import { useQuery } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { getConfig, getMetrics } from '../api/client';
import { useI18n } from '../i18n';

type ProxyConfig = {
  listen?: string;
  skip_ssl?: boolean;
  connect_timeout?: number;
  request_timeout?: number;
  rules?: unknown[] | Record<string, unknown>;
  upstreams?: unknown[] | Record<string, unknown>;
};

type RuleLike = {
  conditions?: Array<{ type: string }>;
};

const countItems = (items: unknown): number => {
  if (Array.isArray(items)) return items.length;
  if (items && typeof items === 'object') return Object.keys(items).length;
  return 0;
};

const asArray = (items: unknown): unknown[] => {
  if (Array.isArray(items)) return items;
  if (items && typeof items === 'object') return Object.values(items);
  return [];
};

const formatMetricPreview = (metrics: unknown): string[] => {
  if (typeof metrics === 'string') {
    return metrics
      .split('\n')
      .map((line) => line.trim())
      .filter((line) => line.length > 0 && !line.startsWith('#'))
      .slice(0, 10);
  }
  return JSON.stringify(metrics, null, 2).split('\n').slice(0, 10);
};

function Dashboard() {
  const { t } = useI18n();

  const configQuery = useQuery({
    queryKey: ['config'],
    queryFn: async () => {
      const response = await getConfig();
      const data = response.data?.data ?? response.data;
      return data as ProxyConfig;
    },
  });

  const metricsQuery = useQuery({
    queryKey: ['metrics'],
    queryFn: async () => {
      const response = await getMetrics();
      return response.data;
    },
  });

  if (configQuery.isLoading) {
    return (
      <div className="page">
        <div className="page-header">
          <div>
            <h1 className="page-header__title">{t.dashboard.title}</h1>
          </div>
        </div>
        <div className="loading-state">{t.common.loading}</div>
      </div>
    );
  }

  if (configQuery.isError) {
    return (
      <div className="page">
        <div className="page-header">
          <div>
            <h1 className="page-header__title">{t.dashboard.title}</h1>
          </div>
        </div>
        <div className="message message--error">{t.common.loadFail}</div>
      </div>
    );
  }

  const config = configQuery.data;
  const ruleCount = countItems(config?.rules);
  const upstreamCount = countItems(config?.upstreams);
  const rulesArr = asArray(config?.rules) as RuleLike[];
  const totalConditions = rulesArr.reduce((sum, r) => sum + (r.conditions?.length ?? 0), 0);
  const upstreamsArr = asArray(config?.upstreams) as { targets?: unknown[] }[];
  const totalTargets = upstreamsArr.reduce((sum, u) => sum + (u.targets?.length ?? 0), 0);
  const metricPreview = metricsQuery.data ? formatMetricPreview(metricsQuery.data) : [];

  return (
    <div className="page">
      <div className="page-header">
        <div>
          <h1 className="page-header__title">{t.dashboard.title}</h1>
          <p className="page-header__desc">{t.dashboard.desc}</p>
        </div>
      </div>

      {/* Stats */}
      <div className="card-grid">
        <div className="stat-card">
          <p className="stat-card__label">{t.dashboard.rules}</p>
          <p className="stat-card__value">{ruleCount}</p>
          <Link to="/rules" className="stat-card__link">{t.dashboard.viewRules}</Link>
        </div>
        <div className="stat-card">
          <p className="stat-card__label">{t.dashboard.upstreams}</p>
          <p className="stat-card__value">{upstreamCount}</p>
          <Link to="/upstreams" className="stat-card__link">{t.dashboard.viewUpstreams}</Link>
        </div>
        <div className="stat-card">
          <p className="stat-card__label">{t.dashboard.status}</p>
          <p style={{ display: 'flex', alignItems: 'center', gap: 'var(--space-2)', marginTop: 'var(--space-2)' }}>
            <span className="status-dot status-dot--running" />
            <span style={{ fontSize: 'var(--text-lg)', fontWeight: 'var(--weight-semibold)' }}>
              {t.dashboard.statusRunning}
            </span>
          </p>
        </div>
        <div className="stat-card">
          <p className="stat-card__label">{t.dashboard.systemInfo}</p>
          <div style={{ marginTop: 'var(--space-2)', fontSize: 'var(--text-sm)', color: 'var(--color-gray-500)' }}>
            {config?.listen && (
              <div>{t.dashboard.listenAddr}: <strong style={{ color: 'var(--color-text)' }}>{config.listen}</strong></div>
            )}
            <div>{ruleCount} {t.dashboard.rulesCount}, {totalConditions} {t.dashboard.conditions}</div>
            <div>{upstreamCount} {t.dashboard.upstreamsCount}, {totalTargets} {t.dashboard.targets}</div>
          </div>
        </div>
      </div>

      {/* Quick Actions */}
      <div>
        <h2 style={{ fontSize: 'var(--text-lg)', fontWeight: 'var(--weight-semibold)', marginBottom: 'var(--space-4)' }}>
          {t.dashboard.quickActions}
        </h2>
        <div className="card-grid">
          <Link to="/rules/new" className="quick-action">
            <p className="quick-action__title">{t.dashboard.newRule}</p>
            <p className="quick-action__desc">{t.dashboard.newRuleDesc}</p>
          </Link>
          <Link to="/upstreams/new" className="quick-action">
            <p className="quick-action__title">{t.dashboard.newUpstream}</p>
            <p className="quick-action__desc">{t.dashboard.newUpstreamDesc}</p>
          </Link>
          <Link to="/settings" className="quick-action">
            <p className="quick-action__title">{t.dashboard.editConfig}</p>
            <p className="quick-action__desc">{t.dashboard.editConfigDesc}</p>
          </Link>
        </div>
      </div>

      {/* Metrics */}
      <div>
        <h2 style={{ fontSize: 'var(--text-lg)', fontWeight: 'var(--weight-semibold)', marginBottom: 'var(--space-4)' }}>
          {t.dashboard.trafficMetrics}
        </h2>
        {metricsQuery.isLoading && <div className="loading-state">{t.dashboard.loadingMetrics}</div>}
        {metricsQuery.isError && <div className="empty-state">{t.dashboard.loadMetricsFail}</div>}
        {metricPreview.length > 0 ? (
          <pre className="code-block">{metricPreview.join('\n')}</pre>
        ) : (
          !metricsQuery.isLoading && !metricsQuery.isError && (
            <div className="empty-state">{t.dashboard.noMetrics}</div>
          )
        )}
      </div>
    </div>
  );
}

export default Dashboard;
