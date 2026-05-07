import { useQuery } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { deleteUpstream, getUpstreams } from '../api/client';

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

const formatTargets = (targets: Target[]): string => {
  if (targets.length === 0) {
    return 'No targets';
  }

  return targets.map((target) => `${target.url} (weight ${target.weight})`).join(', ');
};

function UpstreamList() {
  const upstreamsQuery = useQuery({
    queryKey: ['upstreams'],
    queryFn: async () => {
      const response = await getUpstreams();
      return unwrapApiData<Upstream[]>(response.data);
    },
  });

  const handleDelete = async (upstreamId: string) => {
    await deleteUpstream(upstreamId);
    await upstreamsQuery.refetch();
  };

  return (
    <section>
      <div style={{ display: 'flex', justifyContent: 'space-between', gap: '1rem', alignItems: 'center' }}>
        <div>
          <h2>Upstreams</h2>
          <p>View and manage backend upstream pools for RustProxy.</p>
        </div>
        <Link to="/upstreams/new">New Upstream</Link>
      </div>

      {upstreamsQuery.isLoading && <p>Loading upstreams...</p>}
      {upstreamsQuery.isError && <p>Unable to load upstreams.</p>}

      {upstreamsQuery.data && upstreamsQuery.data.length === 0 && <p>No upstreams configured yet.</p>}

      {upstreamsQuery.data && upstreamsQuery.data.length > 0 && (
        <table style={{ borderCollapse: 'collapse', width: '100%' }}>
          <thead>
            <tr>
              <th style={{ borderBottom: '1px solid #ddd', textAlign: 'left', padding: '0.5rem' }}>Name</th>
              <th style={{ borderBottom: '1px solid #ddd', textAlign: 'left', padding: '0.5rem' }}>Targets</th>
              <th style={{ borderBottom: '1px solid #ddd', textAlign: 'left', padding: '0.5rem' }}>Actions</th>
            </tr>
          </thead>
          <tbody>
            {upstreamsQuery.data.map((upstream) => (
              <tr key={upstream.name}>
                <td style={{ borderBottom: '1px solid #eee', padding: '0.5rem' }}>{upstream.name}</td>
                <td style={{ borderBottom: '1px solid #eee', padding: '0.5rem' }}>{formatTargets(upstream.targets)}</td>
                <td style={{ borderBottom: '1px solid #eee', padding: '0.5rem' }}>
                  <div style={{ display: 'flex', gap: '0.75rem', alignItems: 'center' }}>
                    <Link to={`/upstreams/${upstream.name}`}>Edit</Link>
                    <button type="button" onClick={() => handleDelete(upstream.name)}>
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

export default UpstreamList;
