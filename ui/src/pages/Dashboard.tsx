import { Link } from 'react-router-dom';

function Dashboard() {
  return (
    <section>
      <h2>Dashboard</h2>
      <p>Monitor RustProxy health, traffic, and routing activity from one place.</p>
      <p>
        Start by reviewing <Link to="/rules">routing rules</Link> or checking configured{' '}
        <Link to="/upstreams">upstreams</Link>.
      </p>
    </section>
  );
}

export default Dashboard;
