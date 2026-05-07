import { useQuery } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { getConfig, getMetrics } from '../api/client';

type ProxyConfig = {
  rules?: unknown[] | Record<string, unknown>;
  upstreams?: unknown[] | Record<string, unknown>;
};

const countItems = (items: unknown): number => {
  if (Array.isArray(items)) {
    return items.length;
  }

  if (items && typeof items === 'object') {
    return Object.keys(items).length;
  }

  return 0;
};

const formatMetricPreview = (metrics: unknown): string[] => {
  if (typeof metrics === 'string') {
    return metrics
      .split('\n')
      .map((line) => line.trim())
      .filter((line) => line.length > 0 && !line.startsWith('#'))
      .slice(0, 5);
  }

  return JSON.stringify(metrics, null, 2).split('\n').slice(0, 5);
};

function Dashboard() {
  const configQuery = useQuery({
    queryKey: ['config'],
    queryFn: async () => {
      const response = await getConfig();
      return response.data as ProxyConfig;
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
      <section>
        <h2>Dashboard</h2>
        <p>Loading dashboard data...</p>
      </section>
    );
  }

  if (configQuery.isError) {
    return (
      <section>
        <h2>Dashboard</h2>
        <p>Unable to load RustProxy configuration.</p>
      </section>
    );
  }

  const config = configQuery.data;
  const ruleCount = countItems(config?.rules);
  const upstreamCount = countItems(config?.upstreams);
  const metricPreview = metricsQuery.data ? formatMetricPreview(metricsQuery.data) : [];

  return (
    <section>
      <h2>Dashboard</h2>
      <p>Monitor RustProxy health, traffic, and routing activity from one place.</p>

      <div style={{ display: 'flex', gap: '1rem', flexWrap: 'wrap', margin: '1.5rem 0' }}>
        <article style={{ border: '1px solid #ddd', borderRadius: '0.5rem', padding: '1rem', minWidth: '10rem' }}>
          <h3>Rules</h3>
          <p style={{ fontSize: '2rem', margin: 0 }}>{ruleCount}</p>
          <Link to="/rules">View routing rules</Link>
        </article>

        <article style={{ border: '1px solid #ddd', borderRadius: '0.5rem', padding: '1rem', minWidth: '10rem' }}>
          <h3>Upstreams</h3>
          <p style={{ fontSize: '2rem', margin: 0 }}>{upstreamCount}</p>
          <Link to="/upstreams">View upstreams</Link>
        </article>
      </div>

      <section>
        <h3>Traffic stats</h3>
        {metricsQuery.isLoading && <p>Loading metrics...</p>}
        {metricsQuery.isError && <p>Unable to load metrics from /metrics.</p>}
        {metricPreview.length > 0 && (
          <pre style={{ background: '#f6f8fa', borderRadius: '0.5rem', padding: '1rem', overflowX: 'auto' }}>
            {metricPreview.join('\n')}
          </pre>
        )}
        {!metricsQuery.isLoading && !metricsQuery.isError && metricPreview.length === 0 && <p>No metrics available yet.</p>}
      </section>
    </section>
  );
}

export default Dashboard;
