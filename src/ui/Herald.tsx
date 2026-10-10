import { useEffect, useState } from 'preact/hooks';
import type { HeraldIssue } from '../generated/api';
import { api } from '../net/api';
import { togglePanel, ui } from './store';

/** The Valley Herald: the latest front page, and the earlier ones kept with the world. */
export function HeraldPanel() {
  const latest = ui.herald.value;
  const [issues, setIssues] = useState<HeraldIssue[]>(latest ? [latest] : []);
  const [at, setAt] = useState(0);
  useEffect(() => {
    let live = true;
    api
      .herald()
      .then((archive) => {
        if (live && archive.issues.length > 0) setIssues(archive.issues);
      })
      .catch(() => {
        // the latest issue is still there to read
      });
    return () => {
      live = false;
    };
  }, [latest?.day]);
  const issue = issues[at];
  const close = (): void => {
    ui.panel.value = 'none';
    ui.heraldAway.value = false;
  };
  return (
    <div class="modal" onClick={close}>
      <section class="panel dialog herald" onClick={(event) => event.stopPropagation()}>
        {ui.heraldAway.value && at === 0 && <p class="muted herald-away">While you were away</p>}
        <h2 class="herald-title">The Valley Herald</h2>
        {issue ? (
          <article>
            <p class="muted herald-date">
              Day {issue.day}, with the news of day {issue.day - 1}
              {issue.byModel ? '' : ' (set from the chronicle)'}
            </p>
            <h3 class="herald-headline">{issue.headline}</h3>
            {issue.stories.map((story) => (
              <p>{story}</p>
            ))}
            {issue.gossip && <p class="herald-gossip">Overheard: {issue.gossip}</p>}
          </article>
        ) : (
          <p class="muted">The first issue comes out on the morning of the second day.</p>
        )}
        <div class="herald-nav">
          <button disabled={at + 1 >= issues.length} onClick={() => setAt(at + 1)}>‹ Earlier</button>
          <button disabled={at === 0} onClick={() => setAt(at - 1)}>Later ›</button>
          <button class="primary" onClick={close}>Close</button>
        </div>
      </section>
    </div>
  );
}

/** The pill that opens the Herald, marked while there is an issue this browser has not opened. */
export function HeraldPill({ menu = false }: { menu?: boolean }) {
  if (!ui.herald.value) return null;
  const unread = ui.heraldUnread.value;
  const open = (): void => togglePanel('herald');
  if (menu) return <button onClick={open}>📰 The Valley Herald{unread ? ' (new)' : ''}</button>;
  return (
    <button class={unread ? 'pill new' : 'pill'} title="The Valley Herald: yesterday's news" onClick={open}>
      📰{unread ? ' New' : ''}
    </button>
  );
}
