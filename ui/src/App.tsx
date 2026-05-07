import { BrowserRouter, Link, Route, Routes } from 'react-router-dom';
import ConfigEditor from './pages/ConfigEditor';
import Dashboard from './pages/Dashboard';
import RuleEditor from './pages/RuleEditor';
import RuleList from './pages/RuleList';
import UpstreamEditor from './pages/UpstreamEditor';
import UpstreamList from './pages/UpstreamList';

const navLinks = [
  { to: '/', label: 'Dashboard' },
  { to: '/rules', label: 'Rules' },
  { to: '/rules/new', label: 'New Rule' },
  { to: '/upstreams', label: 'Upstreams' },
  { to: '/upstreams/new', label: 'New Upstream' },
  { to: '/config', label: 'Config' },
];

function App() {
  return (
    <BrowserRouter>
      <div style={{ fontFamily: 'system-ui, sans-serif', margin: '2rem' }}>
        <header>
          <h1>RustProxy Admin</h1>
          <nav style={{ display: 'flex', gap: '1rem', marginBottom: '2rem' }}>
            {navLinks.map((link) => (
              <Link key={link.to} to={link.to}>
                {link.label}
              </Link>
            ))}
          </nav>
        </header>

        <main>
          <Routes>
            <Route path="/" element={<Dashboard />} />
            <Route path="/rules" element={<RuleList />} />
            <Route path="/rules/new" element={<RuleEditor />} />
            <Route path="/rules/:id" element={<RuleEditor />} />
            <Route path="/upstreams" element={<UpstreamList />} />
            <Route path="/upstreams/new" element={<UpstreamEditor />} />
            <Route path="/upstreams/:id" element={<UpstreamEditor />} />
            <Route path="/config" element={<ConfigEditor />} />
          </Routes>
        </main>
      </div>
    </BrowserRouter>
  );
}

export default App;
