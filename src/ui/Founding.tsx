import { useState } from 'preact/hooks';
import { ui } from './store';

/** Over a new world while it is being founded: how many villagers have arrived so far. */
export function FoundingDialog() {
  const founding = ui.founding.value;
  const [dismissed, setDismissed] = useState(false);
  if (!founding || dismissed) return null;
  const percent = founding.total > 0 ? Math.round((founding.settled / founding.total) * 100) : 0;
  const who = ui.llm.value.state === 'off' ? 'The village is being settled.' : 'The language model is writing each of them.';
  return (
    <div class="modal">
      <section class="panel dialog small" role="dialog" aria-label="A village is being founded">
        <h2>A village is being founded</h2>
        <p>
          {founding.settled} of {founding.total} villagers have arrived. {who}
        </p>
        <div class="progress" role="progressbar" aria-valuenow={percent} aria-valuemin={0} aria-valuemax={100}>
          <div class="progress-bar" style={{ width: `${percent}%` }} />
        </div>
        <p class="muted small-text">The first villagers are choosing their homes. You can look around meanwhile; the dead stay buried until everyone is here.</p>
        <footer>
          <button class="primary" onClick={() => setDismissed(true)}>Look around</button>
        </footer>
      </section>
    </div>
  );
}
