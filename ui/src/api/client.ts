import axios from 'axios';

const TOKEN_KEY = 'rustproxy-token';

const api = axios.create({ baseURL: '/api' });

api.interceptors.request.use((config) => {
  const token = localStorage.getItem(TOKEN_KEY);
  if (token) {
    config.headers.Authorization = `Bearer ${token}`;
  }
  return config;
});

api.interceptors.response.use(
  (response) => response,
  (error) => {
    if (error.response?.status === 401) {
      localStorage.removeItem(TOKEN_KEY);
      window.location.href = '/admin/login';
    }
    return Promise.reject(error);
  },
);

export const login = (username: string, password: string) =>
  api.put('/auth/login', { username, password });

export const getSetupStatus = () => api.get('/auth/setup-status');

export const setup = (username: string, password: string) =>
  api.post('/auth/setup', { username, password });

export const getConfig = () => api.get('/config');
export const updateConfig = (config: unknown) => api.put('/config', config);
export const getRules = () => api.get('/rules');
export const createRule = (rule: unknown) => api.post('/rules', rule);
export const updateRule = (id: string, rule: unknown) => api.put(`/rules/${id}`, rule);
export const deleteRule = (id: string) => api.delete(`/rules/${id}`);
export const getUpstreams = () => api.get('/upstreams');
export const createUpstream = (upstream: unknown) => api.post('/upstreams', upstream);
export const updateUpstream = (id: string, upstream: unknown) => api.put(`/upstreams/${id}`, upstream);
export const deleteUpstream = (id: string) => api.delete(`/upstreams/${id}`);
export const getMetrics = () => axios.get('/metrics');
