import { type CSSProperties, FormEvent, type PointerEvent, ReactNode, createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import yaml from 'js-yaml';

/* ===== Types ===== */

type Lang = 'en' | 'zh';
type Theme = 'light' | 'dark';
type AccessLogLevel = 'debug' | 'info' | 'warn' | 'error';
type ApiResponse<T> = { success: boolean; data: T; error?: string };
type Target = { url: string; weight: number };
type HealthCheckMode = 'tcp' | 'http';
type BalanceAlgorithm = 'weighted_round_robin' | 'least_connections' | 'ip_hash' | 'consistent_hash' | 'url_hash';
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
type RetryPolicy = {
  attempts: number;
  retry_on_status: number[];
  retry_on_timeout: boolean;
  retry_on_connect_error: boolean;
};
type Upstream = {
  name: string;
  skip_ssl?: boolean;
  websocket?: boolean;
  balance?: BalanceAlgorithm;
  retry?: RetryPolicy;
  targets: Target[];
  health_check?: Partial<HealthCheck>;
};
type Certificate = { name: string; cert: string; key: string };
type TlsListener = { enabled: boolean; listen: string; certificate: string };
type RuleTls = { enabled: boolean; certificate: string };
type HeaderMutationOp = 'set' | 'add' | 'remove';
type HeaderMutation = { op: HeaderMutationOp; name: string; value?: string | null };
type HeaderPolicy = { request: HeaderMutation[]; response: HeaderMutation[] };
type PathAction =
  | { strip_prefix: { prefix: string } }
  | { rewrite: { pattern: string; replacement: string } }
  | { redirect: { status: number; location: string } };
type RateLimitKey = 'ip' | 'host' | 'route';
type LimitPolicy = {
  rate_per_second?: number | null;
  rate_key: RateLimitKey;
  max_connections?: number | null;
  max_body_bytes?: number | null;
  queue_timeout_ms?: number | null;
};
type HostMatchType = 'any' | 'exact' | 'wildcard';
type LocationMatchType = 'exact' | 'prefix' | 'regex';
type HostMatcher = { type: HostMatchType; value?: string | null };
type LocationMatcher = { type: LocationMatchType; value: string };
type ConditionType = 'header' | 'cookie' | 'jwt';
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
  host: HostMatcher;
  location: LocationMatcher;
  match_set?: string | null;
  conditions?: ConditionExpr | null;
  upstream: string;
  weight: number;
  is_fallback?: boolean;
  listen?: string | null;
  request_timeout?: number;
  tls?: RuleTls | null;
  header_policy?: HeaderPolicy;
  path_actions?: PathAction[];
  limit_policy?: LimitPolicy;
};
type AppConfig = {
  listen: string;
  proxy_listen?: string;
  connect_timeout?: number;
  request_timeout?: number;
  pool_max_idle_per_host?: number;
  pool_idle_timeout?: number;
  tcp_keepalive?: number;
  certificate_dir?: string;
  access_log?: { enabled?: boolean; path?: string | null; buffer_size?: number | null; level?: AccessLogLevel };
  monitoring?: {
    enabled?: boolean;
    prometheus?: {
      url?: string;
      auth?: {
        auth_type?: string;
        username?: string | null;
        password?: string | null;
        bearer_token?: string | null;
        header_name?: string | null;
        header_value?: string | null;
      };
    };
  };
  certificates?: Certificate[];
  tls_listeners?: TlsListener[];
  match_sets?: MatchSet[];
  rules: Rule[];
  upstreams: Record<string, Upstream>;
  fallback: { url: string };
};

type View = 'operations' | 'monitoring' | 'rules' | 'match-sets' | 'upstreams' | 'certificates' | 'config';
type Notice = { type: 'success' | 'error'; message: string } | null;
type DataProps = { config: AppConfig; token: string; setConfig: (config: AppConfig) => void; setNotice: (notice: Notice) => void };
type DropdownOption = { value: string; label: string; description?: string };
type VersionInfo = {
  version: string;
  package_version: string;
  git_ref: string;
  git_ref_kind: string;
  git_commit: string;
  dirty: boolean;
};

type PrometheusMetric = {
  name: string;
  labels: Record<string, string>;
  value: number;
};
type UpstreamHealth = {
  upstream: string;
  enabled: boolean;
  total: number;
  unhealthy: number;
  targets: { url: string; healthy: boolean }[];
};
type RuntimeTargetMode = 'enabled' | 'disabled' | 'drain';
type RuntimeTarget = {
  url: string;
  configured_weight: number;
  effective_weight: number;
  weight_override: number | null;
  mode: RuntimeTargetMode;
  active_connections: number;
  healthy: boolean;
  last_error: string | null;
};
type RuntimeUpstream = {
  name: string;
  targets: RuntimeTarget[];
};
type PrometheusRangeResult = {
  status?: string;
  data?: {
    resultType?: string;
    result?: { metric: Record<string, string>; values: [number, string][] }[];
  };
  error?: string;
};
type ChartPoint = { t: number; v: number };

/* ===== Translations ===== */

const T: Record<string, [string, string]> = {
  'brand.subtitle': ['Reverse Proxy Admin', '反向代理管理'],
  'nav.control': ['CONTROL', '控制'],
  'nav.operations': ['Operations', '运维概览'],
  'nav.monitoring': ['Monitoring', '监控'],
  'nav.rules': ['Rules', '路由规则'],
  'nav.matchSets': ['Match Sets', '匹配集'],
  'nav.upstreams': ['Upstreams', '上游服务'],
  'nav.certificates': ['Certificates', '证书'],
  'nav.config': ['Config File', '配置文件'],
  'listeners.title': ['LISTENERS', '监听器'],
  'listeners.api': ['API + Admin UI', 'API + 管理界面'],
  'listeners.proxy': ['Reverse proxy', '反向代理'],
  'listeners.tls': ['HTTPS proxy', 'HTTPS 代理'],
  'version.label': ['BUILD', '构建版本'],
  'admin.role': ['JWT protected', 'JWT 保护'],
  'admin.logout': ['Sign out', '退出登录'],
  'theme.light': ['Light mode', '亮色模式'],
  'theme.dark': ['Dark mode', '暗色模式'],
  'ops.title': ['Operations Overview', '运维概览'],
  'ops.sub': ['Live proxy health, weighted routing, reload status, and request pressure across listeners.', '代理健康状态、加权路由、重载状态和请求压力概览。'],
  'rules.title': ['Routing Rules', '路由规则'],
  'rules.sub': ['Manage routing rules with header mutations, path actions, and per-route limits.', '管理路由规则的头部改写、路径动作和按规则限流。'],
  'matchSets.title': ['Match Sets', '匹配集'],
  'matchSets.sub': ['Create reusable request match trees and attach them to routing rules.', '创建可复用的请求匹配树，并在路由规则中引用。'],
  'matchSets.empty': ['No match sets configured', '暂无匹配集'],
  'upstreams.title': ['Upstreams', '上游服务'],
  'upstreams.sub': ['Manage upstream target pools with load balancing, retry policy, and health checks.', '管理上游目标池，支持负载均衡、重试策略和健康检查。'],
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
  'rule.requestTimeout': ['Rule request timeout (s)', '规则请求超时 (秒)'],
  'rule.requestTimeoutInherit': ['0 inherits the global request timeout', '0 表示沿用全局请求超时'],
  'config.fallbackUrl': ['Fallback target', '兜底目标'],
  'config.skipSsl': ['Skip SSL verification', '跳过 SSL 验证'],
  'config.websocket': ['Enable WebSocket proxy', '启用 WebSocket 代理'],
  'config.connectTimeout': ['Connect timeout (s)', '连接超时 (秒)'],
  'config.requestTimeout': ['Request timeout (s)', '请求超时 (秒)'],
  'config.poolMaxIdle': ['Max idle per host', '每主机最大空闲连接'],
  'config.poolIdleTimeout': ['Pool idle timeout (s)', '连接池空闲超时 (秒)'],
  'config.tcpKeepalive': ['TCP keepalive (s)', 'TCP Keepalive (秒)'],
  'config.accessLog': ['Access Log', '访问日志'],
  'config.accessLogLevel': ['Access log level', '访问日志等级'],
  'config.accessLogEnabled': ['Enable access log', '启用访问日志'],
  'config.accessLogPath': ['Log file path', '日志文件路径'],
  'config.accessLogPathHint': ['Leave empty to write to stdout', '留空则输出到标准输出'],
  'config.accessLogBuffer': ['Async buffer size', '异步队列大小'],
  'config.monitoring': ['Monitoring', '监控配置'],
  'config.monitoringEnabled': ['Enable Prometheus monitoring', '开启 Prometheus 监控'],
  'config.prometheusUrl': ['Prometheus URL', 'Prometheus 地址'],
  'config.authType': ['Auth type', '认证类型'],
  'config.username': ['Username', '用户名'],
  'config.password': ['Password', '密码'],
  'config.bearerToken': ['Bearer token', 'Bearer Token'],
  'config.headerName': ['Header name', 'Header 名称'],
  'config.headerValue': ['Header value', 'Header 值'],
  'action.reload': ['Reload config', '重新加载'],
  'action.newRule': ['New rule', '新建规则'],
  'action.newMatchSet': ['New match set', '新建匹配集'],
  'action.newUpstream': ['New upstream', '新建上游'],
  'action.save': ['Save', '保存'],
  'action.cancel': ['Cancel', '取消'],
  'action.create': ['Create', '创建'],
  'action.edit': ['Edit', '编辑'],
  'action.del': ['Del', '删除'],
  'action.enable': ['Enable', '启用'],
  'action.disable': ['Disable', '禁用'],
  'action.drain': ['Drain', '排空'],
  'action.apply': ['Apply', '应用'],
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
  'ops.avgLatencyShort': ['avg', '平均'],
  'ops.zoomOut': ['Zoom out', '缩小'],
  'ops.zoomIn': ['Zoom in', '放大'],
  'ops.zoomReset': ['Reset zoom', '重置缩放'],
  'ops.noRoutes': ['No routing rules configured. Traffic falls back to the default upstream URL.', '暂无路由规则，流量会进入默认兜底上游 URL。'],
  'ops.traffic': ['Recent proxy traffic', '近期代理流量'],
  'ops.trafficDesc': ['Requests grouped by rule, upstream, status, and latency histogram bucket.', '按规则、上游、状态和延迟直方图桶分组的请求。'],
  'ops.filterRules': ['Filter rules...', '过滤规则...'],
  'ops.noTraffic': ['No traffic data yet. Metrics will appear when the proxy receives requests.', '暂无流量数据。当代理收到请求时指标将出现。'],
  'ops.last24h': ['Last 24h across all upstreams', '过去 24 小时所有上游'],
  'ops.histogram': ['Histogram rustproxy_proxy_request_duration_seconds', '直方图 rustproxy_proxy_request_duration_seconds'],
  'ops.gauge': ['Gauge rustproxy_proxy_active_connections', '指标 rustproxy_proxy_active_connections'],
  'ops.sqliteReload': ['SQLite backed hot reload loop', 'SQLite 热重载'],
  'monitoring.title': ['Monitoring', '监控'],
  'monitoring.sub': ['Prometheus-backed resource, latency, request, and error trends.', '基于 Prometheus 展示资源、延迟、请求和错误趋势。'],
  'monitoring.disabled': ['Prometheus monitoring is disabled. Enable it in global configuration and set a Prometheus URL.', 'Prometheus 监控未开启。请在全局配置中开启并配置 Prometheus 地址。'],
  'monitoring.noData': ['No Prometheus data returned. Add this RustProxy /metrics endpoint to Prometheus scrape_configs and wait for samples.', '没有查询到 Prometheus 数据。请把当前 RustProxy 的 /metrics 端点加入 Prometheus scrape_configs，并等待采样。'],
  'monitoring.selector': ['Selector', '选择器'],
  'monitoring.entry': ['Entry', '入口'],
  'monitoring.route': ['Route', '链路'],
  'monitoring.allRoutes': ['All routes', '全部链路'],
  'monitoring.rps': ['RPS', '每秒请求'],
  'monitoring.errorRate': ['Error rate', '错误率'],
  'monitoring.memory': ['Memory', '内存'],
  'monitoring.cpu': ['CPU', 'CPU'],
  'monitoring.p50': ['P50 latency', 'P50 延迟'],
  'monitoring.p95': ['P95 latency', 'P95 延迟'],
  'monitoring.p99': ['P99 latency', 'P99 延迟'],
  'monitoring.scrapeHint': ['Example scrape target: ', '采集端点示例：'],
  'table.rule': ['Rule', '规则'],
  'table.upstream': ['Upstream', '上游'],
  'table.status': ['Status', '状态'],
  'table.requests': ['Requests', '请求数'],
  'table.id': ['ID', 'ID'],
  'table.name': ['Name', '名称'],
  'table.priority': ['Priority', '优先级'],
  'table.listen': ['Listen', '监听地址'],
  'table.host': ['Host', '域名'],
  'table.location': ['Location', '路径'],
  'table.pool': ['Upstream Pool', '上游池'],
  'table.weight': ['Weight', '权重'],
  'table.match': ['Match', '匹配'],
  'table.actions': ['Actions', '操作'],
  'table.targets': ['Targets', '目标'],
  'table.healthCheck': ['Health Check', '健康检查'],
  'table.healthTotal': ['total', '总数'],
  'table.healthUnhealthy': ['unhealthy', '异常'],
  'table.healthUnknown': ['unknown', '未知'],
  'table.off': ['Off', '关闭'],
  'table.noRules': ['No rules configured', '暂无路由规则'],
  'table.noUpstreams': ['No upstreams configured', '暂无上游服务'],
  'table.mode': ['Mode', '模式'],
  'table.health': ['Health', '健康'],
  'table.active': ['Active', '活跃'],
  'table.configuredWeight': ['Configured', '配置权重'],
  'table.effectiveWeight': ['Effective', '生效权重'],
  'table.overrideWeight': ['Override', '覆盖权重'],
  'table.lastError': ['Last error', '最近错误'],
  'runtime.title': ['Runtime Target Controls', '运行时目标控制'],
  'runtime.sub': ['Ephemeral target mode and weight overrides; config is not rewritten.', '临时调整目标模式和权重覆盖，不会写回配置。'],
  'runtime.empty': ['No runtime targets available', '暂无运行时目标'],
  'runtime.enabled': ['Enabled', '已启用'],
  'runtime.disabled': ['Disabled', '已禁用'],
  'runtime.drain': ['Drain', '排空'],
  'runtime.healthy': ['Healthy', '健康'],
  'runtime.unhealthy': ['Unhealthy', '异常'],
  'runtime.overrideNone': ['none', '无'],
  'notice.runtimeUpdated': ['Runtime target updated', '运行时目标已更新'],
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
  'form.entryHost': ['Entry and host', '入口与域名'],
  'form.location': ['Location', '路径规则'],
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
  'form.type.header': ['Match request header', '匹配请求头'],
  'form.type.cookie': ['Match cookie value', '匹配 Cookie 值'],
  'form.type.jwt': ['Match JWT claim', '匹配 JWT Claim'],
  'form.hostType': ['Host type', '域名类型'],
  'form.hostValue': ['Host value', '域名值'],
  'form.host.any': ['Any host', '任意域名'],
  'form.host.exact': ['Exact host', '精确域名'],
  'form.host.wildcard': ['Wildcard host', '通配域名'],
  'form.locationType': ['Location type', '路径类型'],
  'form.locationValue': ['Location value', '路径值'],
  'form.location.exact': ['Exact', '精确'],
  'form.location.prefix': ['Prefix', '前缀'],
  'form.location.regex': ['Regex', '正则'],
  'form.key': ['Key', '键'],
  'form.operator': ['Operator', '操作符'],
  'form.value': ['Value', '值'],
  'form.targets': ['Targets', '目标'],
  'form.balance': ['Load balancing', '负载均衡'],
  'form.algorithm': ['Algorithm', '算法'],
  'form.retry': ['Retry policy', '重试策略'],
  'form.retryAttempts': ['Max retries', '最大重试次数'],
  'form.retryStatus': ['Retry status codes', '重试状态码'],
  'form.retryStatusHint': ['Comma separated, for example 502,503,504', '逗号分隔，例如 502,503,504'],
  'form.retryTimeout': ['Retry on timeout', '超时重试'],
  'form.retryConnect': ['Retry on connect error', '连接错误重试'],
  'form.headers': ['Header policy', 'Header 策略'],
  'form.requestHeaders': ['Request headers', '请求头'],
  'form.responseHeaders': ['Response headers', '响应头'],
  'form.addHeader': ['Add header rule', '添加 Header 规则'],
  'form.pathActions': ['Path actions', '路径动作'],
  'form.addPathAction': ['Add path action', '添加路径动作'],
  'form.limitPolicy': ['Limits', '限流与连接控制'],
  'form.ratePerSecond': ['Rate per second', '每秒请求数'],
  'form.rateKey': ['Rate key', '限流维度'],
  'form.maxConnections': ['Max connections', '最大并发连接'],
  'form.maxBodyBytes': ['Max body bytes', '请求体上限字节'],
  'form.queueTimeoutMs': ['Queue timeout (ms)', '排队超时 (毫秒)'],
  'form.optionalZero': ['0 or empty means disabled', '0 或留空表示关闭'],
  'form.noneConfigured': ['No policy entries configured', '暂无策略条目'],
  'form.operation': ['Operation', '操作'],
  'form.headerName': ['Header name', 'Header 名称'],
  'form.headerValue': ['Header value', 'Header 值'],
  'form.actionType': ['Action type', '动作类型'],
  'form.prefix': ['Prefix', '前缀'],
  'form.pattern': ['Pattern', '模式'],
  'form.replacement': ['Replacement', '替换值'],
  'form.status': ['Status', '状态码'],
  'form.locationTarget': ['Location', '跳转地址'],
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
  'notice.loading': ['Loading', '加载中'],
  'notice.sessionExpired': ['Session expired, please sign in again.', '登录已过期，请重新登录。'],
};

/* ===== I18n Context ===== */

const I18nCtx = createContext<{ lang: Lang; t: (key: string) => string }>({ lang: 'zh', t: (k) => k });
const useI18n = () => useContext(I18nCtx);
const DEFAULT_LANG: Lang = 'zh';
const DEFAULT_THEME: Theme = 'light';
const AUTH_EXPIRED_EVENT = 'rustproxy:auth-expired';

/* ===== Constants ===== */

const defaultHealthCheck: HealthCheck = {
  enabled: false, mode: 'tcp', path: '/health', expected_status: 200,
  interval_seconds: 10, timeout_seconds: 2, healthy_threshold: 2, unhealthy_threshold: 2,
};
const defaultRetryPolicy: RetryPolicy = {
  attempts: 0,
  retry_on_status: [],
  retry_on_timeout: false,
  retry_on_connect_error: false,
};
const defaultHeaderPolicy: HeaderPolicy = { request: [], response: [] };
const defaultLimitPolicy: LimitPolicy = {
  rate_per_second: null,
  rate_key: 'ip',
  max_connections: null,
  max_body_bytes: null,
  queue_timeout_ms: null,
};
const BALANCE_ALGORITHMS: BalanceAlgorithm[] = ['weighted_round_robin', 'least_connections', 'ip_hash', 'consistent_hash', 'url_hash'];
const RATE_LIMIT_KEYS: RateLimitKey[] = ['ip', 'host', 'route'];
const HEADER_MUTATION_OPS: HeaderMutationOp[] = ['set', 'add', 'remove'];
const PATH_ACTION_TYPES = ['strip_prefix', 'rewrite', 'redirect'] as const;
type PathActionType = typeof PATH_ACTION_TYPES[number];

const NAV_ITEMS: { id: View; labelKey: string; icon: string }[] = [
  { id: 'operations', labelKey: 'nav.operations', icon: 'monitoring' },
  { id: 'monitoring', labelKey: 'nav.monitoring', icon: 'query_stats' },
  { id: 'rules', labelKey: 'nav.rules', icon: 'route' },
  { id: 'match-sets', labelKey: 'nav.matchSets', icon: 'rule_settings' },
  { id: 'upstreams', labelKey: 'nav.upstreams', icon: 'lan' },
  { id: 'certificates', labelKey: 'nav.certificates', icon: 'workspace_premium' },
  { id: 'config', labelKey: 'nav.config', icon: 'database' },
];

const ROUTE_FLOW_COLORS = ['#FF8400', '#000066', '#804200', '#004D1A'];
const ACCESS_LOG_LEVEL_OPTIONS: DropdownOption[] = [
  { value: 'debug', label: 'debug' },
  { value: 'info', label: 'info' },
  { value: 'warn', label: 'warn' },
  { value: 'error', label: 'error' },
];

const emptyConfig: AppConfig = {
  listen: '0.0.0.0:3000', proxy_listen: '0.0.0.0:80',
  certificate_dir: '/etc/rustproxy/cert.d', access_log: { enabled: false, path: null, buffer_size: 8192, level: 'info' },
  monitoring: { enabled: false, prometheus: { url: '', auth: { auth_type: 'none' } } },
  certificates: [], tls_listeners: [], match_sets: [],
  rules: [], upstreams: {}, fallback: { url: '404' },
};

function compactConfigForYaml(value: unknown): unknown {
  if (value == null) return undefined;

  if (Array.isArray(value)) {
    const compacted = value
      .map((item) => compactConfigForYaml(item))
      .filter((item) => item !== undefined);
    return compacted.length > 0 ? compacted : undefined;
  }

  if (typeof value !== 'object') return value;

  const source = value as Record<string, unknown>;
  if (source.enabled === false && 'prometheus' in source) {
    return { enabled: false };
  }
  if (source.enabled === false && 'mode' in source && 'expected_status' in source && 'interval_seconds' in source) {
    return { enabled: false };
  }
  if (source.enabled === false && ('buffer_size' in source || 'level' in source)) {
    return { enabled: false };
  }

  const compacted = Object.entries(source).reduce<Record<string, unknown>>((next, [key, item]) => {
    if (key === 'balance' && item === 'weighted_round_robin') return next;
    if (key === 'retry') {
      const compactRetry = compactRetryPolicy(item);
      if (compactRetry !== undefined) next[key] = compactRetry;
      return next;
    }
    if (key === 'limit_policy') {
      const compactLimit = compactLimitPolicy(item);
      if (compactLimit !== undefined) next[key] = compactLimit;
      return next;
    }
    if (key === 'request_timeout' && item === 0 && 'upstream' in source && 'priority' in source) return next;
    const compactItem = compactConfigForYaml(item);
    if (compactItem !== undefined) next[key] = compactItem;
    return next;
  }, {});
  return Object.keys(compacted).length > 0 ? compacted : undefined;
}

function compactRetryPolicy(value: unknown): unknown {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return value;
  const source = value as Record<string, unknown>;
  const attempts = Number(source.attempts ?? 0);
  const retryOnStatus = Array.isArray(source.retry_on_status) ? source.retry_on_status : [];
  const retryOnTimeout = Boolean(source.retry_on_timeout);
  const retryOnConnectError = Boolean(source.retry_on_connect_error);
  if (attempts === 0 && retryOnStatus.length === 0 && !retryOnTimeout && !retryOnConnectError) {
    return undefined;
  }
  const compacted: Record<string, unknown> = {};
  if (attempts > 0) compacted.attempts = attempts;
  if (retryOnStatus.length > 0) compacted.retry_on_status = retryOnStatus;
  if (retryOnTimeout) compacted.retry_on_timeout = true;
  if (retryOnConnectError) compacted.retry_on_connect_error = true;
  return compacted;
}

function compactLimitPolicy(value: unknown): unknown {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return value;
  const source = value as Record<string, unknown>;
  const compacted: Record<string, unknown> = {};
  if (source.rate_per_second != null) compacted.rate_per_second = source.rate_per_second;
  if (source.rate_key && source.rate_key !== 'ip') compacted.rate_key = source.rate_key;
  if (source.max_connections != null) compacted.max_connections = source.max_connections;
  if (source.max_body_bytes != null) compacted.max_body_bytes = source.max_body_bytes;
  if (source.queue_timeout_ms != null) compacted.queue_timeout_ms = source.queue_timeout_ms;
  return Object.keys(compacted).length > 0 ? compacted : undefined;
}

function dumpConfigYaml(config: AppConfig): string {
  return yaml.dump(compactConfigForYaml(config) ?? {}, { lineWidth: 110 });
}

/* ===== Icon ===== */

function Icon({ name, size = 22 }: { name: string; size?: number }) {
  return <span className="material-symbols-sharp" style={{ fontSize: size }} aria-hidden="true">{name}</span>;
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
  const [lang, setLang] = useState<Lang>(() => readStoredLang());
  const [theme, setTheme] = useState<Theme>(() => readStoredTheme());
  const [versionInfo, setVersionInfo] = useState<VersionInfo | null>(null);

  const t = useMemo(() => {
    const idx = lang === 'en' ? 0 : 1;
    return (key: string) => T[key]?.[idx] ?? key;
  }, [lang]);

  const changeLang = useCallback((l: Lang) => { setLang(l); localStorage.setItem('rustproxy_lang', l); }, []);
  const changeTheme = useCallback((nextTheme: Theme) => {
    setTheme(nextTheme);
    localStorage.setItem('rustproxy_theme', nextTheme);
  }, [token]);

  useEffect(() => {
    document.documentElement.lang = lang === 'zh' ? 'zh-CN' : 'en';
  }, [lang]);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  useEffect(() => {
    function handleAuthExpired() {
      localStorage.removeItem('rustproxy_token');
      setToken('');
      setConfig(emptyConfig);
      setNotice({ type: 'error', message: t('notice.sessionExpired') });
    }
    window.addEventListener(AUTH_EXPIRED_EVENT, handleAuthExpired);
    return () => window.removeEventListener(AUTH_EXPIRED_EVENT, handleAuthExpired);
  }, [t]);

  useEffect(() => {
    let cancelled = false;
    const markIconsReady = () => {
      if (!cancelled) document.documentElement.classList.add('icons-ready');
    };
    if (!('fonts' in document)) {
      markIconsReady();
      return () => { cancelled = true; };
    }
    document.fonts.load('24px "Material Symbols Sharp"', 'route').then((fonts) => {
      if (fonts.length > 0) markIconsReady();
    }).catch(() => {
      document.documentElement.classList.remove('icons-ready');
    });
    return () => { cancelled = true; };
  }, [token]);

  useEffect(() => {
    api<{ users_exist: boolean }>('/api/auth/setup-status')
      .then((data) => setNeedsSetup(!data.users_exist))
      .catch(() => setNeedsSetup(false));
  }, []);

  useEffect(() => {
    api<VersionInfo>('/api/version')
      .then(setVersionInfo)
      .catch(() => setVersionInfo(null));
  }, []);

  useEffect(() => {
    if (!token || needsSetup) { setLoading(false); return; }
    refreshConfig(token, setConfig, setNotice).finally(() => setLoading(false));
  }, [token, needsSetup]);

  useEffect(() => {
    if (view === 'monitoring' && !monitoringEnabled(config)) {
      setView('operations');
      window.history.replaceState(null, '', '/admin/');
    }
  }, [config, view]);

  function navigate(next: View) {
    if (next === 'monitoring' && !monitoringEnabled(config)) next = 'operations';
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
        <Sidebar view={view} navigate={navigate} config={config} versionInfo={versionInfo} logout={logout} lang={lang} changeLang={changeLang} theme={theme} changeTheme={changeTheme} />
        <main className="workspace">
          {notice && <Toast notice={notice} onClose={() => setNotice(null)} />}
          {view === 'operations' && <OperationsView config={config} token={token} />}
          {view === 'monitoring' && <MonitoringView config={config} token={token} />}
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

function Sidebar({ view, navigate, config, versionInfo, logout, lang, changeLang, theme, changeTheme }: {
  view: View; navigate: (v: View) => void; config: AppConfig; versionInfo: VersionInfo | null; logout: () => void; lang: Lang; changeLang: (l: Lang) => void; theme: Theme; changeTheme: (theme: Theme) => void;
}) {
  const { t } = useI18n();
  const versionTitle = versionInfo
    ? `${versionInfo.git_ref_kind}:${versionInfo.git_ref} ${versionInfo.git_commit}${versionInfo.dirty ? ' dirty' : ''}`
    : undefined;
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
        {NAV_ITEMS.filter((item) => item.id !== 'monitoring' || monitoringEnabled(config)).map((item) => (
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
          <div className="listener-value">{config.listen || '0.0.0.0:3000'}</div>
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
      {versionInfo && (
        <div className="sidebar-build" title={versionTitle}>
          <div className="sidebar-build-label">{t('version.label')}</div>
          <div className="sidebar-build-value">{versionInfo.version}</div>
        </div>
      )}
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
  const [upstreamHealth, setUpstreamHealth] = useState<UpstreamHealth[]>([]);
  const [cpuUsage, setCpuUsage] = useState(0);
  const [routeZoom, setRouteZoom] = useState(1);
  const previousCpu = useRef<{ total: number; at: number } | null>(null);
  useEffect(() => {
    let active = true;
    async function loadMetrics() {
      try {
        const [text, health] = await Promise.all([
          fetch('/metrics').then((r) => r.text()),
          api<UpstreamHealth[]>('/api/upstream-health', { token }).catch(() => []),
        ]);
        if (!active) return;
        const nextMetrics = parsePrometheus(text);
        const cpuTotal = metricValue(nextMetrics, 'rustproxy_process_cpu_seconds_total');
        const now = Date.now();
        if (cpuTotal !== null && previousCpu.current) {
          const cpuDelta = Math.max(0, cpuTotal - previousCpu.current.total);
          const secondsDelta = Math.max(1, (now - previousCpu.current.at) / 1000);
          setCpuUsage((cpuDelta / secondsDelta) * 100);
        }
        if (cpuTotal !== null) previousCpu.current = { total: cpuTotal, at: now };
        setMetrics(nextMetrics);
        setUpstreamHealth(health);
      } catch (_) {}
    }
    loadMetrics();
    const timer = window.setInterval(loadMetrics, 5000);
    return () => { active = false; window.clearInterval(timer); };
  }, []);

  const totalRequests = useMemo(() => metrics.filter((m) => m.name === 'rustproxy_proxy_requests_total').reduce((s, m) => s + m.value, 0), [metrics]);
  const activeConns = useMemo(() => metrics.find((m) => m.name === 'rustproxy_proxy_active_connections')?.value ?? 0, [metrics]);
  const residentMemory = useMemo(() => metricValue(metrics, 'rustproxy_process_resident_memory_bytes') ?? 0, [metrics]);
  const openFds = useMemo(() => metricValue(metrics, 'rustproxy_process_open_fds') ?? 0, [metrics]);
  const avgLatency = useMemo(() => {
    const sum = metrics.filter((m) => m.name === 'rustproxy_proxy_request_duration_seconds_sum').reduce((total, metric) => total + metric.value, 0);
    const count = metrics.filter((m) => m.name === 'rustproxy_proxy_request_duration_seconds_count').reduce((total, metric) => total + metric.value, 0);
    return count > 0 ? (sum / count) * 1000 : 0;
  }, [metrics]);
  const reloadCount = useMemo(() => metrics.find((m) => m.name === 'rustproxy_proxy_config_reloads_total')?.value ?? 0, [metrics]);

  const requestsByRule = useMemo(() => {
    const byRule: Record<string, number> = {};
    metrics.filter((m) => m.name === 'rustproxy_proxy_requests_total').forEach((m) => {
      const rule = m.labels.rule || 'unknown';
      byRule[rule] = (byRule[rule] || 0) + m.value;
    });
    return byRule;
  }, [metrics]);

  const avgLatencyByRoute = useMemo(() => {
    const sums: Record<string, number> = {};
    const counts: Record<string, number> = {};
    metrics.forEach((metric) => {
      if (metric.name !== 'rustproxy_proxy_request_duration_seconds_sum' && metric.name !== 'rustproxy_proxy_request_duration_seconds_count') return;
      const key = `${metric.labels.rule || 'unknown'}\u0000${metric.labels.upstream || '-'}`;
      if (metric.name.endsWith('_sum')) sums[key] = (sums[key] || 0) + metric.value;
      if (metric.name.endsWith('_count')) counts[key] = (counts[key] || 0) + metric.value;
    });
    return Object.fromEntries(Object.keys(counts).map((key) => [key, counts[key] > 0 ? (sums[key] || 0) / counts[key] * 1000 : 0]));
  }, [metrics]);

  const trafficRows = useMemo(() => metrics.filter((m) => m.name === 'rustproxy_proxy_requests_total').map((m) => ({
    rule: m.labels.rule || '-', ruleId: m.labels.rule || '-',
    upstream: m.labels.upstream || '-', status: m.labels.status || '-', count: m.value,
  })), [metrics]);
  const upstreams = useMemo(() => Object.values(config.upstreams ?? {}), [config]);
  const upstreamHealthByName = useMemo(() => Object.fromEntries(upstreamHealth.map((item) => [item.upstream, item])), [upstreamHealth]);
  const routeFlowGroups = useMemo(() => {
    const defaultListen = config.proxy_listen || '0.0.0.0:80';
    const grouped = new Map<string, {
      entry: string;
      color: string;
      flows: {
        rule: Rule;
        upstream?: Upstream;
        requests: number;
        avgLatency: number;
        color: string;
      }[];
    }>();
    [...config.rules]
      .sort((a, b) => summarizeHost(a.host).localeCompare(summarizeHost(b.host)) || summarizeLocation(a.location).localeCompare(summarizeLocation(b.location)) || Number(a.is_fallback) - Number(b.is_fallback) || b.priority - a.priority)
      .forEach((rule) => {
        const entry = `${rule.tls?.enabled ? 'https' : 'http'}://${rule.listen || defaultListen}`;
        if (!grouped.has(entry)) {
          grouped.set(entry, { entry, color: ROUTE_FLOW_COLORS[grouped.size % ROUTE_FLOW_COLORS.length], flows: [] });
        }
        const group = grouped.get(entry)!;
        group.flows.push({
          rule,
          upstream: config.upstreams?.[rule.upstream],
          requests: requestsByRule[rule.name || rule.id] ?? 0,
          avgLatency: avgLatencyByRoute[`${rule.name || rule.id}\u0000${rule.upstream}`] ?? 0,
          color: ROUTE_FLOW_COLORS[group.flows.length % ROUTE_FLOW_COLORS.length],
        });
    });
    return [...grouped.values()];
  }, [config, requestsByRule, avgLatencyByRoute]);
  const routeZoomPercent = Math.round(routeZoom * 100);
  const updateRouteZoom = useCallback((delta: number) => {
    setRouteZoom((current) => Math.min(1.4, Math.max(0.8, Number((current + delta).toFixed(2)))));
  }, []);

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
            <div className="routing-canvas-shell">
              <div className="routing-canvas-toolbar">
                <button className="canvas-tool-btn" type="button" title={t('ops.zoomOut')} onClick={() => updateRouteZoom(-0.1)}><Icon name="remove" size={16} /></button>
                <button className="canvas-zoom-readout" type="button" title={t('ops.zoomReset')} onClick={() => setRouteZoom(1)}>{routeZoomPercent}%</button>
                <button className="canvas-tool-btn" type="button" title={t('ops.zoomIn')} onClick={() => updateRouteZoom(0.1)}><Icon name="add" size={16} /></button>
              </div>
              <div className="routing-canvas-viewport">
                <div className="routing-canvas-surface" style={{ transform: `scale(${routeZoom})` }}>
                  {routeFlowGroups.length > 0 ? routeFlowGroups.map((group) => (
                    <div className="flow-group" key={group.entry} style={{ borderLeftColor: group.color }}>
                      <div className="flow-node flow-entry">
                        <span className="flow-node-label">{t('ops.entry')}</span>
                        <strong>{group.entry}</strong>
                        <small>{lang === 'en' ? `${group.flows.length} route${group.flows.length !== 1 ? 's' : ''}` : `${group.flows.length} 条链路`}</small>
                      </div>
                      <div className="flow-trunk" aria-hidden="true"><span /></div>
                      <div className="flow-branches">
                        {group.flows.map(({ rule, upstream, requests, avgLatency, color }, index) => {
                          const locationText = summarizeLocation(rule.location);
                          const hostText = summarizeHost(rule.host, t);
                          const ruleText = rule.name || rule.id || '-';
                          const upstreamHealthItem = upstreamHealthByName[rule.upstream];
                          const flowHealth = flowHealthVisual(upstreamHealthItem);
                          return (
                            <div
                              className={`flow-branch ${rule.is_fallback ? 'is-fallback' : ''} ${flowHealth.className}`}
                              key={rule.id || `${group.entry}-${index}`}
                              style={{ '--flow-color': flowHealth.color } as CSSProperties}
                            >
                              <span className="flow-motion-line" aria-hidden="true"><span /></span>
                              <span className="flow-branch-rail" style={{ background: color }} />
                              <div className="flow-priority">
                                <span title={rule.is_fallback ? t('rule.fallback') : `${t('ops.priorityShort')}${rule.priority}`}>
                                  {rule.is_fallback ? t('rule.fallback') : `${t('ops.priorityShort')}${rule.priority}`}
                                </span>
                                {rule.is_fallback && <Icon name="shield" size={15} />}
                              </div>
                              <Icon name="arrow_forward" size={16} />
                              <div className="flow-node flow-host">
                                <span className="flow-node-label">{t('table.host')}</span>
                                <strong title={hostText}>{hostText}</strong>
                              </div>
                              <Icon name="arrow_forward" size={16} />
                              <div className="flow-node flow-location">
                                <span className="flow-node-label">{t('table.location')}</span>
                                <strong title={locationText}>{locationText}</strong>
                              </div>
                              <Icon name="arrow_forward" size={16} />
                              <div className="flow-node flow-rule-name">
                                <span className="flow-node-label">{t('table.rule')}</span>
                                <strong title={`${ruleText} · ${rule.id}`}>{ruleText}</strong>
                              </div>
                              <Icon name="arrow_forward" size={16} />
                              <div className="flow-node flow-upstream">
                                <span className="flow-node-label">{t('table.upstream')}</span>
                                <strong title={rule.upstream}>{rule.upstream}</strong>
                                <small title={upstream ? targetSummary(upstream, lang) : 'missing upstream'}>{upstream ? targetSummary(upstream, lang) : 'missing upstream'}</small>
                              </div>
                              <div className="flow-metrics">
                                <span>{formatNumber(requests)} {t('ops.requestsShort')}</span>
                                <strong>{avgLatency > 0 ? `${t('ops.avgLatencyShort')} ${formatLatency(avgLatency)}` : `${t('ops.avgLatencyShort')} —`}</strong>
                              </div>
                              <div className="flow-rule-popover" role="tooltip">
                                <span>{t('table.rule')} · {rule.is_fallback ? t('rule.fallback') : `${t('ops.priorityShort')}${rule.priority}`}</span>
                                <strong>{ruleText}</strong>
                                <small>{rule.is_fallback ? t('rule.fallbackHelp') : summarizeRuleMatch(rule)}</small>
                                <div className={`popover-health ${flowHealth.className}`}>
                                  <span>{t('table.healthCheck')}</span>
                                  <strong>{healthSummaryText(upstreamHealthItem, t, upstream)}</strong>
                                </div>
                              </div>
                            </div>
                          );
                        })}
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
                hint="rustproxy_proxy_active_connections"
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

/* ===== Monitoring View ===== */

function MonitoringView({ config, token }: { config: AppConfig; token: string }) {
  const { t } = useI18n();
  const entries = useMemo(() => uniqueListeners(config), [config]);
  const routeOptions = useMemo(() => (
    config.rules.map((rule) => ({ value: routeKey(rule), label: `${rule.name || rule.id} -> ${rule.upstream}` }))
  ), [config.rules]);
  const [entry, setEntry] = useState(entries[0] ?? config.proxy_listen ?? '0.0.0.0:80');
  const [route, setRoute] = useState(routeOptions[0]?.value ?? '');
  const [series, setSeries] = useState<Record<string, ChartPoint[]>>({});
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    if (!entries.includes(entry) && entries[0]) setEntry(entries[0]);
  }, [entries, entry]);

  useEffect(() => {
    if (!routeOptions.some((option) => option.value === route)) setRoute(routeOptions[0]?.value ?? '');
  }, [routeOptions, route]);

  useEffect(() => {
    if (!monitoringEnabled(config)) return;
    let active = true;
    async function load() {
      setLoading(true);
      setError('');
      const end = Math.floor(Date.now() / 1000);
      const start = end - 15 * 60;
      const step = '15s';
      const labels = monitoringLabelSelector(entry, config.rules.find((rule) => routeKey(rule) === route));
      const queries: Record<string, string> = {
        rps: `sum(rate(rustproxy_proxy_requests_total{${labels}}[1m]))`,
        errorRate: `sum(rate(rustproxy_proxy_requests_total{${labels},status=~"5.."}[1m])) / clamp_min(sum(rate(rustproxy_proxy_requests_total{${labels}}[1m])), 0.001) * 100`,
        p50: `histogram_quantile(0.50, sum(rate(rustproxy_proxy_request_duration_seconds_bucket{${labels}}[5m])) by (le)) * 1000`,
        p95: `histogram_quantile(0.95, sum(rate(rustproxy_proxy_request_duration_seconds_bucket{${labels}}[5m])) by (le)) * 1000`,
        p99: `histogram_quantile(0.99, sum(rate(rustproxy_proxy_request_duration_seconds_bucket{${labels}}[5m])) by (le)) * 1000`,
        cpu: `rate(rustproxy_process_cpu_seconds_total[1m]) * 100`,
        memory: `rustproxy_process_resident_memory_bytes`,
      };
      try {
        const result = await Promise.all(Object.entries(queries).map(async ([key, query]) => {
          const data = await prometheusRange(token, query, start, end, step);
          return [key, firstSeries(data)] as const;
        }));
        if (active) setSeries(Object.fromEntries(result));
      } catch (err) {
        if (active) setError(errorMessage(err));
      } finally {
        if (active) setLoading(false);
      }
    }
    load();
    const timer = window.setInterval(load, 15000);
    return () => { active = false; window.clearInterval(timer); };
  }, [config, entry, route, token]);

  if (!monitoringEnabled(config)) {
    return <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}><ViewHeader title={t('monitoring.title')} subtitle={t('monitoring.sub')} /><div className="monitoring-empty"><Icon name="query_stats" size={24} /><span>{t('monitoring.disabled')}</span></div></div>;
  }

  const hasData = Object.values(series).some((points) => points.length > 0);
  const scrapeTarget = `${config.listen || '0.0.0.0:3000'}/metrics`;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
      <ViewHeader title={t('monitoring.title')} subtitle={t('monitoring.sub')} />
      <div className="monitoring-toolbar">
        <Field label={t('monitoring.entry')}><Dropdown value={entry} options={entries.map((value) => ({ value, label: value }))} onChange={setEntry} /></Field>
        <Field label={t('monitoring.route')}><Dropdown value={route} options={routeOptions} onChange={setRoute} /></Field>
      </div>
      {(error || (!loading && !hasData)) && <div className="monitoring-empty"><Icon name="info" size={20} /><span>{error || t('monitoring.noData')}</span><small>{t('monitoring.scrapeHint')}{scrapeTarget}</small></div>}
      <div className="monitoring-grid">
        <MetricChart title={t('monitoring.rps')} points={series.rps ?? []} unit="" />
        <MetricChart title={t('monitoring.errorRate')} points={series.errorRate ?? []} unit="%" />
        <MetricChart title={t('monitoring.p50')} points={series.p50 ?? []} unit="ms" />
        <MetricChart title={t('monitoring.p95')} points={series.p95 ?? []} unit="ms" />
        <MetricChart title={t('monitoring.p99')} points={series.p99 ?? []} unit="ms" />
        <MetricChart title={t('monitoring.cpu')} points={series.cpu ?? []} unit="%" />
        <MetricChart title={t('monitoring.memory')} points={series.memory ?? []} formatter={formatBytes} />
      </div>
    </div>
  );
}

function MetricChart({ title, points, unit, formatter }: { title: string; points: ChartPoint[]; unit?: string; formatter?: (v: number) => string }) {
  const { lang } = useI18n();
  const chartRef = useRef<SVGSVGElement | null>(null);
  const [hoverIndex, setHoverIndex] = useState<number | null>(null);
  const latest = points.at(-1)?.v ?? 0;
  const formatValue = useCallback((value: number) => (
    formatter ? formatter(value) : `${Number.isFinite(value) ? value.toFixed(value >= 10 ? 1 : 2) : '0'}${unit ? ` ${unit}` : ''}`
  ), [formatter, unit]);
  const display = formatValue(latest);
  const chart = useMemo(() => sparklineChart(points, 520, 150), [points]);
  const hoverPoint = hoverIndex == null ? null : chart.points[hoverIndex];
  const hoverValue = hoverIndex == null ? null : points[hoverIndex];
  const hoverTime = hoverValue
    ? new Intl.DateTimeFormat(lang === 'zh' ? 'zh-CN' : 'en-US', { hour: '2-digit', minute: '2-digit', second: '2-digit' }).format(new Date(hoverValue.t * 1000))
    : '';

  function handlePointerMove(event: PointerEvent<SVGSVGElement>) {
    if (chart.points.length === 0) return;
    const rect = chartRef.current?.getBoundingClientRect();
    if (!rect) return;
    const x = ((event.clientX - rect.left) / rect.width) * 520;
    let nearest = 0;
    let nearestDistance = Math.abs(chart.points[0].x - x);
    chart.points.forEach((point, index) => {
      const distance = Math.abs(point.x - x);
      if (distance < nearestDistance) {
        nearest = index;
        nearestDistance = distance;
      }
    });
    setHoverIndex(nearest);
  }

  return (
    <div className="monitor-chart">
      <div className="monitor-chart-head"><span>{title}</span><strong>{display}</strong></div>
      <div className="monitor-chart-plot">
        <svg
          ref={chartRef}
          viewBox="0 0 520 150"
          preserveAspectRatio="none"
          role="img"
          onPointerMove={handlePointerMove}
          onPointerLeave={() => setHoverIndex(null)}
        >
          <path className="monitor-chart-grid" d="M0 120H520 M0 80H520 M0 40H520" />
          {chart.path ? <path className="monitor-chart-line" d={chart.path} /> : <text x="260" y="82" textAnchor="middle">no data</text>}
          {hoverPoint && (
            <>
              <path className="monitor-chart-hover-line" d={`M${hoverPoint.x.toFixed(1)} 0V150`} />
              <circle className="monitor-chart-hover-dot" cx={hoverPoint.x} cy={hoverPoint.y} r="4.5" />
            </>
          )}
        </svg>
        {hoverPoint && hoverValue && (
          <div
            className={`monitor-chart-tooltip ${hoverPoint.x < 80 ? 'align-left' : hoverPoint.x > 440 ? 'align-right' : ''}`}
            style={{ left: `${(hoverPoint.x / 520) * 100}%`, top: `${(hoverPoint.y / 150) * 100}%` }}
          >
            <span>{hoverTime}</span>
            <strong>{formatValue(hoverValue.v)}</strong>
          </div>
        )}
      </div>
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
  function openEdit(rule: Rule) {
    const normalized = normalizeRule(rule);
    setEditing(rule.id);
    setDraft({ ...normalized, listen: normalized.listen || defaultListen });
    setShowModal(true);
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    const body = {
      ...draft,
      host: normalizeHostMatcher(draft.host),
      location: normalizeLocationMatcher(draft.location),
      match_set: draft.is_fallback ? null : draft.match_set || null,
      conditions: draft.is_fallback || draft.match_set ? null : normalizeCondition(draft.conditions),
      priority: draft.is_fallback ? 0 : draft.priority,
      weight: 100,
      listen: draft.listen || defaultListen,
      request_timeout: draft.is_fallback ? 0 : Number(draft.request_timeout ?? 0),
      tls: draft.is_fallback ? null : draft.tls?.enabled ? { enabled: true, certificate: draft.tls.certificate } : null,
      header_policy: normalizeHeaderPolicy(draft.header_policy),
      path_actions: normalizePathActions(draft.path_actions),
      limit_policy: normalizeLimitPolicy(draft.limit_policy),
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

  const sorted = [...config.rules].map(normalizeRule).sort((a, b) => Number(a.is_fallback) - Number(b.is_fallback) || b.priority - a.priority);

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
          <th style={{ width: 130 }}>{t('table.host')}</th><th style={{ width: 130 }}>{t('table.location')}</th>
          <th style={{ width: 180 }}>{t('table.pool')}</th>
          <th>{t('table.match')}</th><th style={{ width: 120 }}>{t('table.actions')}</th>
        </tr></thead><tbody>
          {sorted.length === 0 ? (
            <tr><td colSpan={9} style={{ textAlign: 'center', color: 'var(--muted-foreground)', padding: 40 }}>{t('table.noRules')}</td></tr>
          ) : sorted.map((rule) => (
            <tr key={rule.id}>
              <td className="td-mono">{rule.id}</td>
              <td style={{ fontWeight: 500 }}>{rule.name || rule.id}</td>
              <td className="td-mono">{rule.priority}</td>
              <td className="td-mono">{rule.is_fallback ? `${t('rule.fallback')} · ${rule.listen || defaultListen}` : `${rule.tls?.enabled ? 'HTTPS ' : ''}${rule.listen || defaultListen}`}</td>
              <td className="td-mono">{summarizeHost(rule.host, t)}</td>
              <td className="td-mono">{summarizeLocation(rule.location)}</td>
              <td className="td-upstream"><span className="td-badge" title={rule.upstream}>{rule.upstream}</span></td>
              <td className="td-mono">{rule.is_fallback ? t('rule.enableFallback') : summarizeRuleMatch(rule)}</td>
              <td className="td-actions">
                <div className="td-action-row">
                  <button className="btn btn-ghost btn-sm" onClick={() => openEdit(rule)}>{t('action.edit')}</button>
                  <button className="btn btn-danger btn-sm" onClick={() => remove(rule.id)}>{t('action.del')}</button>
                </div>
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
            <section className="form-section">
              <h3 className="form-section-title">{t('form.entryHost')}</h3>
              <div className="form-grid-3">
                <Field label={t('table.listen')}><input placeholder={config.proxy_listen || '0.0.0.0:80'} value={draft.listen ?? ''} onChange={(e) => setDraft({ ...draft, listen: e.target.value })} /></Field>
                <Field label={t('form.hostType')}>
                  <Dropdown value={draft.host.type} options={hostTypeOptions(t)} onChange={(type) => setDraft({ ...draft, host: { type: type as HostMatchType, value: type === 'any' ? null : draft.host.value || '' } })} />
                </Field>
                {draft.host.type !== 'any' && <Field label={t('form.hostValue')}><input placeholder={draft.host.type === 'wildcard' ? '*.example.com' : 'api.example.com'} value={draft.host.value ?? ''} onChange={(e) => setDraft({ ...draft, host: { ...draft.host, value: e.target.value } })} required /></Field>}
              </div>
            </section>
            <section className="form-section">
              <h3 className="form-section-title">{t('form.location')}</h3>
              <div className="form-grid">
                <Field label={t('form.locationType')}>
                  <Dropdown value={draft.location.type} options={locationTypeOptions(t)} onChange={(type) => setDraft({ ...draft, location: { type: type as LocationMatchType, value: draft.location.value || '/' } })} />
                </Field>
                <Field label={t('form.locationValue')}><input placeholder={draft.location.type === 'regex' ? '^/api/v[0-9]+/' : '/api'} value={draft.location.value} onChange={(e) => setDraft({ ...draft, location: { ...draft.location, value: e.target.value } })} required /></Field>
              </div>
            </section>
            {!draft.is_fallback && <section className="form-section">
              <h3 className="form-section-title">{t('form.routing')}</h3>
              <Field label={t('table.priority')}><input type="number" value={draft.priority} onChange={(e) => setDraft({ ...draft, priority: Number(e.target.value) })} /></Field>
              <Field label={t('rule.requestTimeout')}><input type="number" min="0" placeholder={t('rule.requestTimeoutInherit')} value={draft.request_timeout ?? 0} onChange={(e) => setDraft({ ...draft, request_timeout: Number(e.target.value) })} /></Field>
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
            <HeaderPolicyEditor draft={draft} setDraft={setDraft} />
            <PathActionsEditor draft={draft} setDraft={setDraft} />
            <LimitPolicyEditor draft={draft} setDraft={setDraft} />
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

/* ===== Rule Policy Editors ===== */

function HeaderPolicyEditor({ draft, setDraft }: { draft: Rule; setDraft: (rule: Rule) => void }) {
  const { t } = useI18n();
  const policy = normalizeHeaderPolicy(draft.header_policy);
  function update(kind: keyof HeaderPolicy, items: HeaderMutation[]) {
    setDraft({ ...draft, header_policy: { ...policy, [kind]: items.map(normalizeHeaderMutation) } });
  }
  return (
    <section className="form-section">
      <h3 className="form-section-title">{t('form.headers')}</h3>
      <HeaderMutationList
        title={t('form.requestHeaders')}
        items={policy.request}
        onChange={(items) => update('request', items)}
      />
      <HeaderMutationList
        title={t('form.responseHeaders')}
        items={policy.response}
        onChange={(items) => update('response', items)}
      />
    </section>
  );
}

function HeaderMutationList({ title, items, onChange }: {
  title: string;
  items: HeaderMutation[];
  onChange: (items: HeaderMutation[]) => void;
}) {
  const { t } = useI18n();
  function replace(index: number, item: HeaderMutation) {
    onChange(items.map((current, i) => i === index ? normalizeHeaderMutation(item) : current));
  }
  return (
    <div className="policy-subsection">
      <div className="policy-subsection-head">
        <span className="field-label">{title}</span>
        <button type="button" className="btn btn-secondary btn-sm" onClick={() => onChange([...items, createHeaderMutation()])}>
          <Icon name="add" size={16} />{t('form.addHeader')}
        </button>
      </div>
      {items.length === 0 ? <p className="card-desc">{t('form.noneConfigured')}</p> : (
        <div className="policy-list">
          {items.map((mutation, index) => (
            <div className="policy-row header-policy-row" key={index}>
              <Field label={t('form.operation')}>
                <Dropdown
                  value={mutation.op}
                  options={enumOptions(HEADER_MUTATION_OPS)}
                  onChange={(op) => replace(index, { ...mutation, op: op as HeaderMutationOp })}
                />
              </Field>
              <Field label={t('form.headerName')}>
                <input value={mutation.name} placeholder="x-forwarded-for" onChange={(e) => replace(index, { ...mutation, name: e.target.value })} />
              </Field>
              <Field label={t('form.headerValue')}>
                <input
                  value={mutation.value ?? ''}
                  disabled={mutation.op === 'remove'}
                  placeholder={mutation.op === 'remove' ? '' : '$remote_addr'}
                  onChange={(e) => replace(index, { ...mutation, value: e.target.value })}
                />
              </Field>
              <button type="button" className="btn btn-ghost btn-sm policy-remove" onClick={() => onChange(items.filter((_, i) => i !== index))}>
                <Icon name="close" size={16} />
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function PathActionsEditor({ draft, setDraft }: { draft: Rule; setDraft: (rule: Rule) => void }) {
  const { t } = useI18n();
  const actions = normalizePathActions(draft.path_actions);
  function replace(index: number, action: PathAction) {
    setDraft({ ...draft, path_actions: actions.map((current, i) => i === index ? action : current) });
  }
  return (
    <section className="form-section">
      <div className="policy-subsection-head">
        <h3 className="form-section-title">{t('form.pathActions')}</h3>
        <button type="button" className="btn btn-secondary btn-sm" onClick={() => setDraft({ ...draft, path_actions: [...actions, createPathAction()] })}>
          <Icon name="add" size={16} />{t('form.addPathAction')}
        </button>
      </div>
      {actions.length === 0 ? <p className="card-desc">{t('form.noneConfigured')}</p> : (
        <div className="policy-list">
          {actions.map((action, index) => {
            const type = pathActionType(action);
            return (
              <div className="policy-row path-policy-row" key={index}>
                <Field label={t('form.actionType')}>
                  <Dropdown
                    value={type}
                    options={enumOptions(PATH_ACTION_TYPES)}
                    onChange={(nextType) => replace(index, createPathAction(nextType as PathActionType, action))}
                  />
                </Field>
                {type === 'strip_prefix' && 'strip_prefix' in action && (
                  <Field label={t('form.prefix')}>
                    <input value={action.strip_prefix.prefix} placeholder="/api" onChange={(e) => replace(index, { strip_prefix: { prefix: e.target.value } })} />
                  </Field>
                )}
                {type === 'rewrite' && 'rewrite' in action && (
                  <>
                    <Field label={t('form.pattern')}>
                      <input value={action.rewrite.pattern} placeholder="^/v1/(.*)" onChange={(e) => replace(index, { rewrite: { ...action.rewrite, pattern: e.target.value } })} />
                    </Field>
                    <Field label={t('form.replacement')}>
                      <input value={action.rewrite.replacement} placeholder="/api/$1" onChange={(e) => replace(index, { rewrite: { ...action.rewrite, replacement: e.target.value } })} />
                    </Field>
                  </>
                )}
                {type === 'redirect' && 'redirect' in action && (
                  <>
                    <Field label={t('form.status')}>
                      <Dropdown
                        value={String(action.redirect.status)}
                        options={[301, 302].map((status) => ({ value: String(status), label: String(status) }))}
                        onChange={(status) => replace(index, { redirect: { ...action.redirect, status: Number(status) } })}
                      />
                    </Field>
                    <Field label={t('form.locationTarget')}>
                      <input value={action.redirect.location} placeholder="https://example.com" onChange={(e) => replace(index, { redirect: { ...action.redirect, location: e.target.value } })} />
                    </Field>
                  </>
                )}
                <button type="button" className="btn btn-ghost btn-sm policy-remove" onClick={() => setDraft({ ...draft, path_actions: actions.filter((_, i) => i !== index) })}>
                  <Icon name="close" size={16} />
                </button>
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}

function LimitPolicyEditor({ draft, setDraft }: { draft: Rule; setDraft: (rule: Rule) => void }) {
  const { t } = useI18n();
  const policy = normalizeLimitPolicy(draft.limit_policy);
  function update(patch: Partial<LimitPolicy>) {
    setDraft({ ...draft, limit_policy: normalizeLimitPolicy({ ...policy, ...patch }) });
  }
  return (
    <section className="form-section">
      <h3 className="form-section-title">{t('form.limitPolicy')}</h3>
      <div className="form-grid-3">
        <Field label={t('form.ratePerSecond')}>
          <input type="number" min="0" placeholder={t('form.optionalZero')} value={policy.rate_per_second ?? ''} onChange={(e) => update({ rate_per_second: parseOptionalPositive(e.target.value) })} />
        </Field>
        <Field label={t('form.rateKey')}>
          <Dropdown value={policy.rate_key} options={enumOptions(RATE_LIMIT_KEYS)} onChange={(rate_key) => update({ rate_key: rate_key as RateLimitKey })} />
        </Field>
        <Field label={t('form.maxConnections')}>
          <input type="number" min="0" placeholder={t('form.optionalZero')} value={policy.max_connections ?? ''} onChange={(e) => update({ max_connections: parseOptionalPositive(e.target.value) })} />
        </Field>
        <Field label={t('form.maxBodyBytes')}>
          <input type="number" min="0" placeholder={t('form.optionalZero')} value={policy.max_body_bytes ?? ''} onChange={(e) => update({ max_body_bytes: parseOptionalPositive(e.target.value) })} />
        </Field>
        <Field label={t('form.queueTimeoutMs')}>
          <input type="number" min="0" placeholder={t('form.optionalZero')} value={policy.queue_timeout_ms ?? ''} onChange={(e) => update({ queue_timeout_ms: parseOptionalPositive(e.target.value) })} />
        </Field>
      </div>
    </section>
  );
}

/* ===== Upstreams View ===== */

function UpstreamsView({ config, token, setConfig, setNotice }: DataProps) {
  const { t } = useI18n();
  const [draft, setDraft] = useState<Upstream>(newUpstream());
  const [editing, setEditing] = useState<string | null>(null);
  const [showModal, setShowModal] = useState(false);
  const [upstreamHealth, setUpstreamHealth] = useState<UpstreamHealth[]>([]);
  const [runtimeUpstreams, setRuntimeUpstreams] = useState<RuntimeUpstream[]>([]);
  const [runtimeLoading, setRuntimeLoading] = useState(false);
  const [runtimeError, setRuntimeError] = useState('');
  const [runtimeAction, setRuntimeAction] = useState<string | null>(null);
  const [weightDrafts, setWeightDrafts] = useState<Record<string, string>>({});
  const upstreams = Object.values(config.upstreams ?? {});
  const upstreamHealthByName = useMemo(() => Object.fromEntries(upstreamHealth.map((item) => [item.upstream, item])), [upstreamHealth]);
  const loadRuntime = useCallback(async () => {
    setRuntimeLoading(true);
    setRuntimeError('');
    try {
      setRuntimeUpstreams(await api<RuntimeUpstream[]>('/api/runtime/upstreams', { token }));
    } catch (error) {
      setRuntimeError(errorMessage(error));
    } finally {
      setRuntimeLoading(false);
    }
  }, [token]);

  useEffect(() => {
    let active = true;
    async function loadHealth() {
      const health = await api<UpstreamHealth[]>('/api/upstream-health', { token }).catch(() => []);
      if (active) setUpstreamHealth(health);
    }
    loadHealth();
    const timer = window.setInterval(loadHealth, 5000);
    return () => { active = false; window.clearInterval(timer); };
  }, [token]);

  useEffect(() => {
    let active = true;
    async function load() {
      setRuntimeLoading(true);
      setRuntimeError('');
      try {
        const runtime = await api<RuntimeUpstream[]>('/api/runtime/upstreams', { token });
        if (active) setRuntimeUpstreams(runtime);
      } catch (error) {
        if (active) setRuntimeError(errorMessage(error));
      } finally {
        if (active) setRuntimeLoading(false);
      }
    }
    load();
    const timer = window.setInterval(load, 5000);
    return () => { active = false; window.clearInterval(timer); };
  }, [token]);

  useEffect(() => {
    setWeightDrafts((current) => {
      const next: Record<string, string> = {};
      runtimeUpstreams.forEach((upstream) => upstream.targets.forEach((target) => {
        const key = runtimeTargetKey(upstream.name, target.url);
        next[key] = current[key] ?? String(target.effective_weight);
      }));
      return next;
    });
  }, [runtimeUpstreams]);

  function openCreate() { setEditing(null); setDraft(newUpstream()); setShowModal(true); }
  function openEdit(u: Upstream) { setEditing(u.name); setDraft(normalizeUpstream(structuredClone(u))); setShowModal(true); }

  async function submit(event: FormEvent) {
    event.preventDefault();
    const body = {
      ...draft,
      balance: draft.balance ?? 'weighted_round_robin',
      retry: normalizeRetryPolicy(draft.retry),
      health_check: normalizeHealthCheck(draft.health_check),
    };
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

  async function handleReload() {
    await Promise.all([refreshConfig(token, setConfig, setNotice), loadRuntime()]);
    setNotice({ type: 'success', message: t('notice.configReloaded') });
  }

  async function applyRuntimeMode(upstream: string, target: RuntimeTarget, mode: RuntimeTargetMode) {
    const actionKey = runtimeActionKey(upstream, target.url, mode);
    setRuntimeAction(actionKey);
    try {
      await runtimeTargetOperation(token, upstream, mode, target.url);
      await loadRuntime();
      setNotice({ type: 'success', message: t('notice.runtimeUpdated') });
    } catch (error) {
      setNotice({ type: 'error', message: errorMessage(error) });
    } finally {
      setRuntimeAction(null);
    }
  }

  async function applyRuntimeWeight(upstream: string, target: RuntimeTarget) {
    const key = runtimeTargetKey(upstream, target.url);
    const weight = clampInt(weightDrafts[key] ?? String(target.effective_weight), 0, 1_000_000, target.effective_weight);
    const actionKey = runtimeActionKey(upstream, target.url, 'weight');
    setRuntimeAction(actionKey);
    try {
      await runtimeTargetWeight(token, upstream, target.url, weight);
      await loadRuntime();
      setNotice({ type: 'success', message: t('notice.runtimeUpdated') });
    } catch (error) {
      setNotice({ type: 'error', message: errorMessage(error) });
    } finally {
      setRuntimeAction(null);
    }
  }

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
              <td><HealthCheckSummary upstream={u} health={upstreamHealthByName[u.name]} /></td>
              <td className="td-actions">
                <button className="btn btn-ghost btn-sm" onClick={() => openEdit(u)}>{t('action.edit')}</button>
                <button className="btn btn-danger btn-sm" onClick={() => remove(u.name)}>{t('action.del')}</button>
              </td>
            </tr>
          ))}
        </tbody></table>
      </div></div>
      <RuntimeTargetsPanel
        runtimeUpstreams={runtimeUpstreams}
        loading={runtimeLoading}
        error={runtimeError}
        action={runtimeAction}
        weightDrafts={weightDrafts}
        setWeightDrafts={setWeightDrafts}
        onMode={applyRuntimeMode}
        onWeight={applyRuntimeWeight}
      />
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
              <h3 className="form-section-title">{t('form.balance')}</h3>
              <div className="form-grid">
                <Field label={t('form.algorithm')}>
                  <Dropdown
                    value={draft.balance ?? 'weighted_round_robin'}
                    options={enumOptions(BALANCE_ALGORITHMS)}
                    onChange={(balance) => setDraft({ ...draft, balance: balance as BalanceAlgorithm })}
                  />
                </Field>
                <Field label={t('form.retryAttempts')}>
                  <input type="number" min="0" value={draft.retry?.attempts ?? 0} onChange={(e) => setDraft({ ...draft, retry: normalizeRetryPolicy({ ...(draft.retry ?? defaultRetryPolicy), attempts: clampInt(e.target.value, 0, 100, 0) }) })} />
                </Field>
              </div>
              <Field label={t('form.retryStatus')}>
                <input
                  value={(draft.retry?.retry_on_status ?? []).join(', ')}
                  placeholder="502,503,504"
                  onChange={(e) => setDraft({ ...draft, retry: normalizeRetryPolicy({ ...(draft.retry ?? defaultRetryPolicy), retry_on_status: parseStatusList(e.target.value) }) })}
                />
              </Field>
              <p className="card-desc">{t('form.retryStatusHint')}</p>
              <div className="form-grid">
                <label className="toggle-row">
                  <input
                    type="checkbox"
                    checked={Boolean(draft.retry?.retry_on_timeout)}
                    onChange={(e) => setDraft({ ...draft, retry: normalizeRetryPolicy({ ...(draft.retry ?? defaultRetryPolicy), retry_on_timeout: e.target.checked }) })}
                  />
                  <span>{t('form.retryTimeout')}</span>
                </label>
                <label className="toggle-row">
                  <input
                    type="checkbox"
                    checked={Boolean(draft.retry?.retry_on_connect_error)}
                    onChange={(e) => setDraft({ ...draft, retry: normalizeRetryPolicy({ ...(draft.retry ?? defaultRetryPolicy), retry_on_connect_error: e.target.checked }) })}
                  />
                  <span>{t('form.retryConnect')}</span>
                </label>
              </div>
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

function RuntimeTargetsPanel({
  runtimeUpstreams,
  loading,
  error,
  action,
  weightDrafts,
  setWeightDrafts,
  onMode,
  onWeight,
}: {
  runtimeUpstreams: RuntimeUpstream[];
  loading: boolean;
  error: string;
  action: string | null;
  weightDrafts: Record<string, string>;
  setWeightDrafts: (drafts: Record<string, string> | ((current: Record<string, string>) => Record<string, string>)) => void;
  onMode: (upstream: string, target: RuntimeTarget, mode: RuntimeTargetMode) => void;
  onWeight: (upstream: string, target: RuntimeTarget) => void;
}) {
  const { t } = useI18n();
  const rows = runtimeUpstreams.flatMap((upstream) => upstream.targets.map((target) => ({ upstream: upstream.name, target })));
  return (
    <div className="table-card runtime-table-card">
      <div className="runtime-panel-head">
        <div>
          <h3 className="card-title-sm">{t('runtime.title')}</h3>
          <p className="card-desc">{t('runtime.sub')}</p>
        </div>
        <span className={loading ? 'runtime-refresh is-loading' : 'runtime-refresh'}>
          <Icon name="sync" size={15} />{loading ? t('notice.loading') : t('metric.live')}
        </span>
      </div>
      {error && <div className="runtime-error"><Icon name="error" size={16} />{error}</div>}
      <div className="table-wrap runtime-table-wrap">
        <table><thead><tr>
          <th>{t('table.upstream')}</th>
          <th>{t('table.targets')}</th>
          <th>{t('table.mode')}</th>
          <th>{t('table.health')}</th>
          <th>{t('table.active')}</th>
          <th>{t('table.configuredWeight')}</th>
          <th>{t('table.effectiveWeight')}</th>
          <th>{t('table.overrideWeight')}</th>
          <th>{t('table.lastError')}</th>
          <th>{t('table.actions')}</th>
        </tr></thead><tbody>
          {rows.length === 0 ? (
            <tr><td colSpan={10} style={{ textAlign: 'center', color: 'var(--muted-foreground)', padding: 32 }}>{t('runtime.empty')}</td></tr>
          ) : rows.map(({ upstream, target }) => {
            const key = runtimeTargetKey(upstream, target.url);
            const isPending = Boolean(action?.startsWith(`${key}\u0000`));
            return (
              <tr key={key}>
                <td className="td-upstream">{upstream}</td>
                <td className="td-mono runtime-target-url" title={target.url}>{target.url}</td>
                <td><span className={`runtime-mode-badge mode-${target.mode}`}>{runtimeModeLabel(target.mode, t)}</span></td>
                <td><span className={target.healthy ? 'runtime-health is-healthy' : 'runtime-health is-unhealthy'}><span />{target.healthy ? t('runtime.healthy') : t('runtime.unhealthy')}</span></td>
                <td className="td-mono">{formatNumber(target.active_connections)}</td>
                <td className="td-mono">{formatNumber(target.configured_weight)}</td>
                <td className="td-mono">{formatNumber(target.effective_weight)}</td>
                <td>
                  <div className="runtime-weight-control">
                    <input
                      type="number"
                      min="0"
                      value={weightDrafts[key] ?? String(target.effective_weight)}
                      onChange={(event) => setWeightDrafts((current) => ({ ...current, [key]: event.target.value }))}
                      aria-label={`${t('table.overrideWeight')} ${target.url}`}
                    />
                    <button className="btn btn-secondary btn-sm" type="button" disabled={isPending} onClick={() => onWeight(upstream, target)} title={t('action.apply')}>
                      <Icon name="check" size={15} />
                    </button>
                  </div>
                  <span className="runtime-override-note">{target.weight_override === null ? t('runtime.overrideNone') : formatNumber(target.weight_override)}</span>
                </td>
                <td className="td-mono runtime-last-error" title={target.last_error ?? ''}>{target.last_error ?? '—'}</td>
                <td>
                  <div className="runtime-actions">
                    <button className="btn btn-ghost btn-sm" type="button" disabled={isPending || target.mode === 'enabled'} onClick={() => onMode(upstream, target, 'enabled')} title={t('action.enable')}>
                      <Icon name="play_arrow" size={15} /><span>{t('action.enable')}</span>
                    </button>
                    <button className="btn btn-ghost btn-sm" type="button" disabled={isPending || target.mode === 'drain'} onClick={() => onMode(upstream, target, 'drain')} title={t('action.drain')}>
                      <Icon name="pause_circle" size={15} /><span>{t('action.drain')}</span>
                    </button>
                    <button className="btn btn-danger btn-sm" type="button" disabled={isPending || target.mode === 'disabled'} onClick={() => onMode(upstream, target, 'disabled')} title={t('action.disable')}>
                      <Icon name="block" size={15} /><span>{t('action.disable')}</span>
                    </button>
                  </div>
                </td>
              </tr>
            );
          })}
        </tbody></table>
      </div>
    </div>
  );
}

/* ===== Health Check Components ===== */

function HealthCheckSummary({ upstream, health }: { upstream: Upstream; health?: UpstreamHealth }) {
  const { t } = useI18n();
  const check = normalizeHealthCheck(upstream.health_check);
  if (!check.enabled) return <span style={{ color: 'var(--muted-foreground)' }}>{t('table.off')}</span>;
  const visual = flowHealthVisual(health);
  return (
    <div className={`health-summary ${visual.className}`} style={{ '--flow-color': visual.color } as CSSProperties}>
      <span className="td-badge">{check.mode.toUpperCase()}</span>
      <span className="health-summary-text">{check.mode === 'http' ? `${check.path} -> ${check.expected_status}` : t('health.hostPort')}</span>
      <span className="health-summary-muted">{check.interval_seconds}s / {check.timeout_seconds}s</span>
      <span className="health-result"><span className="health-dot" />{healthSummaryText(health, t, upstream)}</span>
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
      setConfig(normalizeConfig(next));
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
  const [text, setText] = useState(() => dumpConfigYaml(config));
  useEffect(() => setText(dumpConfigYaml(config)), [config]);
  const lineCount = text.split('\n').length;
  const parsedConfig = useMemo(() => {
    try { return yaml.load(text) as AppConfig; } catch { return null; }
  }, [text]);
  const globalConfig = parsedConfig ?? config;

  async function submit() {
    try {
      const parsed = yaml.load(text) as AppConfig;
      await api<AppConfig>('/api/config', { method: 'PUT', token, body: parsed });
      setConfig(normalizeConfig(parsed));
      setNotice({ type: 'success', message: t('notice.configSaved') });
    } catch (e) { setNotice({ type: 'error', message: errorMessage(e) }); }
  }

  const isValid = useMemo(() => { try { yaml.load(text); return true; } catch { return false; } }, [text]);
  function updateGlobal(patch: Partial<AppConfig>) {
    const next = { ...(parsedConfig ?? config), ...patch };
    setText(dumpConfigYaml(next));
  }

  function updateFallbackUrl(url: string) {
    const current = parsedConfig ?? config;
    setText(dumpConfigYaml({ ...current, fallback: { ...(current.fallback ?? { url: '' }), url } }));
  }

  function updateAccessLog(patch: NonNullable<AppConfig['access_log']>) {
    const current = parsedConfig ?? config;
    setText(dumpConfigYaml({ ...current, access_log: { enabled: false, path: null, buffer_size: 8192, level: 'info', ...(current.access_log ?? {}), ...patch } }));
  }

  function updateMonitoring(patch: NonNullable<AppConfig['monitoring']>) {
    const current = parsedConfig ?? config;
    const currentMonitoring = current.monitoring ?? emptyConfig.monitoring!;
    setText(dumpConfigYaml({ ...current, monitoring: { ...currentMonitoring, ...patch } }));
  }

  function updatePrometheus(patch: NonNullable<NonNullable<AppConfig['monitoring']>['prometheus']>) {
    const current = parsedConfig ?? config;
    const currentMonitoring = current.monitoring ?? emptyConfig.monitoring!;
    const prometheus = currentMonitoring.prometheus ?? emptyConfig.monitoring!.prometheus!;
    setText(dumpConfigYaml({ ...current, monitoring: { ...currentMonitoring, prometheus: { ...prometheus, ...patch } } }));
  }

  function updatePrometheusAuth(patch: NonNullable<NonNullable<NonNullable<AppConfig['monitoring']>['prometheus']>['auth']>) {
    const current = parsedConfig ?? config;
    const currentMonitoring = current.monitoring ?? emptyConfig.monitoring!;
    const prometheus = currentMonitoring.prometheus ?? emptyConfig.monitoring!.prometheus!;
    const auth = prometheus.auth ?? { auth_type: 'none' };
    setText(dumpConfigYaml({ ...current, monitoring: { ...currentMonitoring, prometheus: { ...prometheus, auth: { ...auth, ...patch } } } }));
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
            <Field label={t('config.listen')}><input value={globalConfig.listen ?? '0.0.0.0:3000'} onChange={(e) => updateGlobal({ listen: e.target.value })} /></Field>
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
          <div className="card global-config-card">
            <h3 className="card-title-sm">{t('config.accessLog')}</h3>
            <label className="checkbox-row">
              <input type="checkbox" checked={Boolean(globalConfig.access_log?.enabled)} onChange={(e) => updateAccessLog({ enabled: e.target.checked })} />
              <span>{t('config.accessLogEnabled')}</span>
            </label>
            <Field label={t('config.accessLogPath')}>
              <input value={globalConfig.access_log?.path ?? ''} placeholder={t('config.accessLogPathHint')} onChange={(e) => updateAccessLog({ path: e.target.value.trim() ? e.target.value : null })} />
            </Field>
            <Field label={t('config.accessLogLevel')}>
              <Dropdown value={globalConfig.access_log?.level ?? 'info'} options={ACCESS_LOG_LEVEL_OPTIONS} onChange={(level) => updateAccessLog({ level: level as AccessLogLevel })} />
            </Field>
            <Field label={t('config.accessLogBuffer')}><input type="number" min="1" value={globalConfig.access_log?.buffer_size ?? 8192} onChange={(e) => updateAccessLog({ buffer_size: Number(e.target.value) })} /></Field>
          </div>
          <div className="card global-config-card">
            <h3 className="card-title-sm">{t('config.monitoring')}</h3>
            <label className="checkbox-row">
              <input type="checkbox" checked={Boolean(globalConfig.monitoring?.enabled)} onChange={(e) => updateMonitoring({ enabled: e.target.checked })} />
              <span>{t('config.monitoringEnabled')}</span>
            </label>
            <Field label={t('config.prometheusUrl')}><input value={globalConfig.monitoring?.prometheus?.url ?? ''} placeholder="http://127.0.0.1:9090" onChange={(e) => updatePrometheus({ url: e.target.value })} /></Field>
            <Field label={t('config.authType')}>
              <Dropdown
                value={globalConfig.monitoring?.prometheus?.auth?.auth_type ?? 'none'}
                options={['none', 'basic', 'bearer', 'header'].map((value) => ({ value, label: value }))}
                onChange={(auth_type) => updatePrometheusAuth({ auth_type })}
              />
            </Field>
            {globalConfig.monitoring?.prometheus?.auth?.auth_type === 'basic' && <>
              <Field label={t('config.username')}><input value={globalConfig.monitoring?.prometheus?.auth?.username ?? ''} onChange={(e) => updatePrometheusAuth({ username: e.target.value })} /></Field>
              <Field label={t('config.password')}><input type="password" value={globalConfig.monitoring?.prometheus?.auth?.password ?? ''} onChange={(e) => updatePrometheusAuth({ password: e.target.value })} /></Field>
            </>}
            {globalConfig.monitoring?.prometheus?.auth?.auth_type === 'bearer' && <Field label={t('config.bearerToken')}><input type="password" value={globalConfig.monitoring?.prometheus?.auth?.bearer_token ?? ''} onChange={(e) => updatePrometheusAuth({ bearer_token: e.target.value })} /></Field>}
            {globalConfig.monitoring?.prometheus?.auth?.auth_type === 'header' && <>
              <Field label={t('config.headerName')}><input value={globalConfig.monitoring?.prometheus?.auth?.header_name ?? ''} onChange={(e) => updatePrometheusAuth({ header_name: e.target.value })} /></Field>
              <Field label={t('config.headerValue')}><input type="password" value={globalConfig.monitoring?.prometheus?.auth?.header_value ?? ''} onChange={(e) => updatePrometheusAuth({ header_value: e.target.value })} /></Field>
            </>}
          </div>
          <div className="card">
            <h3 className="card-title-sm">{t('config.schema')}</h3>
            <div className="schema-entry"><span className="schema-key" style={{ color: '#C792EA' }}>global</span><span className="schema-val">listen, proxy_listen, certificate_dir, access_log.level, monitoring.prometheus, certificates[].cert/key path, fallback, connect_timeout, request_timeout, pool_max_idle_per_host, pool_idle_timeout, tcp_keepalive</span></div>
            <div style={{ height: 1, background: 'var(--border)' }} />
            <div className="schema-entry"><span className="schema-key" style={{ color: '#82AAFF' }}>upstreams.&lt;name&gt;</span><span className="schema-val">balance, retry, skip_ssl, websocket, targets[].url, targets[].weight, health_check</span></div>
            <div style={{ height: 1, background: 'var(--border)' }} />
            <div className="schema-entry"><span className="schema-key" style={{ color: '#C792EA' }}>match_sets[]</span><span className="schema-val">name, conditions(header/cookie/jwt)</span></div>
            <div className="schema-entry"><span className="schema-key" style={{ color: '#FFCB6B' }}>routes[]</span><span className="schema-val">id, listen, request_timeout, header_policy, path_actions, limit_policy, host, location, priority, tls, match_set, conditions, upstream</span></div>
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
  if (response.status === 401 && options.token) {
    window.dispatchEvent(new Event(AUTH_EXPIRED_EVENT));
  }
  if (!response.ok || payload?.success === false) throw new Error(payload?.error ?? `HTTP ${response.status}`);
  return payload?.data as T;
}

async function refreshConfig(token: string, setConfig: (c: AppConfig) => void, setNotice: (n: Notice) => void) {
  try { setConfig(normalizeConfig(await api<AppConfig>('/api/config', { token }))); } catch (error) { setNotice({ type: 'error', message: errorMessage(error) }); }
}

async function prometheusRange(token: string, query: string, start: number, end: number, step: string): Promise<PrometheusRangeResult> {
  const params = new URLSearchParams({ query, start: String(start), end: String(end), step });
  return api<PrometheusRangeResult>(`/api/monitoring/query-range?${params}`, { token });
}

async function runtimeTargetOperation(token: string, upstream: string, mode: RuntimeTargetMode, target: string): Promise<RuntimeTarget> {
  const action = mode === 'enabled' ? 'enable' : mode === 'disabled' ? 'disable' : 'drain';
  return api<RuntimeTarget>(`/api/runtime/upstreams/${encodeURIComponent(upstream)}/targets/${action}`, {
    method: 'POST',
    token,
    body: { target },
  });
}

async function runtimeTargetWeight(token: string, upstream: string, target: string, weight: number): Promise<RuntimeTarget> {
  return api<RuntimeTarget>(`/api/runtime/upstreams/${encodeURIComponent(upstream)}/targets/weight`, {
    method: 'POST',
    token,
    body: { target, weight },
  });
}

function firstSeries(result: PrometheusRangeResult): ChartPoint[] {
  return result.data?.result?.[0]?.values?.map(([t, v]) => ({ t, v: Number(v) })).filter((point) => Number.isFinite(point.v)) ?? [];
}

function runtimeTargetKey(upstream: string, target: string): string {
  return `${upstream}\u0000${target}`;
}

function runtimeActionKey(upstream: string, target: string, action: string): string {
  return `${runtimeTargetKey(upstream, target)}\u0000${action}`;
}

function runtimeModeLabel(mode: RuntimeTargetMode, t: (key: string) => string): string {
  if (mode === 'disabled') return t('runtime.disabled');
  if (mode === 'drain') return t('runtime.drain');
  return t('runtime.enabled');
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

function monitoringEnabled(config: AppConfig): boolean {
  return Boolean(config.monitoring?.enabled && config.monitoring?.prometheus?.url?.trim());
}

function uniqueListeners(config: AppConfig): string[] {
  const listeners = new Set<string>();
  listeners.add(config.proxy_listen || '0.0.0.0:80');
  config.rules.forEach((rule) => listeners.add(rule.listen || config.proxy_listen || '0.0.0.0:80'));
  return [...listeners];
}

function routeKey(rule: Rule): string {
  return `${rule.name || rule.id}\u0000${rule.upstream}`;
}

function monitoringLabelSelector(entry: string, rule?: Rule): string {
  const parts = [`listen="${escapePromLabel(entry)}"`];
  if (rule) {
    parts.push(`rule="${escapePromLabel(rule.name || rule.id)}"`);
    parts.push(`upstream="${escapePromLabel(rule.upstream)}"`);
  }
  return parts.join(',');
}

function escapePromLabel(value: string): string {
  return value.replace(/\\/g, '\\\\').replace(/"/g, '\\"').replace(/\n/g, '\\n');
}

function sparklineChart(points: ChartPoint[], width: number, height: number): { path: string; points: { x: number; y: number }[] } {
  if (points.length === 0) return { path: '', points: [] };
  const values = points.map((p) => p.v);
  const min = Math.min(...values);
  const max = Math.max(...values);
  const span = max - min || 1;
  const coordinates = points.map((point, index) => {
    const x = points.length === 1 ? width : (index / (points.length - 1)) * width;
    const y = height - ((point.v - min) / span) * (height - 20) - 10;
    return { x, y };
  });
  return {
    path: coordinates.map((point, index) => `${index === 0 ? 'M' : 'L'}${point.x.toFixed(1)} ${point.y.toFixed(1)}`).join(' '),
    points: coordinates,
  };
}

function formatNumber(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(2) + 'M';
  if (n >= 1_000) return n.toLocaleString();
  return String(n);
}

function formatLatency(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return '—';
  if (ms < 1) return `${ms.toFixed(2)} ms`;
  if (ms < 100) return `${ms.toFixed(1)} ms`;
  return `${Math.round(ms)} ms`;
}

function flowHealthVisual(health?: UpstreamHealth): { className: string; color: string } {
  if (!health?.enabled || health.total === 0 || health.unhealthy === 0) {
    return { className: 'flow-health-ok', color: '#0B7A3B' };
  }
  if (health.unhealthy >= health.total) {
    return { className: 'flow-health-down', color: '#B42318' };
  }
  return { className: 'flow-health-warn', color: '#C76B00' };
}

function healthSummaryText(health: UpstreamHealth | undefined, t: (key: string) => string, upstream?: Upstream): string {
  const checkEnabled = upstream ? normalizeHealthCheck(upstream.health_check).enabled : health?.enabled;
  if (!checkEnabled) return t('table.off');
  if (!health) {
    const total = upstream?.targets.length ?? 0;
    return `${t('table.healthTotal')} ${total}, ${t('table.healthUnhealthy')} ${t('table.healthUnknown')}`;
  }
  return `${t('table.healthTotal')} ${health.total}, ${t('table.healthUnhealthy')} ${health.unhealthy}`;
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
  return {
    id: '',
    name: '',
    priority: 10,
    host: { type: 'any', value: null },
    location: { type: 'prefix', value: '/' },
    match_set: null,
    upstream,
    weight: 100,
    is_fallback: false,
    listen,
    request_timeout: 0,
    conditions: createLeafCondition(),
    header_policy: { ...defaultHeaderPolicy, request: [], response: [] },
    path_actions: [],
    limit_policy: { ...defaultLimitPolicy },
  };
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
  return {
    name: '',
    skip_ssl: false,
    websocket: false,
    balance: 'weighted_round_robin',
    retry: { ...defaultRetryPolicy, retry_on_status: [] },
    targets: [{ url: 'http://127.0.0.1:8080', weight: 100 }],
    health_check: { ...defaultHealthCheck },
  };
}

function normalizeConfig(config: AppConfig): AppConfig {
  return {
    ...config,
    rules: (config.rules ?? []).map(normalizeRule),
    upstreams: Object.fromEntries(
      Object.entries(config.upstreams ?? {}).map(([name, upstream]) => [name, normalizeUpstream(upstream)])
    ),
  };
}

function normalizeRule(rule: Rule): Rule {
  return {
    ...rule,
    host: normalizeHostMatcher(rule.host),
    location: normalizeLocationMatcher(rule.location),
    conditions: rule.match_set ? null : normalizeCondition(rule.conditions),
    request_timeout: Number(rule.request_timeout ?? 0),
    header_policy: normalizeHeaderPolicy(rule.header_policy),
    path_actions: normalizePathActions(rule.path_actions),
    limit_policy: normalizeLimitPolicy(rule.limit_policy),
  };
}

function normalizeUpstream(upstream: Upstream): Upstream {
  return {
    ...upstream,
    balance: BALANCE_ALGORITHMS.includes(upstream.balance as BalanceAlgorithm) ? upstream.balance : 'weighted_round_robin',
    retry: normalizeRetryPolicy(upstream.retry),
    health_check: normalizeHealthCheck(upstream.health_check),
    targets: (upstream.targets ?? []).map((target) => ({ url: target.url, weight: Number(target.weight ?? 0) })),
  };
}

function normalizeHeaderPolicy(policy?: HeaderPolicy): HeaderPolicy {
  return {
    request: (policy?.request ?? []).map(normalizeHeaderMutation),
    response: (policy?.response ?? []).map(normalizeHeaderMutation),
  };
}

function normalizeHeaderMutation(mutation: HeaderMutation): HeaderMutation {
  const op = HEADER_MUTATION_OPS.includes(mutation.op) ? mutation.op : 'set';
  return {
    op,
    name: mutation.name ?? '',
    value: op === 'remove' ? null : mutation.value ?? '',
  };
}

function normalizePathActions(actions?: PathAction[]): PathAction[] {
  return (actions ?? []).map((action) => createPathAction(pathActionType(action), action));
}

function normalizeLimitPolicy(policy?: LimitPolicy): LimitPolicy {
  return {
    rate_per_second: nullablePositiveNumber(policy?.rate_per_second),
    rate_key: RATE_LIMIT_KEYS.includes(policy?.rate_key as RateLimitKey) ? policy!.rate_key : 'ip',
    max_connections: nullablePositiveNumber(policy?.max_connections),
    max_body_bytes: nullablePositiveNumber(policy?.max_body_bytes),
    queue_timeout_ms: nullablePositiveNumber(policy?.queue_timeout_ms),
  };
}

function normalizeRetryPolicy(policy?: RetryPolicy): RetryPolicy {
  return {
    attempts: Math.max(0, Number(policy?.attempts ?? 0)),
    retry_on_status: (policy?.retry_on_status ?? [])
      .map((status) => Number(status))
      .filter((status) => Number.isInteger(status) && status >= 100 && status <= 599),
    retry_on_timeout: Boolean(policy?.retry_on_timeout),
    retry_on_connect_error: Boolean(policy?.retry_on_connect_error),
  };
}

function nullablePositiveNumber(value: unknown): number | null {
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
}

function parseOptionalPositive(value: string): number | null {
  return nullablePositiveNumber(value.trim() === '' ? null : Number(value));
}

function parseStatusList(value: string): number[] {
  return value
    .split(',')
    .map((item) => Number(item.trim()))
    .filter((status) => Number.isInteger(status) && status >= 100 && status <= 599);
}

function createHeaderMutation(): HeaderMutation {
  return { op: 'set', name: '', value: '' };
}

function createPathAction(type: PathActionType = 'strip_prefix', previous?: PathAction): PathAction {
  if (type === 'rewrite') {
    const rewrite = 'rewrite' in (previous ?? {}) ? (previous as Extract<PathAction, { rewrite: unknown }>).rewrite : undefined;
    return { rewrite: { pattern: rewrite?.pattern ?? '^/old', replacement: rewrite?.replacement ?? '/new' } };
  }
  if (type === 'redirect') {
    const redirect = 'redirect' in (previous ?? {}) ? (previous as Extract<PathAction, { redirect: unknown }>).redirect : undefined;
    return { redirect: { status: Number(redirect?.status ?? 301), location: redirect?.location ?? 'https://example.com' } };
  }
  const strip = 'strip_prefix' in (previous ?? {}) ? (previous as Extract<PathAction, { strip_prefix: unknown }>).strip_prefix : undefined;
  return { strip_prefix: { prefix: strip?.prefix ?? '/api' } };
}

function pathActionType(action: PathAction): PathActionType {
  if ('rewrite' in action) return 'rewrite';
  if ('redirect' in action) return 'redirect';
  return 'strip_prefix';
}

function humanizeSnake(value: string): string {
  return value
    .split('_')
    .map((part) => part.length <= 3 ? part.toUpperCase() : part.charAt(0).toUpperCase() + part.slice(1))
    .join(' ');
}

function enumOptions<T extends string>(values: readonly T[]): DropdownOption[] {
  return values.map((value) => ({ value, label: humanizeSnake(value) }));
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
  return (['header', 'cookie', 'jwt'] as const).map((value) => ({
    value,
    label: value,
    description: t(`form.type.${value}`),
  }));
}

function hostTypeOptions(t: (key: string) => string): DropdownOption[] {
  return (['any', 'exact', 'wildcard'] as const).map((value) => ({ value, label: t(`form.host.${value}`) }));
}

function locationTypeOptions(t: (key: string) => string): DropdownOption[] {
  return (['prefix', 'exact', 'regex'] as const).map((value) => ({ value, label: t(`form.location.${value}`) }));
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
  return { type: 'leaf', conditionType: 'header', key: '', claimPath: null, operator: 'exact', value: '' };
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

function normalizeHostMatcher(host?: HostMatcher | null): HostMatcher {
  const type = host?.type === 'exact' || host?.type === 'wildcard' ? host.type : 'any';
  return type === 'any' ? { type, value: null } : { type, value: host?.value?.trim() || '' };
}

function normalizeLocationMatcher(location?: LocationMatcher | null): LocationMatcher {
  const type = location?.type === 'exact' || location?.type === 'regex' ? location.type : 'prefix';
  let value = location?.value?.trim() || '/';
  if (type !== 'regex' && !value.startsWith('/')) value = `/${value}`;
  return { type, value };
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

function summarizeHost(host?: HostMatcher | null, t?: (key: string) => string): string {
  const normalized = normalizeHostMatcher(host);
  if (normalized.type === 'any') return t ? t('form.host.any') : '*';
  return `${normalized.type}:${normalized.value || '-'}`;
}

function summarizeLocation(location?: LocationMatcher | null): string {
  const normalized = normalizeLocationMatcher(location);
  return `${normalized.type}:${normalized.value}`;
}

function resolveInitialView(): View {
  const segment = window.location.pathname.replace('/admin', '').split('/').filter(Boolean)[0];
  return NAV_ITEMS.find((item) => item.id === segment)?.id ?? 'operations';
}

function readStoredLang(): Lang {
  const stored = localStorage.getItem('rustproxy_lang');
  return stored === 'en' || stored === 'zh' ? stored : DEFAULT_LANG;
}

function readStoredTheme(): Theme {
  const stored = localStorage.getItem('rustproxy_theme');
  return stored === 'dark' || stored === 'light' ? stored : DEFAULT_THEME;
}

function errorMessage(error: unknown) { return error instanceof Error ? error.message : 'request failed'; }

export default App;
