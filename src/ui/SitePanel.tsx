import { formatClock, gameTime } from '../core/time';
import { Meter } from './Inspector';
import { actions, closeSheet, ui, type SiteDetail } from './store';

// The page for a building or something the village built: what it is, its condition, who lives and is there,
// the work on it and what the chronicle says about it. Every line comes rendered from the server's `SiteDetail`.

/** "the smithy" -> "The smithy", for the top of a page or a chip. */
export const capitalised = (text: string): string => text.charAt(0).toUpperCase() + text.slice(1);

/** One line on the site's condition, from its stage and how far the fire or the work has got. */
export function condition(site: SiteDetail): string {
  switch (site.stage) {
    case 'whole':
      return site.kind === 'building' ? 'Standing whole' : 'Standing';
    case 'burning':
      return `On fire, ${site.fire}%`;
    case 'ruin':
      return site.progress > 0 ? `In ruins, rebuilt ${site.progress}%` : 'In ruins';
    case 'dismantled':
      return 'Taken down to its footings';
    case 'raising':
      return `Being raised, ${site.progress}%`;
    case 'expanded':
      return 'Made bigger';
    case 'pieces':
      return 'In pieces';
    case 'ash':
      return 'Burnt to ash';
    default:
      return capitalised(site.stage.replace('_', ' '));
  }
}

/** "D3 14:20": when a stage began. */
const since = (tick: number): string => {
  const time = gameTime(tick);
  return `D${time.day} ${formatClock(time.minuteOfDay)}`;
};

/** Follows a villager named on the page; on a phone the pane goes away so they can be watched. */
const follow = (id: string): void => {
  actions.select(id);
  if (ui.mobile.value) closeSheet();
};

function People({ people }: { people: SiteDetail['residents'] }) {
  return (
    <>
      {people.map((person, index) => (
        <>
          {index > 0 && ', '}
          <button class="link" onClick={() => follow(person.id)}>{person.name}</button>
        </>
      ))}
    </>
  );
}

export function SitePanel() {
  const site = ui.site.value;
  if (!site) return null;
  const home = site.beds !== null;
  const beds = site.beds ?? 0;
  const project = site.project;
  const stores = site.stockpile;
  return (
    <aside class="panel inspector site">
      {/* On a phone this only puts the page away; the chip is where to stop looking. */}
      <button class="close" title="Close (Esc)" onClick={() => (ui.mobile.value ? closeSheet() : actions.select(null))}>×</button>
      <h2>{capitalised(site.name)}</h2>
      {site.kind === 'built' && <p class="subtitle">Put up by the village at {site.placeName}</p>}
      {site.kind === 'building' && <p class="subtitle">{capitalised(site.placeName) === capitalised(site.name) ? 'A building of the village' : site.placeName}</p>}
      <p class="status">
        {condition(site)}
        {site.sinceTick > 0 && <span class="muted"> · since {since(site.sinceTick)}</span>}
      </p>
      <div class="tab-body">
        <p class="backstory">{site.description}</p>
        {site.kind === 'building' && (
          <>
            <h3>People</h3>
            {home && (
              <p>
                {site.residents.length > 0 ? <>Lives here: <People people={site.residents} /></> : 'Nobody lives here'}
                <span class="muted"> · {beds} bed{beds === 1 ? '' : 's'}</span>
              </p>
            )}
            <p>{site.present.length > 0 ? <>Here now: <People people={site.present} /></> : <span class="muted">Nobody is here right now.</span>}</p>
          </>
        )}
        {(project || stores) && (
          <>
            <h3>Work</h3>
            {project && (
              <>
                <p>{capitalised(project.label)}{project.helpers.length > 0 && <>, with <People people={project.helpers} /></>}</p>
                <Meter label="Built" value={project.progress} tone="good" />
                {project.needs.length > 0 && (
                  <ul class="needs">
                    {project.needs.map((need) => (
                      <li class={need.delivered >= need.needed ? 'met' : ''}>{need.resource} {need.delivered} / {need.needed}</li>
                    ))}
                  </ul>
                )}
              </>
            )}
            {stores && <p class="muted inventory">In store: {stores.wood} wood · {stores.stone} stone · {stores.ore} ore</p>}
          </>
        )}
        {site.history.length > 0 && (
          <>
            <h3>History</h3>
            <ol class="history">
              {site.history.map((entry) => (
                <li class={`chron ${entry.kind}`} key={`${entry.tick}:${entry.text}`}>
                  <span class="chron-time">{since(entry.tick)}</span>
                  <span class="chron-text">{entry.text}</span>
                </li>
              ))}
            </ol>
          </>
        )}
      </div>
    </aside>
  );
}
