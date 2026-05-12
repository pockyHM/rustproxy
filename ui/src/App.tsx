import { BrowserRouter, NavLink, Route, Routes, useNavigate } from 'react-router-dom';
import { useEffect, useState } from 'react';
import { useI18n, LangToggle } from './i18n';
import { useAuth } from './auth/AuthContext';
import ProtectedRoute from './auth/ProtectedRoute';
import Login from './pages/Login';
import Setup from './pages/Setup';
import ConfigEditor from './pages/ConfigEditor';
import Dashboard from './pages/Dashboard';
import Settings from './pages/Settings';
import RuleEditor from './pages/RuleEditor';
import RuleList from './pages/RuleList';
import UpstreamEditor from './pages/UpstreamEditor';
import UpstreamList from './pages/UpstreamList';
import { getSetupStatus } from './api/client';

type NavItem = {
  to: string;
  labelKey: 'dashboard' | 'rules' | 'upstreams' | 'settings' | 'config';
  end?: boolean;
};

const navItems: NavItem[] = [
  { to: '/', labelKey: 'dashboard', end: true },
  { to: '/rules', labelKey: 'rules' },
  { to: '/upstreams', labelKey: 'upstreams' },
  { to: '/settings', labelKey: 'settings' },
  { to: '/config', labelKey: 'config' },
];

const routerBaseName = '/admin';

type SetupState = 'loading' | 'needed' | 'done';

function AppRoutes() {
  const { t } = useI18n();
  const { isAuthenticated, logout } = useAuth();
  const [setupState, setSetupState] = useState<SetupState>('loading');
  const navigate = useNavigate();

  useEffect(() => {
    getSetupStatus()
      .then((response) => {
        const data = response.data?.data ?? response.data;
        if (!data.users_exist) {
          setSetupState('needed');
          navigate('/setup', { replace: true });
        } else {
          setSetupState('done');
        }
      })
      .catch(() => {
        setSetupState('done');
      });
  }, [navigate]);

  if (setupState === 'loading') {
    return <div className="loading-state">{t.common.loading}</div>;
  }

  return (
    <>
      {isAuthenticated && (
        <header className="topbar">
          <div className="topbar__inner">
            <NavLink to="/" className="topbar__logo" end>
              {t.app.title}
              <span className="topbar__logo-tag">{t.app.tag}</span>
            </NavLink>
            <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--space-2)' }}>
              <nav className="topbar__nav">
                {navItems.map((item) => (
                  <NavLink
                    key={item.to}
                    to={item.to}
                    end={item.end}
                    className={({ isActive }) =>
                      `topbar__link ${isActive ? 'topbar__link--active' : ''}`
                    }
                  >
                    {t.nav[item.labelKey]}
                  </NavLink>
                ))}
              </nav>
              <LangToggle />
              <button
                type="button"
                className="btn-ghost btn-sm"
                onClick={logout}
                style={{ marginLeft: 'var(--space-2)' }}
              >
                {t.auth.logout}
              </button>
            </div>
          </div>
        </header>
      )}
      <main className={isAuthenticated ? 'content' : ''}>
        <Routes>
          <Route path="/setup" element={<Setup />} />
          <Route path="/login" element={<Login />} />
          <Route
            path="/"
            element={
              <ProtectedRoute>
                <Dashboard />
              </ProtectedRoute>
            }
          />
          <Route
            path="/rules"
            element={
              <ProtectedRoute>
                <RuleList />
              </ProtectedRoute>
            }
          />
          <Route
            path="/rules/new"
            element={
              <ProtectedRoute>
                <RuleEditor />
              </ProtectedRoute>
            }
          />
          <Route
            path="/rules/:id"
            element={
              <ProtectedRoute>
                <RuleEditor />
              </ProtectedRoute>
            }
          />
          <Route
            path="/upstreams"
            element={
              <ProtectedRoute>
                <UpstreamList />
              </ProtectedRoute>
            }
          />
          <Route
            path="/upstreams/new"
            element={
              <ProtectedRoute>
                <UpstreamEditor />
              </ProtectedRoute>
            }
          />
          <Route
            path="/upstreams/:id"
            element={
              <ProtectedRoute>
                <UpstreamEditor />
              </ProtectedRoute>
            }
          />
          <Route
            path="/settings"
            element={
              <ProtectedRoute>
                <Settings />
              </ProtectedRoute>
            }
          />
          <Route
            path="/config"
            element={
              <ProtectedRoute>
                <ConfigEditor />
              </ProtectedRoute>
            }
          />
        </Routes>
      </main>
    </>
  );
}

function App() {
  return (
    <BrowserRouter basename={routerBaseName}>
      <AppRoutes />
    </BrowserRouter>
  );
}

export default App;
