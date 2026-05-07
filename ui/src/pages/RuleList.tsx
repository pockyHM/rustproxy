import { Link } from 'react-router-dom';

function RuleList() {
  return (
    <section>
      <h2>Rules</h2>
      <p>View and manage traffic routing rules for RustProxy.</p>
      <p>
        <Link to="/rules/new">Create a new rule</Link> or open a rule by ID at{' '}
        <Link to="/rules/example-rule">/rules/example-rule</Link>.
      </p>
    </section>
  );
}

export default RuleList;
