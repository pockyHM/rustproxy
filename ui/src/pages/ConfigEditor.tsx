import { FormEvent, useEffect, useState } from 'react';
import { dump, load } from 'js-yaml';
import { getConfig, updateConfig } from '../api/client';

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

function ConfigEditor() {
  const [yamlText, setYamlText] = useState('');
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let isMounted = true;

    const loadConfig = async () => {
      setIsLoading(true);
      setError(null);
      setMessage(null);

      try {
        const response = await getConfig();
        const config = unwrapApiData<unknown>(response.data);

        if (isMounted) {
          setYamlText(dump(config, { noRefs: true }));
        }
      } catch (loadError) {
        if (isMounted) {
          setError(loadError instanceof Error ? loadError.message : 'Unable to load configuration.');
        }
      } finally {
        if (isMounted) {
          setIsLoading(false);
        }
      }
    };

    void loadConfig();

    return () => {
      isMounted = false;
    };
  }, []);

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setIsSaving(true);
    setError(null);
    setMessage(null);

    try {
      const parsedConfig = load(yamlText);
      await updateConfig(parsedConfig);
      setMessage('Configuration saved successfully.');
    } catch (saveError) {
      setError(saveError instanceof Error ? saveError.message : 'Unable to save configuration.');
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <section>
      <h2>Configuration</h2>
      <p>Edit and validate the RustProxy YAML configuration.</p>

      {isLoading && <p>Loading configuration...</p>}
      {message && <p>{message}</p>}
      {error && <p>{error}</p>}

      {!isLoading && (
        <form onSubmit={handleSubmit} style={{ display: 'grid', gap: '1rem' }}>
          <label style={{ display: 'grid', gap: '0.5rem' }}>
            Raw YAML
            <textarea
              value={yamlText}
              onChange={(event) => setYamlText(event.target.value)}
              rows={24}
              spellCheck={false}
              style={{ fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace', width: '100%' }}
            />
          </label>

          <div>
            <button type="submit" disabled={isSaving}>
              {isSaving ? 'Saving...' : 'Save configuration'}
            </button>
          </div>
        </form>
      )}
    </section>
  );
}

export default ConfigEditor;
