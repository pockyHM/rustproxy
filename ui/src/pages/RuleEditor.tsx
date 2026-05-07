import { Link, useParams } from 'react-router-dom';

function RuleEditor() {
  const { id } = useParams();
  const isNewRule = id === undefined;

  return (
    <section>
      <h2>{isNewRule ? 'Create Rule' : `Edit Rule: ${id}`}</h2>
      <p>{isNewRule ? 'Define a new routing rule.' : 'Update routing rule conditions and upstream targets.'}</p>
      <p>
        Return to the <Link to="/rules">rule list</Link>.
      </p>
    </section>
  );
}

export default RuleEditor;
