import axios from 'axios';

const api = axios.create({ baseURL: '/api' });

export const getConfig = () => api.get('/config');
export const updateConfig = (config: unknown) => api.put('/config', config);
export const getRules = () => api.get('/rules');
export const createRule = (rule: unknown) => api.post('/rules', rule);
export const updateRule = (id: string, rule: unknown) => api.put(`/rules/${id}`, rule);
export const deleteRule = (id: string) => api.delete(`/rules/${id}`);
export const getUpstreams = () => api.get('/upstreams');
export const getMetrics = () => axios.get('/metrics');
