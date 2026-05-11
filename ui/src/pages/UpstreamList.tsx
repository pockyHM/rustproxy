import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { deleteUpstream, getUpstreams } from '../api/client';
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

const unwrapApiData = <T,>(payload: T | ApiResponse<T>): T => {
  if (payload && typeof payload === 'object' && 'data' in payload) {
    return (payload as ApiResponse<T>).data;
  }
  return payload as T;
};

const WEIGHT_COLORS = ['var(--color-link)', 'var(--color-success)', 'var(--color-warning)', 'var(--color-gray-400)'];

function UpstreamList() {
  const { t } = useI18n();

  const upstreamsQuery = useQuery({
    queryKey: ['upstreams'],
    queryFn: async () => {
      const response = await getUpstreams();
      return unwrapApiData<Upstream[]>(response.data);
    },
  });

  const totalTargets = useMemo(
    () => (upstreamsQuery.data ?? []).reduce((sum, u) => sum + u.targets.length, 0),
    [upstreamsQuery.data],
  );

  const handleDelete = async (upstreamId: string, upstreamName: string) => {
    const confirmed = window.confirm(
      t.common.confirmDelete.replace('{name}', upstreamName),
    );
    if (!confirmed) return;
    await deleteUpstream(upstreamId);
    await upstreamsQuery.refetch();
  };

  return (
    <div className="page">
      <div className="page-header">
        <div>
          <h1 className="page-header__title">{t.upstreams.title}</h1>
          <p className="page-header__desc">{t.upstreams.desc}</p>
        </div>
        <Link to="/upstreams/new" className="btn-primary">{t.upstreams.newUpstream}</Link>
      </div>

      {upstreamsQuery.isLoading && <div className="loading-state">{t.common.loading}</div>}
      {upstreamsQuery.isError && <div className="empty-state">{t.common.loadFail}</div>}

      {upstreamsQuery.data && upstreamsQuery.data.length > 0 && (
        <>
          {/* Summary */}
          <div className="summary-row">
            <span className="summary-stat">
              <strong>{upstreamsQuery.data.length}</strong> {t.upstreams.totalUpstreams}
            </span>
            <span className="summary-stat">
              <strong>{totalTargets}</strong> {t.upstreams.totalTargets}
            </span>
          </div>

          {/* Table */}
          <table className="data-table">
            <thead>
              <tr>
                <th>{t.upstreams.name}</th>
                <th>{t.upstreams.targets}</th>
                <th>{t.upstreams.actions}</th>
              </tr>
            </thead>
            <tbody>
              {upstreamsQuery.data.map((upstream) => {
                const totalWeight = upstream.targets.reduce((sum, t) => sum + t.weight, 0);

                return (
                  <tr key={upstream.name}>
                    <td>
                      <div style={{ fontWeight: 'var(--weight-medium)' }}>{upstream.name}</div>
                      <div style={{ fontSize: 'var(--text-xs)', color: 'var(--color-gray-400)', marginTop: 'var(--space-1)' }}>
                        {upstream.targets.length} {upstream.targets.length === 1 ? 'target' : 'targets'}
                      </div>
                    </td>
                    <td>
                      {upstream.targets.length > 0 ? (
                        <div>
                          <div className="badge-list" style={{ marginBottom: 'var(--space-2)' }}>
                            {upstream.targets.map((target, index) => (
                              <span key={index} className="badge badge--default">
                                {target.url}
                                {target.weight !== 100 && ` (${target.weight})`}
                              </span>
                            ))}
                          </div>
                          {upstream.targets.length > 1 && totalWeight > 0 && (
                            <div className="weight-bar" style={{ maxWidth: '16rem' }}>
                              {upstream.targets.map((target, index) => (
                                <div
                                  key={index}
                                  className="weight-bar__segment"
                                  style={{
                                    width: `${(target.weight / totalWeight) * 100}%`,
                                    background: WEIGHT_COLORS[index % WEIGHT_COLORS.length],
                                  }}
                                />
                              ))}
                            </div>
                          )}
                        </div>
                      ) : (
                        <span style={{ color: 'var(--color-gray-400)' }}>{t.upstreams.noTargets}</span>
                      )}
                    </td>
                    <td>
                      <div className="table-actions">
                        <Link to={`/upstreams/${upstream.name}`} className="btn-ghost btn-sm">{t.common.edit}</Link>
                        <button
                          type="button"
                          className="btn-danger btn-sm"
                          onClick={() => handleDelete(upstream.name, upstream.name)}
                          aria-label={`Delete upstream ${upstream.name}`}
                        >
                          {t.common.delete}
                        </button>
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </>
      )}

      {upstreamsQuery.data?.length === 0 && (
        <div className="empty-state">
          {t.upstreams.empty}
          <div className="empty-state__action">
            <Link to="/upstreams/new" className="btn-primary">{t.upstreams.createFirst}</Link>
          </div>
        </div>
      )}
    </div>
  );
}

export default UpstreamList;
