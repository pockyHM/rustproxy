import { FormEvent, ReactNode, createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import yaml from 'js-yaml';

/* ===== Types ===== */

type Lang = 'en' | 'zh';
type Theme = 'light' | 'dark';
type ApiResponse<T> = { success: boolean; data: T; error?: string };
type Target = { url: string; weight: number };
type HealthCheckMode = 'tcp' | 'http';
type HealthCheck = {
  enabled: boolean;
  mode: HealthCheckMode;
  path: string;
  expected_status: number;
  interval_seconds: number;
  timeout_seconds: number;
  healthy_threshold: number;
  unhealthy_threshold: number;
};
type Upstream = {
  name: string;
  skip_ssl?: boolean;
  websocket?: boolean;
  targets: Target[];
  health_check?: Partial<HealthCheck>;
};
type Certificate = { name: string; cert: string; key: string };
type TlsListener = { enabled: boolean; listen: string; certificate: string };
type RuleTls = { enabled: boolean; certificate: string };
type ConditionType = 'host' | 'path' | 'header' | 'cookie' | 'jwt';
type Operator = 'exact' | 'prefix' | 'regex' | 'exists' | 'contains';
type ConditionExpr =
  | { type: 'leaf'; conditionType: ConditionType; key?: string | null; claimPath?: string | null; operator: Operator; value?: string | null }
  | { type: 'and'; children: ConditionExpr[] }
  | { type: 'or'; children: ConditionExpr[] };
type MatchSet = { name: string; conditions?: ConditionExpr | null };
type Rule = {
  id: string;
  name: string;
  priority: number;
  match_set?: string | null;
  conditions?: ConditionExpr | null;
  upstream: string;
  weight: number;
  is_fallback?: boolean;
  listen?: string | null;
  tls?: RuleTls | null;
};
type AppConfig = {
  version: string;
  listen: string;
  proxy_listen?: string;
  connect_timeout?: number;
  request_timeout?: number;
  pool_max_idle_per_host?: number;
  pool_idle_timeout?: number;
  tcp_keepalive?: number;
  certificate_dir?: string;
  certificates?: Certificate[];
  tls_listeners?: TlsListener[];
  match_sets?: MatchSet[];
  rules: Rule[];
  upstreams: Record<string, Upstream>;
  fallback: { url: string };
};

type View = 'operations' | 'rules' | 'match-sets' | 'upstreams' | 'certificates' | 'config';
type Notice = { type: 'success' | 'error'; message: string } | null;
type DataProps = { config: AppConfig; token: string; setConfig: (config: AppConfig) => void; setNotice: (notice: Notice) => void };
type DropdownOption = { value: string; label: string; description?: string };

type PrometheusMetric = {
  name: string;
  labels: Record<string, string>;
  value: number;
};

/* ===== Translations ===== */

const T: Record<string, [string, string]> = {
  'brand.subtitle': ['Reverse Proxy Admin', '反向代理管理'],
  'nav.control': ['CONTROL', '控制'],
  'nav.operations': ['Operations', '运维概览'],
  'nav.rules': ['Rules', '路由规则'],
  'nav.matchSets': ['Match Sets', '匹配集'],
  'nav.upstreams': ['Upstreams', '上游服务'],
  'nav.certificates': ['Certificates', '证书'],
  'nav.config': ['Config File', '配置文件'],
  'listeners.title': ['LISTENERS', '监听器'],
  'listeners.api': ['API + Admin UI', 'API + 管理界面'],
  'listeners.proxy': ['Reverse proxy', '反向代理'],
  'listeners.tls': ['HTTPS proxy', 'HTTPS 代理'],
  'admin.role': ['JWT protected', 'JWT 保护'],
  'admin.logout': ['Sign out', '退出登录'],
  'theme.light': ['Light mode', '亮色模式'],
  'theme.dark': ['Dark mode', '暗色模式'],
  'ops.title': ['Operations Overview', '运维概览'],
  'ops.sub': ['Live proxy health, weighted routing, reload status, and request pressure across listeners.', '代理健康状态、加权路由、重载状态和请求压力概览。'],
  'rules.title': ['Routing Rules', '路由规则'],
  'rules.sub': ['Manage request matching rules with priority-based routing to upstream pools.', '管理请求匹配规则，按优先级路由到上游池。'],
  'matchSets.title': ['Match Sets', '匹配集'],
  'matchSets.sub': ['Create reusable request match trees and attach them to routing rules.', '创建可复用的请求匹配树，并在路由规则中引用。'],
  'matchSets.empty': ['No match sets configured', '暂无匹配集'],
  'upstreams.title': ['Upstreams', '上游服务'],
  'upstreams.sub': ['Manage upstream target pools with weighted load balancing and health checks.', '管理上游目标池，支持加权负载均衡和健康检查。'],
  'config.title': ['Config File', '配置文件'],
  'config.sub': ['Edit the active YAML config and global runtime options. Changes are applied via hot-reload.', '编辑当前 YAML 配置和全局运行选项，更改通过热重载生效。'],
  'config.global': ['Global Config', '全局配置'],
  'config.loadAdvanced': ['Load Advanced Config', '负载高级配置'],
  'config.listen': ['Admin listen', '管理端监听'],
  'config.proxyListen': ['Proxy listen', '代理监听'],
  'config.tlsListeners': ['HTTPS Listeners', 'HTTPS 监听'],
  'config.certificates': ['Certificates', '证书'],
  'config.certificateDir': ['Certificate directory', '证书目录'],
  'config.certName': ['Certificate name', '证书名称'],
  'config.certFile': ['Certificate / chain', '证书 / 证书链'],
  'config.keyFile': ['Private key', '私钥'],
  'config.certFormat': ['PEM, CRT, CER, base64 DER', 'PEM、CRT、CER、base64 DER'],
  'config.keyFormat': ['PEM KEY, base64 DER PKCS#8/RSA/EC', 'PEM KEY、base64 DER PKCS#8/RSA/EC'],
  'config.addCertificate': ['Add certificate', '添加证书'],
  'config.addTlsListener': ['Add HTTPS listener', '添加 HTTPS 监听'],
  'config.noCertificates': ['Upload a certificate before enabling HTTPS listeners.', '启用 HTTPS 监听前请先上传证书。'],
  'config.tlsRestart': ['Proxy listener ports are hot-updated; changing the admin listen address still requires a restart.', '代理监听端口会热更新；修改管理端监听地址仍需要重启。'],
  'cert.title': ['Certificates', '证书管理'],
  'cert.sub': ['Upload reusable TLS certificates for HTTPS rule listeners.', '上传可复用的 TLS 证书，用于规则级 HTTPS 监听。'],
  'cert.empty': ['No certificates uploaded yet.', '还没有上传证书。'],
  'cert.saved': ['Certificates saved', '证书已保存'],
  'rule.tls': ['TLS listener', 'TLS 监听'],
  'rule.enableTls': ['Enable HTTPS for this rule', '为此规则开启 HTTPS'],
  'rule.tlsHelp': ['Rules sharing one HTTPS port can use different certificates when SNI and exact Host conditions are configured.', '多个规则可以共用同一个 HTTPS 端口；配置精确 Host 条件并且客户端发送 SNI 时，可选择不同证书。'],
  'rule.noCertificates': ['Upload a certificate first from the Certificates menu.', '请先在证书菜单上传证书。'],
  'rule.protocolConflict': ['This port is already used by the other protocol. HTTP and HTTPS cannot share one port yet.', '该端口已经被另一种协议占用，当前暂不支持 HTTP 和 HTTPS 共用同一端口。'],
  'rule.fallback': ['Fallback rule', '兜底规则'],
  'rule.enableFallback': ['Use when no other rule matches', '所有规则都未命中时使用'],
  'rule.fallbackHelp': ['Fallback rules run after normal rules and only need an upstream.', '兜底规则会在普通规则全部未命中后执行，只需要选择上游。'],
  'config.fallbackUrl': ['Fallback target', '兜底目标'],
  'config.skipSsl': ['Skip SSL verification', '跳过 SSL 验证'],
  'config.websocket': ['Enable WebSocket proxy', '启用 WebSocket 代理'],
  'config.connectTimeout': ['Connect timeout (s)', '连接超时 (秒)'],
  'config.requestTimeout': ['Request timeout (s)', '请求超时 (秒)'],
  'config.poolMaxIdle': ['Max idle per host', '每主机最大空闲连接'],
  'config.poolIdleTimeout': ['Pool idle timeout (s)', '连接池空闲超时 (秒)'],
  'config.tcpKeepalive': ['TCP keepalive (s)', 'TCP Keepalive (秒)'],
  'action.reload': ['Reload config', '重新加载'],
  'action.newRule': ['New rule', '新建规则'],
  'action.newMatchSet': ['New match set', '新建匹配集'],
  'action.newUpstream': ['New upstream', '新建上游'],
  'action.save': ['Save', '保存'],
  'action.cancel': ['Cancel', '取消'],
  'action.create': ['Create', '创建'],
  'action.edit': ['Edit', '编辑'],
  'action.del': ['Del', '删除'],
  'action.addTarget': ['Add Target', '添加目标'],
  'metric.requests': ['PROXY REQUESTS', '代理请求数'],
  'metric.latency': ['AVG LATENCY', '平均延迟'],
  'metric.conns': ['ACTIVE CONNECTIONS', '活跃连接'],
  'metric.reloads': ['CONFIG RELOAD', '配置重载'],
  'metric.live': ['LIVE', '实时'],
  'metric.ms': ['MS', '毫秒'],
  'metric.now': ['NOW', '当前'],
  'ops.matcher': ['MATCHER + BALANCER', '匹配器 + 负载均衡'],
  'ops.matching': ['Live request routing chain', '实时请求路由链路'],
  'ops.matchingDesc': ['Requests enter through a listener, match rules by priority, then flow into an upstream target pool.', '请求从入口监听进入，按优先级匹配规则，再流向对应的上游目标池。'],
  'ops.entry': ['Entry', '入口'],
  'ops.priorityShort': ['P', 'P'],
  'ops.requestsShort': ['req', '请求'],
  'ops.noRoutes': ['No routing rules configured. Traffic falls back to the default upstream URL.', '暂无路由规则，流量会进入默认兜底上游 URL。'],
  'ops.traffic': ['Recent proxy traffic', '近期代理流量'],
  'ops.trafficDesc': ['Requests grouped by rule, upstream, status, and latency histogram bucket.', '按规则、上游、状态和延迟直方图桶分组的请求。'],
  'ops.filterRules': ['Filter rules...', '过滤规则...'],
  'ops.noTraffic': ['No traffic data yet. Metrics will appear when the proxy receives requests.', '暂无流量数据。当代理收到请求时指标将出现。'],
  'ops.last24h': ['Last 24h across all upstreams', '过去 24 小时所有上游'],
  'ops.histogram': ['Histogram proxy_request_duration_seconds', '直方图 proxy_request_duration_seconds'],
  'ops.gauge': ['Gauge proxy_active_connections', '指标 proxy_active_connections'],
  'ops.sqliteReload': ['SQLite backed hot reload loop', 'SQLite 热重载'],
  'table.rule': ['Rule', '规则'],
  'table.upstream': ['Upstream', '上游'],
  'table.status': ['Status', '状态'],
  'table.requests': ['Requests', '请求数'],
  'table.id': ['ID', 'ID'],
  'table.name': ['Name', '名称'],
  'table.priority': ['Priority', '优先级'],
  'table.listen': ['Listen', '监听地址'],
  'table.pool': ['Upstream Pool', '上游池'],
  'table.weight': ['Weight', '权重'],
  'table.match': ['Match', '匹配'],
  'table.actions': ['Actions', '操作'],
  'table.targets': ['Targets', '目标'],
  'table.healthCheck': ['Health Check', '健康检查'],
  'table.off': ['Off', '关闭'],
  'table.noRules': ['No rules configured', '暂无路由规则'],
  'table.noUpstreams': ['No upstreams configured', '暂无上游服务'],
  'inspector.status': ['Runtime status', '运行时状态'],
  'inspector.proxyOk': ['Proxy listener forwarding traffic', '代理监听器正在转发流量'],
  'inspector.apiOk': ['Admin API serving /admin/ and /metrics', '管理 API 提供 /admin/ 和 /metrics'],
  'inspector.wsEnabled': ['WebSocket proxy enabled', 'WebSocket 代理已启用'],
  'inspector.wsDisabled': ['WebSocket proxy disabled', 'WebSocket 代理未启用'],
  'inspector.snapshot': ['Config snapshot', '配置快照'],
  'inspector.prometheus': ['Prometheus', 'Prometheus'],
  'inspector.prometheusDesc': ['Runtime resource gauges scraped from /metrics.', '从 /metrics 抓取的运行时资源指标。'],
  'inspector.memory': ['Memory', '内存'],
  'inspector.cpu': ['CPU', 'CPU'],
  'inspector.connections': ['Connections', '连接数'],
  'inspector.fds': ['Open FDs', '打开 FD'],
  'inspector.cpuHint': ['process CPU', '进程占用'],
  'inspector.rssHint': ['resident set', '常驻内存'],
  'inspector.fdHint': ['file descriptors', '文件描述符'],
  'form.condition': ['Condition', '匹配条件'],
  'form.identity': ['Identity', '基础信息'],
  'form.routing': ['Routing target', '路由目标'],
  'form.matching': ['Match condition', '匹配条件'],
  'form.matchSource': ['Match source', '匹配来源'],
  'form.inlineMatch': ['Inline condition', '规则内新建匹配'],
  'form.reusableMatch': ['Reusable match set', '复用匹配集'],
  'form.pool': ['Pool details', '上游池信息'],
  'form.nodeType': ['Node type', '节点类型'],
  'form.leaf': ['Condition', '条件'],
  'form.and': ['AND group', 'AND 条件组'],
  'form.or': ['OR group', 'OR 条件组'],
  'form.addCondition': ['Add condition', '添加条件'],
  'form.addGroup': ['Add group', '添加条件组'],
  'form.type': ['Type', '类型'],
  'form.type.host': ['Match request host', '匹配请求 Host'],
  'form.type.path': ['Match request path', '匹配请求路径'],
  'form.type.header': ['Match request header', '匹配请求头'],
  'form.type.cookie': ['Match cookie value', '匹配 Cookie 值'],
  'form.type.jwt': ['Match JWT claim', '匹配 JWT Claim'],
  'form.key': ['Key', '键'],
  'form.operator': ['Operator', '操作符'],
  'form.value': ['Value', '值'],
  'form.targets': ['Targets', '目标'],
  'health.title': ['Health Check', '健康检查'],
  'health.desc': ['Probe targets in the background and skip endpoints after repeated failures.', '后台探测目标并在反复失败后跳过端点。'],
  'health.enabled': ['Enabled', '已启用'],
  'health.disabled': ['Disabled', '已禁用'],
  'health.tcp': ['TCP port', 'TCP 端口'],
  'health.tcpDesc': ['Connect to host and port only', '仅连接主机和端口'],
  'health.http': ['HTTP endpoint', 'HTTP 端点'],
  'health.httpDesc': ['Expect a specific status code', '期望特定的状态码'],
  'health.path': ['Path', '路径'],
  'health.expectedStatus': ['Expected status', '期望状态码'],
  'health.interval': ['Interval (s)', '间隔 (秒)'],
  'health.timeout': ['Timeout (s)', '超时 (秒)'],
  'health.healthyThreshold': ['Healthy threshold', '健康阈值'],
  'health.unhealthyThreshold': ['Unhealthy threshold', '不健康阈值'],
  'health.tcpPreview': ['TCP mode checks only whether each target port accepts connections.', 'TCP 模式仅检查每个目标端口是否接受连接。'],
  'health.hostPort': ['host:port', '主机:端口'],
  'modal.editRule': ['Edit Rule', '编辑规则'],
  'modal.newRule': ['New Rule', '新建规则'],
  'modal.editMatchSet': ['Edit Match Set', '编辑匹配集'],
  'modal.newMatchSet': ['New Match Set', '新建匹配集'],
  'modal.editUpstream': ['Edit Upstream', '编辑上游'],
  'modal.newUpstream': ['New Upstream', '新建上游'],
  'config.schema': ['Schema Reference', 'Schema 参考'],
  'config.validation': ['Validation', '验证'],
  'config.store': ['SQLite Store', 'SQLite 存储'],
  'config.valid': ['YAML syntax valid', 'YAML 语法有效'],
  'config.invalid': ['YAML syntax invalid', 'YAML 语法无效'],
  'config.poolsDefined': ['upstream pools defined', '个上游池已定义'],
  'config.routesConfigured': ['route rules configured', '条路由规则已配置'],
  'config.lastWrite': ['Last write', '最后写入'],
  'config.size': ['Size', '大小'],
  'auth.createAccount': ['Create Admin Account', '创建管理员账户'],
  'auth.createDesc': ['Set up your administrator credentials to get started.', '设置管理员凭据以开始使用。'],
  'auth.username': ['Username', '用户名'],
  'auth.email': ['Email', '邮箱'],
  'auth.password': ['Password', '密码'],
  'auth.confirmPassword': ['Confirm Password', '确认密码'],
  'auth.createBtn': ['Create Account', '创建账户'],
  'auth.or': ['or', '或'],
  'auth.hasAccount': ['Already have an account?', '已有账户？'],
  'auth.signIn': ['Sign in', '登录'],
  'auth.welcome': ['Welcome Back', '欢迎回来'],
  'auth.signInDesc': ['Sign in to access your proxy dashboard.', '登录以访问代理仪表盘。'],
  'auth.signInBtn': ['Sign In', '登录'],
  'notice.configReloaded': ['Config reloaded', '配置已重新加载'],
  'notice.ruleCreated': ['Rule created', '规则已创建'],
  'notice.ruleUpdated': ['Rule updated', '规则已更新'],
  'notice.ruleDeleted': ['Rule deleted', '规则已删除'],
  'notice.matchSetCreated': ['Match set created', '匹配集已创建'],
  'notice.matchSetUpdated': ['Match set updated', '匹配集已更新'],
  'notice.matchSetDeleted': ['Match set deleted', '匹配集已删除'],
  'notice.upstreamCreated': ['Upstream created', '上游已创建'],
  'notice.upstreamUpdated': ['Upstream updated', '上游已更新'],
  'notice.upstreamDeleted': ['Upstream deleted', '上游已删除'],
  'notice.configSaved': ['Config saved', '配置已保存'],
  'notice.adminCreated': ['Admin created — please sign in', '管理员已创建 — 请登录'],
  'notice.passwordMismatch': ['Passwords do not match', '密码不匹配'],
  'notice.connecting': ['connecting...', '连接中...'],
};

/* ===== I18n Context ===== */

const I18nCtx = createContext<{ lang: Lang; t: (key: string) => string }>({ lang: 'zh', t: (k) => k });
const useI18n = () => useContext(I18nCtx);

/* ===== Constants ===== */

const defaultHealthCheck: HealthCheck = {
  enabled: false, mode: 'tcp', path: '/health', expected_status: 200,
  interval_seconds: 10, timeout_seconds: 2, healthy_threshold: 2, unhealthy_threshold: 2,
};

const NAV_ITEMS: { id: View; labelKey: string; icon: string }[] = [
  { id: 'operations', labelKey: 'nav.operations', icon: 'monitoring' },
  { id: 'rules', labelKey: 'nav.rules', icon: 'route' },
  { id: 'match-sets', labelKey: 'nav.matchSets', icon: 'rule_settings' },
  { id: 'upstreams', labelKey: 'nav.upstreams', icon: 'lan' },
  { id: 'certificates', labelKey: 'nav.certificates', icon: 'workspace_premium' },
  { id: 'config', labelKey: 'nav.config', icon: 'database' },
];

const ROUTE_FLOW_COLORS = ['#FF8400', '#000066', '#804200', '#004D1A'];

const emptyConfig: AppConfig = {
  version: '1.0', listen: '127.0.0.1:3000', proxy_listen: '0.0.0.0:80',
  certificate_dir: '/etc/rustproxy/cert.d', certificates: [], tls_listeners: [], match_sets: [],
  rules: [], upstreams: {}, fallback: { url: '404' },
};

/* ===== Icon ===== */

function Icon({ name, size = 22 }: { name: string; size?: number }) {
  return <span className="material-symbols-sharp" style={{ fontSize: size }}>{name}</span>;
}

/* ===== Shared Components ===== */

function Field({ label, children }: { label: string; children: ReactNode }) {
  return <label className="field"><span className="field-label">{label}</span>{children}</label>;
}

function Dropdown({ value, options, onChange }: { value: string; options: DropdownOption[]; onChange: (value: string) => void }) {
  const [open, setOpen] = useState(false);
  const selected = options.find((option) => option.value === value) ?? options[0];

  function choose(nextValue: string) {
    onChange(nextValue);
    setOpen(false);
  }

  return (
    <div className={open ? 'dropdown is-open' : 'dropdown'} onBlur={(event) => {
      if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setOpen(false);
    }}>
      <button type="button" className="dropdown-trigger" onClick={() => setOpen((next) => !next)}>
        <span className="dropdown-value">{selected?.label ?? '—'}</span>
        <Icon name={open ? 'keyboard_arrow_up' : 'keyboard_arrow_down'} size={16} />
      </button>
      {open && (
        <div className="dropdown-panel">
          {options.map((option) => (
            <button
              type="button"
              key={option.value}
              className={option.value === value ? 'dropdown-option is-selected' : 'dropdown-option'}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => choose(option.value)}
            >
              <span className="dropdown-option-copy">
                <span className="dropdown-option-label">{option.label}</span>
                {option.description && <span className="dropdown-option-description">{option.description}</span>}
              </span>
              {option.value === value && <Icon name="check" size={16} />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function Toast({ notice, onClose }: { notice: NonNullable<Notice>; onClose: () => void }) {
  useEffect(() => {
    const timer = window.setTimeout(onClose, 3600);
    return () => window.clearTimeout(timer);
  }, [notice.message, notice.type, onClose]);

  return (
    <div className={`toast ${notice.type}`} role="status" aria-live="polite">
      <span>{notice.message}</span>
      <button className="toast-close" onClick={onClose} aria-label="Close notification">&times;</button>
    </div>
  );
}

function Splash({ label }: { label: string }) {
  return (
    <main className="splash-screen">
      <div className="splash-logo">
        <img src="/admin/favicon.svg" width="32" height="32" alt="" />
        <span>RustProxy</span>
      </div>
      <p>{label}</p>
    </main>
  );
}

function Modal({ title, onClose, children }: { title: string; onClose: () => void; children: ReactNode }) {
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <div className="modal-title-wrap">
            <span className="modal-title-mark"><Icon name="rule_settings" size={18} /></span>
            <h2 className="modal-title">{title}</h2>
          </div>
          <button className="modal-close" onClick={onClose} aria-label="Close"><Icon name="close" size={18} /></button>
        </div>
        <div className="modal-body">{children}</div>
      </div>
    </div>
  );
}

function ViewHeader({ title, subtitle, actions }: { title: string; subtitle: string; actions?: ReactNode }) {
  return (
    <header className="page-header">
      <div className="page-heading">
        <h1 className="page-title">{title}</h1>
        <p className="page-subtitle">{subtitle}</p>
      </div>
      {actions && <div className="page-actions">{actions}</div>}
    </header>
  );
}

/* ===== App ===== */

function App() {
  const [token, setToken] = useState(() => localStorage.getItem('rustproxy_token') ?? '');
  const [needsSetup, setNeedsSetup] = useState<boolean | null>(null);
  const [config, setConfig] = useState<AppConfig>(emptyConfig);
  const [view, setView] = useState<View>(() => resolveInitialView());
  const [notice, setNotice] = useState<Notice>(null);
  const [loading, setLoading] = useState(true);
  const [lang, setLang] = useState<Lang>(() => (localStorage.getItem('rustproxy_lang') as Lang) || 'zh');
  const [theme, setTheme] = useState<Theme>(() => (localStorage.getItem('rustproxy_theme') as Theme) || 'light');

  const t = useMemo(() => {
    const idx = lang === 'en' ? 0 : 1;
    return (key: string) => T[key]?.[idx] ?? key;
  }, [lang]);

  const changeLang = useCallback((l: Lang) => { setLang(l); localStorage.setItem('rustproxy_lang', l); }, []);
  const changeTheme = useCallback((nextTheme: Theme) => {
    setTheme(nextTheme);
    localStorage.setItem('rustproxy_theme', nextTheme);
  }, []);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  useEffect(() => {
    api<{ users_exist: boolean }>('/api/auth/setup-status')
      .then((data) => setNeedsSetup(!data.users_exist))
      .catch(() => setNeedsSetup(false));
  }, []);

  useEffect(() => {
    if (!token || needsSetup) { setLoading(false); return; }
    refreshConfig(token, setConfig, setNotice).finally(() => setLoading(false));
  }, [token, needsSetup]);

  function navigate(next: View) {
    setView(next);
    window.history.replaceState(null, '', `/admin/${next === 'operations' ? '' : next}`);
  }

  function handleAuth(nextToken: string) {
    localStorage.setItem('rustproxy_token', nextToken);
    setToken(nextToken);
    setNeedsSetup(false);
  }

  function logout() { localStorage.removeItem('rustproxy_token'); setToken(''); }

  if (needsSetup === null || loading) return <I18nCtx.Provider value={{ lang, t }}><Splash label={t('notice.connecting')} /></I18nCtx.Provider>;
  if (needsSetup) return <I18nCtx.Provider value={{ lang, t }}><SetupScreen onDone={() => setNeedsSetup(false)} setNotice={setNotice} notice={notice} /></I18nCtx.Provider>;
  if (!token) return <I18nCtx.Provider value={{ lang, t }}><LoginScreen onDone={handleAuth} notice={notice} setNotice={setNotice} /></I18nCtx.Provider>;

  return (
    <I18nCtx.Provider value={{ lang, t }}>
      <div className="shell">
        <Sidebar view={view} navigate={navigate} config={config} logout={logout} lang={lang} changeLang={changeLang} theme={theme} changeTheme={changeTheme} />
        <main className="workspace">
          {notice && <Toast notice={notice} onClose={() => setNotice(null)} />}
          {view === 'operations' && <OperationsView config={config} token={token} />}
          {view === 'rules' && <RulesView config={config} token={token} setConfig={setConfig} setNotice={setNotice} />}
          {view === 'match-sets' && <MatchSetsView config={config} token={token} setConfig={setConfig} setNotice={setNotice} />}
          {view === 'upstreams' && <UpstreamsView config={config} token={token} setConfig={setConfig} setNotice={setNotice} />}
          {view === 'certificates' && <CertificatesView config={config} token={token} setConfig={setConfig} setNotice={setNotice} />}
          {view === 'config' && <ConfigView config={config} token={token} setConfig={setConfig} setNotice={setNotice} />}
        </main>
      </div>
    </I18nCtx.Provider>
  );
}

/* ===== Sidebar ===== */

function Sidebar({ view, navigate, config, logout, lang, changeLang, theme, changeTheme }: {
  view: View; navigate: (v: View) => void; config: AppConfig; logout: () => void; lang: Lang; changeLang: (l: Lang) => void; theme: Theme; changeTheme: (theme: Theme) => void;
}) {
  const { t } = useI18n();
  return (
    <aside className="sidebar">
      <div className="sidebar-brand">
        <div className="sidebar-logo"><img src="/admin/favicon.svg" alt="RustProxy" /></div>
        <div className="sidebar-brand-text">
          <div className="sidebar-name">RustProxy</div>
          <div className="sidebar-subtitle">{t('brand.subtitle')}</div>
        </div>
      </div>
      <div className="sidebar-nav-title">{t('nav.control')}</div>
      <nav className="sidebar-nav">
        {NAV_ITEMS.map((item) => (
          <button key={item.id} className={view === item.id ? 'nav-item is-active' : 'nav-item'} onClick={() => navigate(item.id)}>
            <Icon name={item.icon} />
            <span>{t(item.labelKey)}</span>
          </button>
        ))}
      </nav>
      <div className="sidebar-listeners">
        <div className="listeners-title">{t('listeners.title')}</div>
        <div className="listener-item">
          <div className="listener-label">{t('listeners.api')}</div>
          <div className="listener-value">{config.listen || '127.0.0.1:3000'}</div>
        </div>
        <div className="listener-item">
          <div className="listener-label">{t('listeners.proxy')}</div>
          <div className="listener-value">{config.proxy_listen || '0.0.0.0:80'}</div>
        </div>
        {tlsRuleListeners(config).slice(0, 2).map((listener) => (
          <div className="listener-item" key={listener}>
            <div className="listener-label">{t('listeners.tls')}</div>
            <div className="listener-value">{listener}</div>
          </div>
        ))}
      </div>
      <div className="sidebar-spacer" />
      <div className="sidebar-utility-actions">
        <button className="sidebar-toggle" onClick={() => changeTheme(theme === 'light' ? 'dark' : 'light')} title={theme === 'light' ? t('theme.dark') : t('theme.light')}>
          <Icon name={theme === 'light' ? 'dark_mode' : 'light_mode'} size={16} />
          <span>{theme === 'light' ? t('theme.dark') : t('theme.light')}</span>
        </button>
        <button className="sidebar-toggle" onClick={() => changeLang(lang === 'en' ? 'zh' : 'en')}>
          <Icon name="language" size={16} />
          {lang === 'en' ? '中文' : 'EN'}
        </button>
      </div>
      <div className="sidebar-admin">
        <div className="sidebar-avatar">AD</div>
        <div className="sidebar-admin-meta">
          <div className="sidebar-admin-name">admin</div>
          <div className="sidebar-admin-role">{t('admin.role')}</div>
        </div>
        <button className="btn btn-ghost" style={{ marginLeft: 'auto', padding: 4 }} onClick={logout} title={t('admin.logout')}>
          <Icon name="logout" size={18} />
        </button>
      </div>
    </aside>
  );
}

/* ===== Operations Overview ===== */

function OperationsView({ config, token }: { config: AppConfig; token: string }) {
  const { lang, t } = useI18n();
  const [metrics, setMetrics] = useState<PrometheusMetric[]>([]);
  const [cpuUsage, setCpuUsage] = useState(0);
  const previousCpu = useRef<{ total: number; at: number } | null>(null);
  useEffect(() => {
    let active = true;
    async function loadMetrics() {
      try {
        const text = await fetch('/metrics').then((r) => r.text());
        if (!active) return;
        const nextMetrics = parsePrometheus(text);
        const cpuTotal = metricValue(nextMetrics, 'process_cpu_seconds_total');
        const now = Date.now();
        if (cpuTotal !== null && previousCpu.current) {
          const cpuDelta = Math.max(0, cpuTotal - previousCpu.current.total);
          const secondsDelta = Math.max(1, (now - previousCpu.current.at) / 1000);
          setCpuUsage((cpuDelta / secondsDelta) * 100);
        }
        if (cpuTotal !== null) previousCpu.current = { total: cpuTotal, at: now };
        setMetrics(nextMetrics);
      } catch (_) {}
    }
    loadMetrics();
    const timer = window.setInterval(loadMetrics, 5000);
    return () => { active = false; window.clearInterval(timer); };
  }, []);

  const totalRequests = useMemo(() => metrics.filter((m) => m.name === 'proxy_requests_total').reduce((s, m) => s + m.value, 0), [metrics]);
  const activeConns = useMemo(() => metrics.find((m) => m.name === 'proxy_active_connections')?.value ?? 0, [metrics]);
  const residentMemory = useMemo(() => metricValue(metrics, 'process_resident_memory_bytes') ?? 0, [metrics]);
  const openFds = useMemo(() => metricValue(metrics, 'process_open_fds') ?? 0, [metrics]);
  const avgLatency = useMemo(() => {
    const sum = metrics.find((m) => m.name === 'proxy_request_duration_seconds_sum')?.value;
    const count = metrics.find((m) => m.name === 'proxy_request_duration_seconds_count')?.value;
    return sum && count && count > 0 ? (sum / count) * 1000 : 0;
  }, [metrics]);
  const reloadCount = useMemo(() => metrics.find((m) => m.name === 'proxy_config_reloads_total')?.value ?? 0, [metrics]);

  const ruleNameById = useMemo(() => Object.fromEntries(config.rules.map((r) => [r.id, r.name || r.id])), [config.rules]);
  const requestsByRule = useMemo(() => {
    const byRule: Record<string, number> = {};
    metrics.filter((m) => m.name === 'proxy_requests_total').forEach((m) => {
      const rule = m.labels.rule || 'unknown';
      byRule[rule] = (byRule[rule] || 0) + m.value;
    });
    return byRule;
  }, [metrics]);

  const trafficRows = useMemo(() => metrics.filter((m) => m.name === 'proxy_requests_total').map((m) => ({
    rule: ruleNameById[m.labels.rule] || m.labels.rule || '-', ruleId: m.labels.rule || '-',
    upstream: m.labels.upstream || '-', status: m.labels.status || '-', count: m.value,
  })), [metrics, ruleNameById]);
  const upstreams = useMemo(() => Object.values(config.upstreams ?? {}), [config]);
  const routeFlowGroups = useMemo(() => {
    const defaultListen = config.proxy_listen || '0.0.0.0:80';
    const grouped = new Map<string, {
      entry: string;
      color: string;
      flows: {
        rule: Rule;
        upstream?: Upstream;
        requests: number;
        color: string;
      }[];
    }>();
    [...config.rules]
      .sort((a, b) => Number(a.is_fallback) - Number(b.is_fallback) || b.priority - a.priority)
      .forEach((rule) => {
        const entry = `${rule.tls?.enabled ? 'https' : 'http'}://${rule.listen || defaultListen}`;
        if (!grouped.has(entry)) {
          grouped.set(entry, { entry, color: ROUTE_FLOW_COLORS[grouped.size % ROUTE_FLOW_COLORS.length], flows: [] });
        }
        const group = grouped.get(entry)!;
        group.flows.push({
          rule,
          upstream: config.upstreams?.[rule.upstream],
          requests: requestsByRule[rule.id] ?? 0,
          color: ROUTE_FLOW_COLORS[group.flows.length % ROUTE_FLOW_COLORS.length],
        });
      });
    return [...grouped.values()];
  }, [config, requestsByRule]);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 16, flex: 1 }}>
      <ViewHeader title={t('ops.title')} subtitle={t('ops.sub')} />
      <section className="metrics-strip">
        <div className="metric-card">
          <div className="metric-header"><span className="metric-label">{t('metric.requests')}</span><span className="metric-badge badge-success">{t('metric.live')}</span></div>
          <div className="metric-value">{formatNumber(totalRequests)}</div>
          <div className="metric-detail">{t('ops.last24h')}</div>
        </div>
        <div className="metric-card">
          <div className="metric-header"><span className="metric-label">{t('metric.latency')}</span><span className="metric-badge badge-warning">{t('metric.ms')}</span></div>
          <div className="metric-value">{avgLatency > 0 ? `${Math.round(avgLatency)} ms` : '—'}</div>
          <div className="metric-detail">{t('ops.histogram')}</div>
        </div>
        <div className="metric-card">
          <div className="metric-header"><span className="metric-label">{t('metric.conns')}</span><span className="metric-badge badge-info">{t('metric.now')}</span></div>
          <div className="metric-value">{formatNumber(activeConns)}</div>
          <div className="metric-detail">{t('ops.gauge')}</div>
        </div>
        <div className="metric-card">
          <div className="metric-header"><span className="metric-label">{t('metric.reloads')}</span><span className="metric-badge badge-warning">{reloadCount}</span></div>
          <div className="metric-value">OK</div>
          <div className="metric-detail">{t('ops.sqliteReload')}</div>
        </div>
      </section>
      <div className="ops-body">
        <div className="ops-primary">
          <div className="routing-health">
            <div className="routing-health-copy">
              <span className="health-eyebrow">{t('ops.matcher')}</span>
              <h2 className="health-title">{t('ops.matching')}</h2>
              <p className="health-desc">{t('ops.matchingDesc')}</p>
              <div className="health-alert success">
                <Icon name="check_circle" size={18} />
                {lang === 'en'
                  ? `${config.rules.length} rule${config.rules.length !== 1 ? 's' : ''} active, ${upstreams.length} upstream pool${upstreams.length !== 1 ? 's' : ''}`
                  : `${config.rules.length} 条规则激活，${upstreams.length} 个上游池`}
              </div>
            </div>
            <div className="routing-flow-list">
              {routeFlowGroups.length > 0 ? routeFlowGroups.map((group) => (
                <div className="flow-group" key={group.entry} style={{ borderLeftColor: group.color }}>
                  <div className="flow-node flow-entry">
                    <span className="flow-node-label">{t('ops.entry')}</span>
                    <strong>{group.entry}</strong>
                    <small>{lang === 'en' ? `${group.flows.length} route${group.flows.length !== 1 ? 's' : ''}` : `${group.flows.length} 条链路`}</small>
                  </div>
                  <div className="flow-branches">
                    {group.flows.map(({ rule, upstream, requests, color }, index) => (
                      <div className={`flow-branch ${rule.is_fallback ? 'is-fallback' : ''}`} key={rule.id || `${group.entry}-${index}`}>
                        <span className="flow-branch-rail" style={{ background: color }} />
                        <div className="flow-priority">
                          <span>{rule.is_fallback ? t('rule.fallback') : `${t('ops.priorityShort')}${rule.priority}`}</span>
                          {rule.is_fallback && <Icon name="shield" size={15} />}
                        </div>
                        <Icon name="arrow_forward" size={16} />
                        <div className="flow-node flow-rule-name">
                          <span className="flow-node-label">{t('table.rule')}</span>
                          <strong title={rule.id}>{rule.name || rule.id || '-'}</strong>
                        </div>
                        <Icon name="arrow_forward" size={16} />
                        <div className="flow-node flow-upstream">
                          <span className="flow-node-label">{t('table.upstream')}</span>
                          <strong>{rule.upstream}</strong>
                          <small>{upstream ? targetSummary(upstream, lang) : 'missing upstream'}</small>
                        </div>
                        <div className="flow-count">{formatNumber(requests)} {t('ops.requestsShort')}</div>
                        <div className="flow-rule-popover" role="tooltip">
                          <span>{t('table.rule')} · {rule.is_fallback ? t('rule.fallback') : `${t('ops.priorityShort')}${rule.priority}`}</span>
                          <strong>{rule.name || rule.id || '-'}</strong>
                          <small>{rule.is_fallback ? t('rule.fallbackHelp') : summarizeRuleMatch(rule)}</small>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              )) : (
                <div className="flow-empty">
                  <Icon name="alt_route" size={22} />
                  <span>{t('ops.noRoutes')}</span>
                  <strong>{config.proxy_listen || '0.0.0.0:80'} → {config.fallback?.url}</strong>
                </div>
              )}
            </div>
          </div>
          <div className="traffic-card">
            <div className="traffic-header">
              <div className="traffic-title-wrap"><h2 className="traffic-title">{t('ops.traffic')}</h2><p className="traffic-sub">{t('ops.trafficDesc')}</p></div>
              <div className="traffic-search"><Icon name="search" size={16} /><input placeholder={t('ops.filterRules')} /></div>
            </div>
            <div className="traffic-table-wrap">
              <table><thead><tr><th>{t('table.rule')}</th><th>{t('table.upstream')}</th><th>{t('table.status')}</th><th>{t('table.requests')}</th></tr></thead>
              <tbody>
                {trafficRows.length > 0 ? trafficRows.slice(0, 20).map((row, i) => (
                  <tr key={i}><td className="td-mono" title={row.ruleId}>{row.rule}</td><td className="td-mono">{row.upstream}</td><td><span className="td-badge">{row.status}</span></td><td className="td-mono">{formatNumber(row.count)}</td></tr>
                )) : <tr><td colSpan={4} style={{ textAlign: 'center', color: 'var(--muted-foreground)', padding: 32 }}>{t('ops.noTraffic')}</td></tr>}
              </tbody></table>
            </div>
          </div>
        </div>
        <div className="ops-inspector">
          <div className="card">
            <h3 className="card-title">{t('inspector.status')}</h3>
            <div className="status-item"><span className="status-dot success" /><span className="status-text">{t('inspector.proxyOk')}</span></div>
            <div className="status-item"><span className="status-dot success" /><span className="status-text">{t('inspector.apiOk')}</span></div>
          </div>
          <div className="card">
            <h3 className="card-title">{t('inspector.snapshot')}</h3>
            <div className="config-row"><span className="config-key">request_timeout</span><span className="config-val">{config.request_timeout ?? 60}s</span></div>
            <div className="config-row"><span className="config-key">pool_idle_timeout</span><span className="config-val">{config.pool_idle_timeout ?? 90}s</span></div>
          </div>
          <div className="card" style={{ flex: 1 }}>
            <div className="resource-card-head">
              <div>
                <h3 className="card-title">{t('inspector.prometheus')}</h3>
                <p className="card-desc">{t('inspector.prometheusDesc')}</p>
              </div>
              <span className="resource-live-dot">{t('metric.live')}</span>
            </div>
            <div className="resource-grid">
              <ResourceMetric
                label={t('inspector.memory')}
                value={formatBytes(residentMemory)}
                hint={t('inspector.rssHint')}
                icon="memory"
                tone="success"
                progress={clampPercent((residentMemory / (512 * 1024 * 1024)) * 100)}
              />
              <ResourceMetric
                label={t('inspector.cpu')}
                value={`${cpuUsage.toFixed(cpuUsage >= 10 ? 0 : 1)}%`}
                hint={t('inspector.cpuHint')}
                icon="speed"
                tone="warning"
                progress={clampPercent(cpuUsage)}
              />
              <ResourceMetric
                label={t('inspector.connections')}
                value={formatNumber(activeConns)}
                hint="proxy_active_connections"
                icon="hub"
                tone="info"
                progress={clampPercent((activeConns / 100) * 100)}
              />
              <ResourceMetric
                label={t('inspector.fds')}
                value={formatNumber(openFds)}
                hint={t('inspector.fdHint')}
                icon="folder_open"
                tone="neutral"
                progress={clampPercent((openFds / 1024) * 100)}
              />
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function ResourceMetric({ label, value, hint, icon, tone, progress }: { label: string; value: string; hint: string; icon: string; tone: 'success' | 'warning' | 'info' | 'neutral'; progress: number }) {
  return (
    <div className={`resource-metric tone-${tone}`}>
      <div className="resource-metric-top">
        <span className="resource-icon"><Icon name={icon} size={18} /></span>
        <span className="resource-label">{label}</span>
      </div>
      <strong>{value}</strong>
      <div className="resource-bar-track"><span className="resource-bar-fill" style={{ width: `${progress}%` }} /></div>
      <small>{hint}</small>
    </div>
  );
}

/* ===== Rules View ===== */

function RulesView({ config, token, setConfig, setNotice }: DataProps) {
  const { t } = useI18n();
  const defaultListen = config.proxy_listen || '0.0.0.0:80';
  const [draft, setDraft] = useState<Rule>(() => newRule(Object.keys(config.upstreams)[0], defaultListen));
  const [editing, setEditing] = useState<string | null>(null);
  const [showModal, setShowModal] = useState(false);
  const upstreamNames = Object.keys(config.upstreams);
  const certificates = config.certificates ?? [];
  const matchSets = config.match_sets ?? [];
  const protocolConflict = listenerProtocolConflict(config, draft, editing);

  function openCreate() { setEditing(null); setDraft(newRule(upstreamNames[0], defaultListen)); setShowModal(true); }
  function openEdit(rule: Rule) { setEditing(rule.id); setDraft({ ...rule, listen: rule.listen || defaultListen, conditions: rule.match_set ? null : normalizeCondition(rule.conditions) }); setShowModal(true); }

  async function submit(event: FormEvent) {
    event.preventDefault();
    const body = {
      ...draft,
      match_set: draft.is_fallback ? null : draft.match_set || null,
      conditions: draft.is_fallback || draft.match_set ? null : normalizeCondition(draft.conditions),
      priority: draft.is_fallback ? 0 : draft.priority,
      weight: draft.is_fallback ? 100 : draft.weight,
      listen: draft.listen || defaultListen,
      tls: draft.is_fallback ? null : draft.tls?.enabled ? { enabled: true, certificate: draft.tls.certificate } : null,
    };
    await api<Rule>(editing ? `/api/rules/${encodeURIComponent(editing)}` : '/api/rules', { method: editing ? 'PUT' : 'POST', token, body });
    await refreshConfig(token, setConfig, setNotice);
    setNotice({ type: 'success', message: editing ? t('notice.ruleUpdated') : t('notice.ruleCreated') });
    setShowModal(false);
  }

  async function remove(id: string) {
    await api(`/api/rules/${encodeURIComponent(id)}`, { method: 'DELETE', token });
    await refreshConfig(token, setConfig, setNotice);
    setNotice({ type: 'success', message: t('notice.ruleDeleted') });
  }

  async function handleReload() { await refreshConfig(token, setConfig, setNotice); setNotice({ type: 'success', message: t('notice.configReloaded') }); }

  const sorted = [...config.rules].sort((a, b) => Number(a.is_fallback) - Number(b.is_fallback) || b.priority - a.priority);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', flex: 1, gap: 16 }}>
      <ViewHeader title={t('rules.title')} subtitle={t('rules.sub')} actions={
        <><button className="btn btn-secondary btn-rect" onClick={handleReload}><Icon name="refresh" size={18} />{t('action.reload')}</button>
        <button className="btn btn-primary btn-rect" onClick={openCreate}><Icon name="add" size={18} />{t('action.newRule')}</button></>
      } />
      <div className="table-card"><div className="table-wrap">
        <table><thead><tr>
          <th style={{ width: 70 }}>{t('table.id')}</th><th style={{ width: 130 }}>{t('table.name')}</th>
          <th style={{ width: 80 }}>{t('table.priority')}</th><th style={{ width: 90 }}>{t('table.listen')}</th>
          <th style={{ width: 130 }}>{t('table.pool')}</th><th style={{ width: 70 }}>{t('table.weight')}</th>
          <th>{t('table.match')}</th><th style={{ width: 80 }}>{t('table.actions')}</th>
        </tr></thead><tbody>
          {sorted.length === 0 ? (
            <tr><td colSpan={8} style={{ textAlign: 'center', color: 'var(--muted-foreground)', padding: 40 }}>{t('table.noRules')}</td></tr>
          ) : sorted.map((rule) => (
            <tr key={rule.id}>
              <td className="td-mono">{rule.id}</td>
              <td style={{ fontWeight: 500 }}>{rule.name || rule.id}</td>
              <td className="td-mono">{rule.priority}</td>
              <td className="td-mono">{rule.is_fallback ? `${t('rule.fallback')} · ${rule.listen || defaultListen}` : `${rule.tls?.enabled ? 'HTTPS ' : ''}${rule.listen || defaultListen}`}</td>
              <td><span className="td-badge">{rule.upstream}</span></td>
              <td className="td-mono">{rule.weight}</td>
              <td className="td-mono">{rule.is_fallback ? t('rule.enableFallback') : summarizeRuleMatch(rule)}</td>
              <td className="td-actions">
                <button className="btn btn-ghost btn-sm" onClick={() => openEdit(rule)}>{t('action.edit')}</button>
                <button className="btn btn-danger btn-sm" onClick={() => remove(rule.id)}>{t('action.del')}</button>
              </td>
            </tr>
          ))}
        </tbody></table>
      </div></div>
      {showModal && (
        <Modal title={editing ? t('modal.editRule') : t('modal.newRule')} onClose={() => setShowModal(false)}>
          <form onSubmit={submit} className="config-form">
            <section className="form-section">
              <h3 className="form-section-title">{t('form.identity')}</h3>
              <div className="form-grid">
                <Field label={t('table.name')}><input value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} required /></Field>
                {editing && <Field label={t('table.id')}><input value={draft.id} disabled /></Field>}
              </div>
              <label className="toggle-row">
                <input
                  type="checkbox"
                  checked={Boolean(draft.is_fallback)}
                  onChange={(e) => setDraft({
                    ...draft,
                    is_fallback: e.target.checked,
                    match_set: e.target.checked ? null : draft.match_set,
                    conditions: e.target.checked ? null : draft.conditions ?? createLeafCondition(),
                    listen: draft.listen || defaultListen,
                    tls: e.target.checked ? null : draft.tls,
                  })}
                />
                <span>{t('rule.enableFallback')}</span>
              </label>
              {draft.is_fallback && <p className="card-desc">{t('rule.fallbackHelp')}</p>}
            </section>
            {!draft.is_fallback && <section className="form-section">
              <h3 className="form-section-title">{t('form.routing')}</h3>
              <div className="form-grid-3">
                <Field label={t('table.priority')}><input type="number" value={draft.priority} onChange={(e) => setDraft({ ...draft, priority: Number(e.target.value) })} /></Field>
                <Field label={t('table.listen')}><input placeholder={config.proxy_listen || '0.0.0.0:80'} value={draft.listen ?? ''} onChange={(e) => setDraft({ ...draft, listen: e.target.value })} /></Field>
                <Field label={t('table.weight')}><input type="number" min="0" max="100" value={draft.weight} onChange={(e) => setDraft({ ...draft, weight: Number(e.target.value) })} /></Field>
              </div>
              <Field label={t('table.pool')}>
                <Dropdown
                  value={draft.upstream}
                  options={upstreamNames.length > 0 ? upstreamNames.map((name) => ({ value: name, label: name })) : [{ value: '', label: '—' }]}
                  onChange={(upstream) => setDraft({ ...draft, upstream })}
                />
              </Field>
            </section>}
            <section className="form-section">
              {draft.is_fallback && <h3 className="form-section-title">{t('form.routing')}</h3>}
              {draft.is_fallback && (
                <Field label={t('table.pool')}>
                  <Dropdown
                    value={draft.upstream}
                    options={upstreamNames.length > 0 ? upstreamNames.map((name) => ({ value: name, label: name })) : [{ value: '', label: '—' }]}
                    onChange={(upstream) => setDraft({ ...draft, upstream })}
                  />
                </Field>
              )}
              {!draft.is_fallback && (
              <div className="tls-rule-card">
                <div className="tls-rule-head">
                  <div>
                    <h3 className="form-section-title">{t('rule.tls')}</h3>
                    <p className="card-desc">{t('rule.tlsHelp')} {t('config.tlsRestart')}</p>
                  </div>
                  <label className="switch-control">
                    <input
                      type="checkbox"
                      checked={Boolean(draft.tls?.enabled)}
                      onChange={(e) => setDraft({
                        ...draft,
                        tls: e.target.checked
                          ? { enabled: true, certificate: draft.tls?.certificate || certificates[0]?.name || '' }
                          : null,
                      })}
                    />
                    <span className="switch-track"><span className="switch-thumb" /></span>
                    <span>{t('rule.enableTls')}</span>
                  </label>
                </div>
                {draft.tls?.enabled && (
                  <div className="form-grid">
                    <Field label={t('config.certName')}>
                      <Dropdown
                        value={draft.tls.certificate}
                        options={certificates.length > 0 ? certificates.map((cert) => ({ value: cert.name, label: cert.name })) : [{ value: '', label: t('rule.noCertificates') }]}
                        onChange={(certificate) => setDraft({ ...draft, tls: { enabled: true, certificate } })}
                      />
                    </Field>
                    <Field label={t('table.listen')}>
                      <input placeholder="0.0.0.0:443" value={draft.listen ?? ''} onChange={(e) => setDraft({ ...draft, listen: e.target.value })} required />
                    </Field>
                  </div>
                )}
                {protocolConflict && <div className="form-warning"><Icon name="warning" size={16} />{t('rule.protocolConflict')}</div>}
              </div>
              )}
            </section>
            {!draft.is_fallback && <section className="form-section">
              <h3 className="form-section-title">{t('form.matching')}</h3>
              <Field label={t('form.matchSource')}>
                <Dropdown
                  value={draft.match_set || '__inline__'}
                  options={[
                    { value: '__inline__', label: t('form.inlineMatch'), description: t('form.matching') },
                    ...matchSets.map((set) => ({ value: set.name, label: set.name, description: summarizeCondition(set.conditions) })),
                  ]}
                  onChange={(value) => setDraft(value === '__inline__'
                    ? { ...draft, match_set: null, conditions: draft.conditions ?? createLeafCondition() }
                    : { ...draft, match_set: value, conditions: null })}
                />
              </Field>
              {!draft.match_set && <ConditionEditor draft={draft} setDraft={setDraft} />}
            </section>}
            <div className="modal-footer">
              <button type="button" className="btn btn-secondary" onClick={() => setShowModal(false)}>{t('action.cancel')}</button>
              <button type="submit" className="btn btn-primary" disabled={protocolConflict}>{editing ? t('action.save') : t('action.create')}</button>
            </div>
          </form>
        </Modal>
      )}
    </div>
  );
}

/* ===== Match Sets View ===== */

function MatchSetsView({ config, token, setConfig, setNotice }: DataProps) {
  const { t } = useI18n();
  const [draft, setDraft] = useState<MatchSet>(() => newMatchSet());
  const [editing, setEditing] = useState<string | null>(null);
  const [showModal, setShowModal] = useState(false);
  const matchSets = [...(config.match_sets ?? [])].sort((a, b) => a.name.localeCompare(b.name));
  const usedBy: Record<string, number> = {};
  (config.rules ?? []).forEach((rule) => {
    if (rule.match_set) usedBy[rule.match_set] = (usedBy[rule.match_set] || 0) + 1;
  });

  function openCreate() { setEditing(null); setDraft(newMatchSet()); setShowModal(true); }
  function openEdit(matchSet: MatchSet) { setEditing(matchSet.name); setDraft({ name: matchSet.name, conditions: normalizeCondition(matchSet.conditions) ?? createLeafCondition() }); setShowModal(true); }

  async function submit(event: FormEvent) {
    event.preventDefault();
    const body = { ...draft, conditions: normalizeCondition(draft.conditions) };
    await api<MatchSet>(editing ? `/api/match-sets/${encodeURIComponent(editing)}` : '/api/match-sets', { method: editing ? 'PUT' : 'POST', token, body });
    await refreshConfig(token, setConfig, setNotice);
    setNotice({ type: 'success', message: editing ? t('notice.matchSetUpdated') : t('notice.matchSetCreated') });
    setShowModal(false);
  }

  async function remove(name: string) {
    await api(`/api/match-sets/${encodeURIComponent(name)}`, { method: 'DELETE', token });
    await refreshConfig(token, setConfig, setNotice);
    setNotice({ type: 'success', message: t('notice.matchSetDeleted') });
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', flex: 1, gap: 16 }}>
      <ViewHeader title={t('matchSets.title')} subtitle={t('matchSets.sub')} actions={
        <button className="btn btn-primary btn-rect" onClick={openCreate}><Icon name="add" size={18} />{t('action.newMatchSet')}</button>
      } />
      <div className="table-card"><div className="table-wrap">
        <table><thead><tr>
          <th style={{ width: 180 }}>{t('table.name')}</th>
          <th>{t('table.match')}</th>
          <th style={{ width: 100 }}>{t('table.rule')}</th>
          <th style={{ width: 80 }}>{t('table.actions')}</th>
        </tr></thead><tbody>
          {matchSets.length === 0 ? (
            <tr><td colSpan={4} style={{ textAlign: 'center', color: 'var(--muted-foreground)', padding: 40 }}>{t('matchSets.empty')}</td></tr>
          ) : matchSets.map((matchSet) => (
            <tr key={matchSet.name}>
              <td style={{ fontWeight: 600 }}>{matchSet.name}</td>
              <td className="td-mono">{summarizeCondition(matchSet.conditions)}</td>
              <td className="td-mono">{usedBy[matchSet.name] || 0}</td>
              <td className="td-actions">
                <button className="btn btn-ghost btn-sm" onClick={() => openEdit(matchSet)}>{t('action.edit')}</button>
                <button className="btn btn-danger btn-sm" onClick={() => remove(matchSet.name)}>{t('action.del')}</button>
              </td>
            </tr>
          ))}
        </tbody></table>
      </div></div>
      {showModal && (
        <Modal title={editing ? t('modal.editMatchSet') : t('modal.newMatchSet')} onClose={() => setShowModal(false)}>
          <form onSubmit={submit} className="config-form">
            <section className="form-section">
              <h3 className="form-section-title">{t('form.identity')}</h3>
              <Field label={t('table.name')}><input value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} disabled={Boolean(editing)} required /></Field>
            </section>
            <section className="form-section">
              <h3 className="form-section-title">{t('form.matching')}</h3>
              <ConditionTreeEditor
                condition={draft.conditions ?? createLeafCondition()}
                onChange={(conditions) => setDraft({ ...draft, conditions })}
              />
            </section>
            <div className="modal-footer">
              <button type="button" className="btn btn-secondary" onClick={() => setShowModal(false)}>{t('action.cancel')}</button>
              <button type="submit" className="btn btn-primary">{editing ? t('action.save') : t('action.create')}</button>
            </div>
          </form>
        </Modal>
      )}
    </div>
  );
}

/* ===== Condition Editor ===== */

function ConditionEditor({ draft, setDraft }: { draft: Rule; setDraft: (r: Rule) => void }) {
  const condition = draft.conditions ?? createLeafCondition();
  return <ConditionTreeEditor condition={condition} onChange={(next) => setDraft({ ...draft, conditions: next })} />;
}

function ConditionTreeEditor({ condition, onChange }: { condition: ConditionExpr; onChange: (expr: ConditionExpr) => void }) {
  return (
    <div className="condition-tree">
      <ConditionNodeEditor
        expr={condition}
        depth={0}
        onChange={onChange}
      />
    </div>
  );
}

function ConditionNodeEditor({ expr, depth, onChange, onRemove }: {
  expr: ConditionExpr;
  depth: number;
  onChange: (expr: ConditionExpr) => void;
  onRemove?: () => void;
}) {
  const { t } = useI18n();
  const isGroup = expr.type === 'and' || expr.type === 'or';
  const nodeType = expr.type;

  function changeNodeType(type: ConditionExpr['type']) {
    if (type === expr.type) return;
    if (type === 'leaf') {
      onChange(createLeafCondition());
      return;
    }
    onChange({
      type,
      children: expr.type === 'leaf' ? [normalizeConditionExpr(expr)] : expr.children,
    });
  }

  function updateChild(index: number, child: ConditionExpr) {
    if (!isGroup) return;
    onChange({ ...expr, children: expr.children.map((item, i) => (i === index ? child : item)) });
  }

  function removeChild(index: number) {
    if (!isGroup) return;
    const children = expr.children.filter((_, i) => i !== index);
    onChange({ ...expr, children: children.length > 0 ? children : [createLeafCondition()] });
  }

  return (
    <div className={isGroup ? 'condition-node is-group' : 'condition-node'}>
      <div className="condition-node-head">
        <Field label={t('form.nodeType')}>
          <Dropdown
            value={nodeType}
            options={[
              { value: 'leaf', label: t('form.leaf') },
              { value: 'and', label: t('form.and') },
              { value: 'or', label: t('form.or') },
            ]}
            onChange={(nextType) => changeNodeType(nextType as ConditionExpr['type'])}
          />
        </Field>
        {onRemove && (
          <button type="button" className="condition-remove" onClick={onRemove} aria-label="Remove condition">
            <Icon name="close" size={16} />
          </button>
        )}
      </div>

      {expr.type === 'leaf' ? (
        <ConditionLeafEditor expr={expr} onChange={onChange} />
      ) : (
        <div className="condition-children">
          <div className="condition-group-label">
            <span>{expr.type.toUpperCase()}</span>
          </div>
          {expr.children.map((child, index) => (
            <ConditionNodeEditor
              key={index}
              expr={child}
              depth={depth + 1}
              onChange={(next) => updateChild(index, next)}
              onRemove={expr.children.length > 1 ? () => removeChild(index) : undefined}
            />
          ))}
          <div className="condition-actions">
            <button type="button" className="btn btn-secondary btn-sm" onClick={() => onChange({ ...expr, children: [...expr.children, createLeafCondition()] })}>
              <Icon name="add" size={16} />{t('form.addCondition')}
            </button>
            <button type="button" className="btn btn-secondary btn-sm" onClick={() => onChange({ ...expr, children: [...expr.children, { type: 'and', children: [createLeafCondition()] }] })}>
              <Icon name="account_tree" size={16} />{t('form.addGroup')}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

function ConditionLeafEditor({ expr, onChange }: {
  expr: Extract<ConditionExpr, { type: 'leaf' }>;
  onChange: (expr: ConditionExpr) => void;
}) {
  const { t } = useI18n();
  return (
    <div className="condition-grid">
      <Field label={t('form.type')}>
        <Dropdown
          value={expr.conditionType}
          options={conditionTypeOptions(t)}
          onChange={(conditionType) => onChange(normalizeLeafForType({ ...expr, conditionType: conditionType as ConditionType }))}
        />
      </Field>
      <Field label={t('form.key')}>
        <input value={expr.key ?? expr.claimPath ?? ''} onChange={(e) => onChange(expr.conditionType === 'jwt' ? { ...expr, claimPath: e.target.value, key: null } : { ...expr, key: e.target.value, claimPath: null })} />
      </Field>
      <Field label={t('form.operator')}>
        <Dropdown
          value={expr.operator}
          options={operatorOptions()}
          onChange={(operator) => onChange({ ...expr, operator: operator as Operator })}
        />
      </Field>
      <Field label={t('form.value')}>
        <input value={expr.value ?? ''} onChange={(e) => onChange({ ...expr, value: e.target.value || null })} disabled={expr.operator === 'exists'} />
      </Field>
    </div>
  );
}

/* ===== Upstreams View ===== */

function UpstreamsView({ config, token, setConfig, setNotice }: DataProps) {
  const { t } = useI18n();
  const [draft, setDraft] = useState<Upstream>(newUpstream());
  const [editing, setEditing] = useState<string | null>(null);
  const [showModal, setShowModal] = useState(false);
  const upstreams = Object.values(config.upstreams ?? {});

  function openCreate() { setEditing(null); setDraft(newUpstream()); setShowModal(true); }
  function openEdit(u: Upstream) { setEditing(u.name); setDraft({ ...structuredClone(u), health_check: normalizeHealthCheck(u.health_check) }); setShowModal(true); }

  async function submit(event: FormEvent) {
    event.preventDefault();
    const body = { ...draft, health_check: normalizeHealthCheck(draft.health_check) };
    await api<Upstream>(editing ? `/api/upstreams/${encodeURIComponent(editing)}` : '/api/upstreams', { method: editing ? 'PUT' : 'POST', token, body });
    await refreshConfig(token, setConfig, setNotice);
    setNotice({ type: 'success', message: editing ? t('notice.upstreamUpdated') : t('notice.upstreamCreated') });
    setShowModal(false);
  }

  async function remove(name: string) {
    await api(`/api/upstreams/${encodeURIComponent(name)}`, { method: 'DELETE', token });
    await refreshConfig(token, setConfig, setNotice);
    setNotice({ type: 'success', message: t('notice.upstreamDeleted') });
  }

  async function handleReload() { await refreshConfig(token, setConfig, setNotice); setNotice({ type: 'success', message: t('notice.configReloaded') }); }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', flex: 1, gap: 16 }}>
      <ViewHeader title={t('upstreams.title')} subtitle={t('upstreams.sub')} actions={
        <><button className="btn btn-secondary" onClick={handleReload}><Icon name="refresh" size={18} />{t('action.reload')}</button>
        <button className="btn btn-primary" onClick={openCreate}><Icon name="add" size={18} />{t('action.newUpstream')}</button></>
      } />
      <div className="table-card"><div className="table-wrap">
        <table><thead><tr>
          <th>{t('table.name')}</th><th>{t('table.targets')}</th><th>{t('table.healthCheck')}</th><th style={{ width: 80 }}>{t('table.actions')}</th>
        </tr></thead><tbody>
          {upstreams.length === 0 ? (
            <tr><td colSpan={4} style={{ textAlign: 'center', color: 'var(--muted-foreground)', padding: 40 }}>{t('table.noUpstreams')}</td></tr>
          ) : upstreams.map((u) => (
            <tr key={u.name}>
              <td style={{ fontWeight: 500 }}>{u.name}</td>
              <td className="td-mono">{u.targets.map((tgt) => `${tgt.url} (${tgt.weight})`).join(', ')}</td>
              <td><HealthCheckSummary upstream={u} /></td>
              <td className="td-actions">
                <button className="btn btn-ghost btn-sm" onClick={() => openEdit(u)}>{t('action.edit')}</button>
                <button className="btn btn-danger btn-sm" onClick={() => remove(u.name)}>{t('action.del')}</button>
              </td>
            </tr>
          ))}
        </tbody></table>
      </div></div>
      {showModal && (
        <Modal title={editing ? t('modal.editUpstream') : t('modal.newUpstream')} onClose={() => setShowModal(false)}>
          <form onSubmit={submit} className="config-form">
            <section className="form-section">
              <h3 className="form-section-title">{t('form.pool')}</h3>
              <Field label={t('table.name')}>
                <input value={draft.name} disabled={Boolean(editing)} onChange={(e) => setDraft({ ...draft, name: e.target.value })} required />
              </Field>
              <label className="toggle-row">
                <input type="checkbox" checked={Boolean(draft.skip_ssl)} onChange={(e) => setDraft({ ...draft, skip_ssl: e.target.checked })} />
                <span>{t('config.skipSsl')}</span>
              </label>
              <label className="toggle-row">
                <input type="checkbox" checked={Boolean(draft.websocket)} onChange={(e) => setDraft({ ...draft, websocket: e.target.checked })} />
                <span>{t('config.websocket')}</span>
              </label>
            </section>
            <section className="form-section">
              <h3 className="form-section-title">{t('form.targets')}</h3>
              <div className="target-editor">
                {draft.targets.map((target, i) => (
                  <div className="target-row" key={i}>
                    <Field label="URL"><input value={target.url} onChange={(e) => setDraft(replaceTarget(draft, i, { ...target, url: e.target.value }))} required /></Field>
                    <Field label={t('table.weight')}><input type="number" min="0" value={target.weight} onChange={(e) => setDraft(replaceTarget(draft, i, { ...target, weight: Number(e.target.value) }))} /></Field>
                    <button type="button" className="btn btn-ghost btn-sm" style={{ marginTop: 24 }} onClick={() => setDraft({ ...draft, targets: draft.targets.filter((_, j) => j !== i) })}><Icon name="close" size={16} /></button>
                  </div>
                ))}
              </div>
              <button type="button" className="btn btn-secondary" style={{ marginTop: 10 }} onClick={() => setDraft({ ...draft, targets: [...draft.targets, { url: 'http://127.0.0.1:8080', weight: 100 }] })}><Icon name="add" size={16} />{t('action.addTarget')}</button>
            </section>
            <HealthCheckEditor draft={draft} setDraft={setDraft} />
            <div className="modal-footer">
              <button type="button" className="btn btn-secondary" onClick={() => setShowModal(false)}>{t('action.cancel')}</button>
              <button type="submit" className="btn btn-primary">{editing ? t('action.save') : t('action.create')}</button>
            </div>
          </form>
        </Modal>
      )}
    </div>
  );
}

/* ===== Health Check Components ===== */

function HealthCheckSummary({ upstream }: { upstream: Upstream }) {
  const { t } = useI18n();
  const check = normalizeHealthCheck(upstream.health_check);
  if (!check.enabled) return <span style={{ color: 'var(--muted-foreground)' }}>{t('table.off')}</span>;
  return (
    <div className="health-summary">
      <span className="td-badge">{check.mode.toUpperCase()}</span>
      <span className="health-summary-text">{check.mode === 'http' ? `${check.path} -> ${check.expected_status}` : t('health.hostPort')}</span>
      <span className="health-summary-muted">{check.interval_seconds}s / {check.timeout_seconds}s</span>
    </div>
  );
}

function HealthCheckEditor({ draft, setDraft }: { draft: Upstream; setDraft: (upstream: Upstream) => void }) {
  const { t } = useI18n();
  const check = normalizeHealthCheck(draft.health_check);
  function update(patch: Partial<HealthCheck>) { setDraft({ ...draft, health_check: { ...check, ...patch } }); }

  return (
    <section className="health-panel">
      <div className="health-panel-head">
        <div>
          <div className="field-label">{t('health.title')}</div>
          <p className="health-panel-copy">{t('health.desc')}</p>
        </div>
        <label className="switch-control">
          <input type="checkbox" checked={check.enabled} onChange={(e) => update({ enabled: e.target.checked })} />
          <span className="switch-track"><span className="switch-thumb" /></span>
          <span>{check.enabled ? t('health.enabled') : t('health.disabled')}</span>
        </label>
      </div>
      <div className={check.enabled ? 'health-options is-enabled' : 'health-options'}>
        <div className="health-mode-control" role="radiogroup" aria-label="Health check mode">
          <button type="button" className={check.mode === 'tcp' ? 'health-mode is-active' : 'health-mode'} onClick={() => update({ mode: 'tcp' })}>
            <Icon name="settings_ethernet" size={18} /><span><strong>{t('health.tcp')}</strong><small>{t('health.tcpDesc')}</small></span>
          </button>
          <button type="button" className={check.mode === 'http' ? 'health-mode is-active' : 'health-mode'} onClick={() => update({ mode: 'http' })}>
            <Icon name="http" size={18} /><span><strong>{t('health.http')}</strong><small>{t('health.httpDesc')}</small></span>
          </button>
        </div>
        <div className="health-config-grid">
          <Field label={t('health.path')}><input value={check.path} disabled={check.mode !== 'http'} placeholder="/health" onChange={(e) => update({ path: e.target.value || '/health' })} /></Field>
          <Field label={t('health.expectedStatus')}><input type="number" min="100" max="599" disabled={check.mode !== 'http'} value={check.expected_status} onChange={(e) => update({ expected_status: clampInt(e.target.value, 100, 599, 200) })} /></Field>
          <Field label={t('health.interval')}><input type="number" min="1" value={check.interval_seconds} onChange={(e) => update({ interval_seconds: clampInt(e.target.value, 1, 3600, 10) })} /></Field>
          <Field label={t('health.timeout')}><input type="number" min="1" value={check.timeout_seconds} onChange={(e) => update({ timeout_seconds: clampInt(e.target.value, 1, 300, 2) })} /></Field>
          <Field label={t('health.healthyThreshold')}><input type="number" min="1" value={check.healthy_threshold} onChange={(e) => update({ healthy_threshold: clampInt(e.target.value, 1, 100, 2) })} /></Field>
          <Field label={t('health.unhealthyThreshold')}><input type="number" min="1" value={check.unhealthy_threshold} onChange={(e) => update({ unhealthy_threshold: clampInt(e.target.value, 1, 100, 2) })} /></Field>
        </div>
        <div className="health-preview">
          <Icon name="rule_settings" size={17} />
          {check.mode === 'http' ? `GET target host + ${check.path || '/health'} must return ${check.expected_status}.` : t('health.tcpPreview')}
        </div>
      </div>
    </section>
  );
}

/* ===== Config File View ===== */

function CertificatesView({ config, token, setConfig, setNotice }: DataProps) {
  const { t } = useI18n();
  const [certificates, setCertificates] = useState<Certificate[]>(() => structuredClone(config.certificates ?? []));
  useEffect(() => setCertificates(structuredClone(config.certificates ?? [])), [config.certificates]);

  async function importCertificateFile(index: number, field: 'cert' | 'key', file: File | null) {
    if (!file) return;
    const content = await readCertificateFile(file);
    setCertificates((items) => items.map((item, i) => i === index ? { ...item, [field]: content } : item));
  }

  async function submit() {
    try {
      for (const certificate of certificates) {
        if (!certificate.name.trim()) continue;
        if (isUploadedCertificate(certificate)) continue;
        await api<Certificate>('/api/certificates', { method: 'POST', token, body: certificate });
      }
      const uploadedNames = new Set(certificates.map((certificate) => certificate.name).filter(Boolean));
      const latest = await api<AppConfig>('/api/config', { token });
      const next = { ...latest, certificates: (latest.certificates ?? []).filter((certificate) => uploadedNames.has(certificate.name)) };
      await api<AppConfig>('/api/config', { method: 'PUT', token, body: next });
      setConfig(next);
      setCertificates(structuredClone(next.certificates ?? []));
      setNotice({ type: 'success', message: t('cert.saved') });
    } catch (e) { setNotice({ type: 'error', message: errorMessage(e) }); }
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', flex: 1, gap: 16 }}>
      <ViewHeader title={t('cert.title')} subtitle={t('cert.sub')} actions={
        <button className="btn btn-primary" onClick={submit}><Icon name="save" size={18} />{t('action.save')}</button>
      } />
      <div className="cert-page">
        <div className="section-title-row">
          <h3 className="card-title-sm">{t('config.certificates')}</h3>
          <button className="btn btn-secondary btn-sm" onClick={() => setCertificates([...certificates, { name: '', cert: '', key: '' }])}>
            <Icon name="upload_file" size={16} />{t('config.addCertificate')}
          </button>
        </div>
        {certificates.length === 0 ? (
          <div className="empty-state"><Icon name="workspace_premium" size={28} /><span>{t('cert.empty')}</span></div>
        ) : (
          <div className="certificate-grid">
            {certificates.map((certificate, index) => (
              <div className="certificate-editor" key={`${certificate.name}-${index}`}>
                <Field label={t('config.certName')}><input value={certificate.name} onChange={(e) => setCertificates(certificates.map((item, i) => i === index ? { ...item, name: e.target.value } : item))} /></Field>
                <div className="upload-grid">
                  <label className="upload-box">
                    <Icon name="verified_user" size={18} />
                    <span>{certificate.cert ? certificate.cert.split('\n')[0].slice(0, 54) : t('config.certFile')}</span>
                    <small>{t('config.certFormat')}</small>
                    <input type="file" accept=".pem,.crt,.cer,.der" onChange={(e) => importCertificateFile(index, 'cert', e.target.files?.[0] ?? null)} />
                  </label>
                  <label className="upload-box">
                    <Icon name="key" size={18} />
                    <span>{certificate.key ? certificate.key.split('\n')[0].slice(0, 54) : t('config.keyFile')}</span>
                    <small>{t('config.keyFormat')}</small>
                    <input type="file" accept=".pem,.key,.der" onChange={(e) => importCertificateFile(index, 'key', e.target.files?.[0] ?? null)} />
                  </label>
                </div>
                <button className="btn btn-ghost btn-sm" onClick={() => setCertificates(certificates.filter((_, i) => i !== index))}>{t('action.del')}</button>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function ConfigView({ config, token, setConfig, setNotice }: DataProps) {
  const { t } = useI18n();
  const [text, setText] = useState(() => yaml.dump(config, { lineWidth: 110 }));
  useEffect(() => setText(yaml.dump(config, { lineWidth: 110 })), [config]);
  const lineCount = text.split('\n').length;
  const parsedConfig = useMemo(() => {
    try { return yaml.load(text) as AppConfig; } catch { return null; }
  }, [text]);
  const globalConfig = parsedConfig ?? config;

  async function submit() {
    try {
      const parsed = yaml.load(text) as AppConfig;
      await api<AppConfig>('/api/config', { method: 'PUT', token, body: parsed });
      setConfig(parsed);
      setNotice({ type: 'success', message: t('notice.configSaved') });
    } catch (e) { setNotice({ type: 'error', message: errorMessage(e) }); }
  }

  const isValid = useMemo(() => { try { yaml.load(text); return true; } catch { return false; } }, [text]);
  function updateGlobal(patch: Partial<AppConfig>) {
    const next = { ...(parsedConfig ?? config), ...patch };
    setText(yaml.dump(next, { lineWidth: 110 }));
  }

  function updateFallbackUrl(url: string) {
    const current = parsedConfig ?? config;
    setText(yaml.dump({ ...current, fallback: { ...(current.fallback ?? { url: '' }), url } }, { lineWidth: 110 }));
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', flex: 1, gap: 16 }}>
      <ViewHeader title={t('config.title')} subtitle={t('config.sub')} actions={
        <button className="btn btn-primary" onClick={submit}><Icon name="save" size={18} />{t('action.save')}</button>
      } />
      <div className="editor-layout">
        <div className="editor-col">
          <div className="editor-toolbar"><span className="editor-filename">config.yaml</span><span className="editor-lang">YAML</span></div>
          <div className="editor-content">
            <div className="line-nums">{Array.from({ length: lineCount }, (_, i) => <div className="line-num" key={i}>{i + 1}</div>)}</div>
            <div className="code-area"><textarea spellCheck={false} value={text} onChange={(e) => setText(e.target.value)} /></div>
          </div>
        </div>
        <div className="schema-col">
          <div className="card global-config-card">
            <h3 className="card-title-sm">{t('config.global')}</h3>
            <Field label={t('config.listen')}><input value={globalConfig.listen ?? '127.0.0.1:3000'} onChange={(e) => updateGlobal({ listen: e.target.value })} /></Field>
            <Field label={t('config.proxyListen')}><input value={globalConfig.proxy_listen ?? '0.0.0.0:80'} onChange={(e) => updateGlobal({ proxy_listen: e.target.value })} /></Field>
            <Field label={t('config.certificateDir')}><input value={globalConfig.certificate_dir ?? '/etc/rustproxy/cert.d'} onChange={(e) => updateGlobal({ certificate_dir: e.target.value })} /></Field>
            <Field label={t('config.fallbackUrl')}><input value={globalConfig.fallback?.url ?? ''} onChange={(e) => updateFallbackUrl(e.target.value)} /></Field>
            <Field label={t('config.requestTimeout')}><input type="number" min="0" value={globalConfig.request_timeout ?? 60} onChange={(e) => updateGlobal({ request_timeout: Number(e.target.value) })} /></Field>
          </div>
          <div className="card global-config-card">
            <h3 className="card-title-sm">{t('config.loadAdvanced')}</h3>
            <Field label={t('config.connectTimeout')}><input type="number" min="0" value={globalConfig.connect_timeout ?? 10} onChange={(e) => updateGlobal({ connect_timeout: Number(e.target.value) })} /></Field>
            <Field label={t('config.poolMaxIdle')}><input type="number" min="0" value={globalConfig.pool_max_idle_per_host ?? 32} onChange={(e) => updateGlobal({ pool_max_idle_per_host: Number(e.target.value) })} /></Field>
            <Field label={t('config.poolIdleTimeout')}><input type="number" min="0" value={globalConfig.pool_idle_timeout ?? 90} onChange={(e) => updateGlobal({ pool_idle_timeout: Number(e.target.value) })} /></Field>
            <Field label={t('config.tcpKeepalive')}><input type="number" min="0" value={globalConfig.tcp_keepalive ?? 60} onChange={(e) => updateGlobal({ tcp_keepalive: Number(e.target.value) })} /></Field>
          </div>
          <div className="card">
            <h3 className="card-title-sm">{t('config.schema')}</h3>
            <div className="schema-entry"><span className="schema-key" style={{ color: '#C792EA' }}>global</span><span className="schema-val">listen, proxy_listen, certificate_dir, certificates[].cert/key path, fallback, connect_timeout, request_timeout, pool_max_idle_per_host, pool_idle_timeout, tcp_keepalive</span></div>
            <div style={{ height: 1, background: 'var(--border)' }} />
            <div className="schema-entry"><span className="schema-key" style={{ color: '#82AAFF' }}>upstreams.&lt;name&gt;</span><span className="schema-val">skip_ssl, websocket, targets[].url, targets[].weight, health_check</span></div>
            <div style={{ height: 1, background: 'var(--border)' }} />
            <div className="schema-entry"><span className="schema-key" style={{ color: '#C792EA' }}>match_sets[]</span><span className="schema-val">name, conditions</span></div>
            <div className="schema-entry"><span className="schema-key" style={{ color: '#FFCB6B' }}>routes[]</span><span className="schema-val">id, name, priority, listen, tls.enabled, tls.certificate, match_set, conditions, upstream</span></div>
          </div>
          <div className="card">
            <h3 className="card-title-sm">{t('config.validation')}</h3>
            <div className="validation-row"><span className="validation-dot" style={{ background: isValid ? '#4ADE80' : '#FF5C33' }} /><span className="validation-text">{isValid ? t('config.valid') : t('config.invalid')}</span></div>
            <div className="validation-row"><span className="validation-dot" style={{ background: '#4ADE80' }} /><span className="validation-text">{Object.keys(config.upstreams).length} {t('config.poolsDefined')}</span></div>
            <div className="validation-row"><span className="validation-dot" style={{ background: '#C792EA' }} /><span className="validation-text">{config.match_sets?.length ?? 0} {t('matchSets.title')}</span></div>
            <div className="validation-row"><span className="validation-dot" style={{ background: 'var(--primary)' }} /><span className="validation-text">{config.rules.length} {t('config.routesConfigured')}</span></div>
          </div>
          <div className="card">
            <h3 className="card-title-sm">{t('config.store')}</h3>
            <div className="db-row"><span className="db-key">Path</span><span className="db-val">./data/config.db</span></div>
            <div className="db-row"><span className="db-key">{t('config.lastWrite')}</span><span className="db-val">—</span></div>
            <div className="db-row"><span className="db-key">{t('config.size')}</span><span className="db-val">—</span></div>
          </div>
        </div>
      </div>
    </div>
  );
}

/* ===== Setup Screen ===== */

function SetupScreen({ onDone, notice, setNotice }: { onDone: () => void; notice: Notice; setNotice: (notice: Notice) => void }) {
  const { t } = useI18n();
  const [username, setUsername] = useState('admin');
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (password !== confirmPassword) { setNotice({ type: 'error', message: t('notice.passwordMismatch') }); return; }
    try {
      await api('/api/auth/setup', { method: 'POST', body: { username, password } });
      setNotice({ type: 'success', message: t('notice.adminCreated') });
      onDone();
    } catch (error) { setNotice({ type: 'error', message: errorMessage(error) }); }
  }

  return (
    <main className="auth-screen"><div className="auth-card">
      <div className="auth-header">
        <div className="auth-logo"><img src="/admin/favicon.svg" width="32" height="32" alt="" /><span className="auth-logo-text">RustProxy</span></div>
        <h1 className="auth-title">{t('auth.createAccount')}</h1>
        <p className="auth-subtitle">{t('auth.createDesc')}</p>
      </div>
      {notice && <Toast notice={notice} onClose={() => setNotice(null)} />}
      <form className="auth-form" onSubmit={submit}>
        <Field label={t('auth.username')}><input autoComplete="username" value={username} onChange={(e) => setUsername(e.target.value)} required /></Field>
        <Field label={t('auth.email')}><input type="email" placeholder="admin@example.com" value={email} onChange={(e) => setEmail(e.target.value)} /></Field>
        <Field label={t('auth.password')}><input type="password" autoComplete="new-password" value={password} onChange={(e) => setPassword(e.target.value)} required /></Field>
        <Field label={t('auth.confirmPassword')}><input type="password" autoComplete="new-password" value={confirmPassword} onChange={(e) => setConfirmPassword(e.target.value)} required /></Field>
        <button className="btn btn-primary" type="submit" style={{ width: '100%', height: 44 }}>{t('auth.createBtn')}</button>
      </form>
      <div className="auth-divider"><span className="auth-divider-line" /><span className="auth-divider-text">{t('auth.or')}</span><span className="auth-divider-line" /></div>
      <div className="auth-alt-row"><span className="auth-alt-text">{t('auth.hasAccount')}</span><button className="auth-link" onClick={onDone}>{t('auth.signIn')}</button></div>
    </div></main>
  );
}

/* ===== Login Screen ===== */

function LoginScreen({ onDone, notice, setNotice }: { onDone: (token: string) => void; notice: Notice; setNotice: (notice: Notice) => void }) {
  const { t } = useI18n();
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');

  async function submit(event: FormEvent) {
    event.preventDefault();
    try {
      const data = await api<{ token: string }>('/api/auth/login', { method: 'PUT', body: { username, password } });
      onDone(data.token);
    } catch (error) { setNotice({ type: 'error', message: errorMessage(error) }); }
  }

  return (
    <main className="auth-screen"><div className="auth-card auth-card-login">
      <div className="auth-header">
        <div className="auth-logo"><img src="/admin/favicon.svg" width="32" height="32" alt="" /><span className="auth-logo-text">RustProxy</span></div>
        <h1 className="auth-title">{t('auth.welcome')}</h1>
        <p className="auth-subtitle">{t('auth.signInDesc')}</p>
      </div>
      {notice && <Toast notice={notice} onClose={() => setNotice(null)} />}
      <form className="auth-form" onSubmit={submit}>
        <Field label={t('auth.username')}><input autoComplete="username" value={username} onChange={(e) => setUsername(e.target.value)} required /></Field>
        <Field label={t('auth.password')}><input type="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} required /></Field>
        <button className="btn btn-primary" type="submit" style={{ width: '100%', height: 44 }}>{t('auth.signInBtn')}</button>
      </form>
    </div></main>
  );
}

/* ===== API & Helpers ===== */

async function api<T>(path: string, options: { method?: string; token?: string; body?: unknown } = {}): Promise<T> {
  const response = await fetch(path, {
    method: options.method ?? 'GET',
    headers: { 'Content-Type': 'application/json', ...(options.token ? { Authorization: `Bearer ${options.token}` } : {}) },
    body: options.body ? JSON.stringify(options.body) : undefined,
  });
  const contentType = response.headers.get('content-type') ?? '';
  const payload = contentType.includes('application/json') ? await response.json() as ApiResponse<T> : null;
  if (!response.ok || payload?.success === false) throw new Error(payload?.error ?? `HTTP ${response.status}`);
  return payload?.data as T;
}

async function refreshConfig(token: string, setConfig: (c: AppConfig) => void, setNotice: (n: Notice) => void) {
  try { setConfig(await api<AppConfig>('/api/config', { token })); } catch (error) { setNotice({ type: 'error', message: errorMessage(error) }); }
}

function parsePrometheus(text: string): PrometheusMetric[] {
  const metrics: PrometheusMetric[] = [];
  for (const line of text.split('\n')) {
    if (line.startsWith('#') || !line.trim()) continue;
    const match = line.match(/^(\w+)(?:\{([^}]*)\})?\s+([\d.eE+-]+)$/);
    if (!match) continue;
    const labels: Record<string, string> = {};
    if (match[2]) { for (const pair of match[2].split(',')) { const [k, v] = pair.split('='); if (k && v) labels[k.trim()] = v.replace(/^"|"$/g, ''); } }
    metrics.push({ name: match[1], labels, value: parseFloat(match[3]) });
  }
  return metrics;
}

function metricValue(metrics: PrometheusMetric[], name: string): number | null {
  return metrics.find((metric) => metric.name === name)?.value ?? null;
}

function formatNumber(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(2) + 'M';
  if (n >= 1_000) return n.toLocaleString();
  return String(n);
}

function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GiB`;
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${Math.round(bytes)} B`;
}

function clampPercent(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(100, value));
}

function newRule(upstream = '', listen = '0.0.0.0:80'): Rule {
  return { id: '', name: '', priority: 10, match_set: null, upstream, weight: 100, is_fallback: false, listen, conditions: createLeafCondition() };
}

function newMatchSet(): MatchSet {
  return { name: '', conditions: createLeafCondition() };
}

function tlsRuleListeners(config: AppConfig): string[] {
  const listeners = new Set<string>();
  (config.rules ?? []).forEach((rule) => {
    if (rule.tls?.enabled && rule.listen) listeners.add(rule.listen);
  });
  (config.tls_listeners ?? []).forEach((listener) => {
    if (listener.enabled) listeners.add(listener.listen);
  });
  return [...listeners].sort();
}

function isUploadedCertificate(certificate: Certificate): boolean {
  return isFilePath(certificate.cert) && isFilePath(certificate.key);
}

function isFilePath(value: string): boolean {
  return value.startsWith('/') || value.startsWith('./') || value.startsWith('../');
}

function listenerProtocolConflict(config: AppConfig, draft: Rule, editingId: string | null): boolean {
  if (draft.is_fallback) return false;
  const port = extractPort(draft.listen);
  if (!port) return false;

  const httpPorts = new Set<string>();
  const httpsPorts = new Set<string>();
  const proxyPort = extractPort(config.proxy_listen);
  if (proxyPort) httpPorts.add(proxyPort);

  (config.rules ?? []).forEach((rule) => {
    if (editingId && rule.id === editingId) return;
    const rulePort = extractPort(rule.listen);
    if (!rulePort) return;
    if (rule.tls?.enabled) httpsPorts.add(rulePort);
    else httpPorts.add(rulePort);
  });
  (config.tls_listeners ?? []).forEach((listener) => {
    const listenerPort = listener.enabled ? extractPort(listener.listen) : null;
    if (listenerPort) httpsPorts.add(listenerPort);
  });

  return draft.tls?.enabled ? httpPorts.has(port) : httpsPorts.has(port);
}

function extractPort(listen?: string | null): string | null {
  const port = listen?.trim().split(':').pop();
  return port && /^\d+$/.test(port) ? port : null;
}

function newUpstream(): Upstream {
  return { name: '', skip_ssl: false, websocket: false, targets: [{ url: 'http://127.0.0.1:8080', weight: 100 }], health_check: { ...defaultHealthCheck } };
}

function targetSummary(upstream: Upstream, lang: Lang): string {
  const targetCount = upstream.targets.length;
  const totalWeight = upstream.targets.reduce((sum, target) => sum + Number(target.weight || 0), 0);
  if (lang === 'en') {
    return `${targetCount} target${targetCount !== 1 ? 's' : ''}, weight ${totalWeight}`;
  }
  return `${targetCount} 个目标，权重 ${totalWeight}`;
}

function replaceTarget(upstream: Upstream, index: number, target: Target): Upstream {
  return { ...upstream, targets: upstream.targets.map((item, i) => (i === index ? target : item)) };
}

function conditionTypeOptions(t: (key: string) => string): DropdownOption[] {
  return (['path', 'header', 'host', 'cookie', 'jwt'] as const).map((value) => ({
    value,
    label: value,
    description: t(`form.type.${value}`),
  }));
}

function operatorOptions(): DropdownOption[] {
  return (['exact', 'prefix', 'regex', 'exists', 'contains'] as const).map((value) => ({ value, label: value }));
}

function normalizeHealthCheck(check?: Partial<HealthCheck>): HealthCheck {
  return {
    ...defaultHealthCheck, ...(check ?? {}),
    enabled: Boolean(check?.enabled), mode: check?.mode === 'http' ? 'http' : 'tcp',
    path: check?.path || defaultHealthCheck.path,
    expected_status: Number(check?.expected_status ?? defaultHealthCheck.expected_status),
    interval_seconds: Number(check?.interval_seconds ?? defaultHealthCheck.interval_seconds),
    timeout_seconds: Number(check?.timeout_seconds ?? defaultHealthCheck.timeout_seconds),
    healthy_threshold: Number(check?.healthy_threshold ?? defaultHealthCheck.healthy_threshold),
    unhealthy_threshold: Number(check?.unhealthy_threshold ?? defaultHealthCheck.unhealthy_threshold),
  };
}

function clampInt(value: string, min: number, max: number, fallback: number): number {
  const parsed = Number.parseInt(value, 10);
  if (!Number.isFinite(parsed)) return fallback;
  return Math.min(max, Math.max(min, parsed));
}

function createLeafCondition(): Extract<ConditionExpr, { type: 'leaf' }> {
  return { type: 'leaf', conditionType: 'host', key: null, claimPath: null, operator: 'exact', value: '' };
}

function normalizeLeafForType(leaf: Extract<ConditionExpr, { type: 'leaf' }>): Extract<ConditionExpr, { type: 'leaf' }> {
  if (leaf.conditionType === 'jwt') return { ...leaf, key: null };
  if (leaf.conditionType === 'header' || leaf.conditionType === 'cookie') return { ...leaf, claimPath: null };
  return { ...leaf, key: null, claimPath: null };
}

function normalizeConditionExpr(condition: ConditionExpr): ConditionExpr {
  if (condition.type === 'leaf') {
    const leaf = normalizeLeafForType(condition);
    return leaf.operator === 'exists' ? { ...leaf, value: null } : leaf;
  }
  const children = condition.children.length > 0
    ? condition.children.map(normalizeConditionExpr)
    : [createLeafCondition()];
  return { type: condition.type, children };
}

function normalizeCondition(condition?: ConditionExpr | null): ConditionExpr | null {
  return condition ? normalizeConditionExpr(condition) : null;
}

async function readCertificateFile(file: File): Promise<string> {
  const lowerName = file.name.toLowerCase();
  if (lowerName.endsWith('.der')) {
    const buffer = await file.arrayBuffer();
    let binary = '';
    const bytes = new Uint8Array(buffer);
    bytes.forEach((byte) => { binary += String.fromCharCode(byte); });
    return btoa(binary);
  }
  return file.text();
}

function summarizeCondition(condition?: ConditionExpr | null): string {
  if (!condition) return '*';
  if (condition.type === 'leaf') {
    const key = condition.conditionType === 'jwt' ? condition.claimPath : condition.key;
    const value = condition.operator === 'exists' ? '?' : condition.value;
    return [condition.conditionType, key, condition.operator, value].filter(Boolean).join(' ');
  }
  return `${condition.type.toUpperCase()} (${condition.children.map(summarizeCondition).join(', ')})`;
}

function summarizeRuleMatch(rule: Rule): string {
  if (rule.match_set) return `@${rule.match_set}`;
  return summarizeCondition(rule.conditions);
}

function resolveInitialView(): View {
  const segment = window.location.pathname.replace('/admin', '').split('/').filter(Boolean)[0];
  return NAV_ITEMS.find((item) => item.id === segment)?.id ?? 'operations';
}

function errorMessage(error: unknown) { return error instanceof Error ? error.message : 'request failed'; }

export default App;
