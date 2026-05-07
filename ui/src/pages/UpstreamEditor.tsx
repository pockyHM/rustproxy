import { FormEvent, useEffect, useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Link, useNavigate, useParams } from 'react-router-dom';
import { createUpstream, getUpstreams, updateUpstream } from '../api/client';

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

type TargetFormState = {
  url: string;
  weight: string;
};

type UpstreamFormState = {
  name: string;
  targets: TargetFormState[];
};

const unwrapApiData = <T,>(payload: T | ApiResponse<T>): T => {
  if (payload && typeof payload === 'object' && 'data' in payload) {
    return (payload as ApiResponse<T>).data;
  }

  return payload as T;
};

const createEmptyForm = (): UpstreamFormState => ({
  name: '',
  targets: [{ url: '', weight: '100' }],
});

function UpstreamEditor() {
  const { id } = useParams();
  const navigate = useNavigate();
  const isNewUpstream = id === undefined || id === 'new';
  const [form, setForm] = useState<UpstreamFormState>(createEmptyForm);
  const [saveError, setSaveError] = useState<string | null>(null);

  const upstreamsQuery = useQuery({
    queryKey: ['upstreams'],
    queryFn: async () => {
      const response = await getUpstreams();
      return unwrapApiData<Upstream[]>(response.data);
    },
    enabled: !isNewUpstream,
  });

  const existingUpstream = useMemo(() => {
    if (isNewUpstream || !upstreamsQuery.data) {
      return undefined;
    }

    return upstreamsQuery.data.find((upstream) => upstream.name === id);
  }, [id, isNewUpstream, upstreamsQuery.data]);

  useEffect(() => {
    if (existingUpstream) {
      setForm({
        name: existingUpstream.name,
        targets:
          existingUpstream.targets.length > 0
            ? existingUpstream.targets.map((target) => ({
                url: target.url,
                weight: String(target.weight),
              }))
            : [{ url: '', weight: '100' }],
      });
    }
  }, [existingUpstream]);

  const updateName = (name: string) => {
    setForm((currentForm) => ({ ...currentForm, name }));
  };

  const updateTarget = (index: number, field: keyof TargetFormState, value: string) => {
    setForm((currentForm) => ({
      ...currentForm,
      targets: currentForm.targets.map((target, targetIndex) =>
        targetIndex === index ? { ...target, [field]: value } : target,
      ),
    }));
  };

  const addTarget = () => {
    setForm((currentForm) => ({
      ...currentForm,
      targets: [...currentForm.targets, { url: '', weight: '100' }],
    }));
  };

  const removeTarget = (index: number) => {
    setForm((currentForm) => ({
      ...currentForm,
      targets: currentForm.targets.filter((_, targetIndex) => targetIndex !== index),
    }));
  };

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setSaveError(null);

    const upstreamName = isNewUpstream ? form.name.trim() : id ?? form.name.trim();
    const upstream: Upstream = {
      name: upstreamName,
      targets: form.targets.map((target) => ({
        url: target.url.trim(),
        weight: Number(target.weight),
      })),
    };

    try {
      if (isNewUpstream) {
        await createUpstream(upstream);
      } else {
        await updateUpstream(upstreamName, upstream);
      }

      navigate('/upstreams');
    } catch (error) {
      setSaveError(error instanceof Error ? error.message : 'Unable to save upstream.');
    }
  };

  if (!isNewUpstream && upstreamsQuery.isLoading) {
    return (
      <section>
        <h2>Edit Upstream</h2>
        <p>Loading upstream...</p>
      </section>
    );
  }

  if (!isNewUpstream && upstreamsQuery.data && !existingUpstream) {
    return (
      <section>
        <h2>Edit Upstream</h2>
        <p>Upstream not found.</p>
        <p>
          Return to the <Link to="/upstreams">upstream list</Link>.
        </p>
      </section>
    );
  }

  return (
    <section>
      <h2>{isNewUpstream ? 'Create Upstream' : `Edit Upstream: ${id}`}</h2>
      <p>{isNewUpstream ? 'Define a new backend upstream pool.' : 'Update backend targets and weights.'}</p>

      {upstreamsQuery.isError && <p>Unable to load upstream data.</p>}
      {saveError && <p>{saveError}</p>}

      <form onSubmit={handleSubmit} style={{ display: 'grid', gap: '1rem', maxWidth: '48rem' }}>
        <label style={{ display: 'grid', gap: '0.25rem' }}>
          Upstream Name
          <input type="text" value={form.name} onChange={(event) => updateName(event.target.value)} required disabled={!isNewUpstream} />
        </label>

        <fieldset style={{ display: 'grid', gap: '1rem' }}>
          <legend>Targets</legend>
          {form.targets.map((target, index) => (
            <div
              key={index}
              style={{
                border: '1px solid #ddd',
                borderRadius: '0.5rem',
                display: 'grid',
                gap: '0.75rem',
                padding: '1rem',
              }}
            >
              <label style={{ display: 'grid', gap: '0.25rem' }}>
                URL
                <input
                  type="url"
                  value={target.url}
                  onChange={(event) => updateTarget(index, 'url', event.target.value)}
                  placeholder="http://localhost:8080"
                  required
                />
              </label>

              <label style={{ display: 'grid', gap: '0.25rem' }}>
                Weight
                <input
                  type="number"
                  min="0"
                  value={target.weight}
                  onChange={(event) => updateTarget(index, 'weight', event.target.value)}
                  required
                />
              </label>

              <button type="button" onClick={() => removeTarget(index)} disabled={form.targets.length === 1}>
                Remove target
              </button>
            </div>
          ))}

          <button type="button" onClick={addTarget}>
            Add target
          </button>
        </fieldset>

        <div style={{ display: 'flex', gap: '1rem', alignItems: 'center' }}>
          <button type="submit">Save upstream</button>
          <Link to="/upstreams">Cancel</Link>
        </div>
      </form>
    </section>
  );
}

export default UpstreamEditor;
