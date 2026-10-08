import { useState } from 'preact/hooks';
import { formatClock, gameTime } from '../core/time';
import { actions, closeSheet, ui, type CharacterDetail, type MonsterDetail, type RelationRow } from './store';

type Tab = 'about' | 'mind' | 'bonds' | 'memory';

const LABEL_CLASS: Record<string, string> = {
  spouse: 'love', lover: 'love', family: 'good', 'close friend': 'good', friend: 'good', acquaintance: '', stranger: '', rival: 'bad', enemy: 'bad',
};

export function Meter({ label, value, max = 100, tone }: { label: string; value: number; max?: number; tone?: string }) {
  const fraction = Math.max(0, Math.min(1, value / max));
  return (
    <div class="meter">
      <span class="meter-label">{label}</span>
      <span class="meter-track"><span class={`meter-fill ${tone ?? (fraction < 0.25 ? 'low' : '')}`} style={{ width: `${Math.round(fraction * 100)}%` }} /></span>
      <span class="meter-value">{Math.round(value)}</span>
    </div>
  );
}

function Relation({ row }: { row: RelationRow }) {
  const affinity = Math.max(-100, Math.min(100, row.affinity));
  return (
    <li class="relation">
      <div class="relation-head">
        {row.alive ? <button class="link" onClick={() => actions.select(row.id)}>{row.name}</button> : <span class="muted">{row.name}</span>}
        <span class={`badge ${LABEL_CLASS[row.label] ?? ''}`}>{row.label}</span>
        {row.rung !== 'calm' && <span class="badge bad" title="How far the hostility has gone">{row.rung}</span>}
      </div>
      <div class="affinity" title={`Affinity ${Math.round(row.affinity)}`}>
        <span class="affinity-mid" />
        <span
          class={`affinity-fill ${affinity < 0 ? 'neg' : 'pos'}`}
          style={affinity < 0 ? { right: '50%', width: `${-affinity / 2}%` } : { left: '50%', width: `${affinity / 2}%` }}
        />
      </div>
      <div class="relation-stats">
        {row.attraction >= 15 && <span class="love">♥ {Math.round(row.attraction)}</span>}
        {row.grievance >= 10 && <span class="bad-text">grievance {Math.round(row.grievance)}</span>}
      </div>
      {row.summary && <p class="relation-summary">“{row.summary}”</p>}
    </li>
  );
}

function About({ d }: { d: CharacterDetail }) {
  return (
    <div class="tab-body">
      <p class="backstory">{d.backstory}</p>
      <div class="chips">{d.traits.map((trait) => <span class="chip">{trait}</span>)}</div>
      <h3>Wants</h3>
      <ul class="goals">{d.goals.map((goal) => <li>{goal}</li>)}</ul>
      <h3>State</h3>
      <Meter label="Health" value={d.hp} max={d.maxHp} tone={d.hp < d.maxHp * 0.4 ? 'low' : 'good'} />
      <Meter label="Fed" value={d.needs.hunger} />
      <Meter label="Rested" value={d.needs.energy} />
      <Meter label="Company" value={d.needs.social} />
      <p class="muted inventory">
        Carrying: {d.inventory.food} food · {d.inventory.potion} potion{d.inventory.potion === 1 ? '' : 's'} · {d.inventory.gift} gift{d.inventory.gift === 1 ? '' : 's'} · {d.gold} gold
        {d.inventory.trinket > 0 && ` · ${d.inventory.trinket} trinket${d.inventory.trinket === 1 ? '' : 's'}`}
        {d.inventory.relic > 0 && ` · ${d.inventory.relic} relic${d.inventory.relic === 1 ? '' : 's'}`}
        {d.carried.wood > 0 && ` · ${d.carried.wood} wood`}
        {d.carried.stone > 0 && ` · ${d.carried.stone} stone`}
        {d.carried.ore > 0 && ` · ${d.carried.ore} ore`}
        {d.kills > 0 && ` · ${d.kills} monster${d.kills === 1 ? '' : 's'} destroyed`}
      </p>
      <p class="muted inventory">
        {d.homeless ? `Homeless: ${d.home} burned down. Sleeps rough by the campfire until it is rebuilt.` : `Home: ${d.home}`}
        {d.spouse && ` · Married to ${d.spouse}`}
      </p>
    </div>
  );
}

function Mind({ d }: { d: CharacterDetail }) {
  return (
    <div class="tab-body">
      <h3>Thinking</h3>
      <p class="thought">{d.thought ? `“${d.thought}”` : <span class="muted">Nothing in particular.</span>}</p>
      {d.summary && (
        <>
          <h3>How they see themselves</h3>
          <p>{d.summary}</p>
        </>
      )}
      <h3>Today's plan <span class="muted">({d.planSource === 'llm' ? 'their own' : 'routine'})</span></h3>
      <ol class="plan">
        {d.plan.map((block) => (
          <li class={block.current ? 'current' : ''}><span class="plan-time">{block.time}</span> {block.text}</li>
        ))}
      </ol>
    </div>
  );
}

function Memories({ d }: { d: CharacterDetail }) {
  return (
    <div class="tab-body">
      <ol class="memories">
        {d.memories.map((memory) => {
          const time = gameTime(memory.tick);
          return (
            <li class={`memory ${memory.kind}`}>
              <span class="memory-meta">D{time.day} {formatClock(time.minuteOfDay)} · {memory.kind} · {'●'.repeat(Math.max(1, Math.round(memory.importance / 2)))}</span>
              <span>{memory.text}</span>
            </li>
          );
        })}
      </ol>
    </div>
  );
}

/** The card of one of the dead or an orc raider: what it is, how it fares, what it is after and what it has done. No tabs. */
function MonsterCard({ d }: { d: MonsterDetail }) {
  const target = d.target;
  const plural = (n: number, word: string): string => `${n} ${word}${n === 1 ? '' : 's'}`;
  const side = d.faction === 'orc' ? 'one of a war band' : 'one of the dead';
  return (
    <>
      <h2>{d.name}</h2>
      <p class="subtitle">{d.species} · {side}{d.lasting && ' · does not fear the dawn'}</p>
      <p class="status">{d.status}</p>
      <div class="tab-body">
        {/* Red whatever it has left, as its label's bar is. */}
        <Meter label="Health" value={d.hp} max={d.maxHp} tone="low" />
        {target && (
          <p>
            Hunting <button class="link" onClick={() => actions.select(target.id)}>{target.name}</button>
          </p>
        )}
        {d.rose && <p class="muted">{d.rose}.</p>}
        {d.was && <p class="muted">Once {d.was}.</p>}
        {d.carrying && <p>Carrying off {d.carrying}.</p>}
        <p class="muted inventory">
          {plural(d.kills, 'villager')} slain · {plural(d.blows, 'blow')} landed · strikes for {d.damage[0]}–{d.damage[1]}
        </p>
      </div>
    </>
  );
}

function VillagerCard({ d }: { d: CharacterDetail }) {
  const [tab, setTab] = useState<Tab>('about');
  return (
    <>
      <h2>{d.name}</h2>
      <p class="subtitle">{d.occupation} · {d.age} · {d.pronouns}</p>
      <p class="status">{d.status}</p>
      <nav class="tabs">
        {(['about', 'mind', 'bonds', 'memory'] as Tab[]).map((name) => (
          <button class={tab === name ? 'on' : ''} onClick={() => setTab(name)}>{name}</button>
        ))}
      </nav>
      {tab === 'about' && <About d={d} />}
      {tab === 'mind' && <Mind d={d} />}
      {tab === 'bonds' && (
        <div class="tab-body"><ul class="relations">{d.relations.map((row) => <Relation row={row} key={row.id} />)}</ul></div>
      )}
      {tab === 'memory' && <Memories d={d} />}
    </>
  );
}

export function Inspector() {
  const d = ui.detail.value;
  if (!d) return null;
  return (
    <aside class="panel inspector">
      {/* On a phone this only puts the page away; the follow chip is where to stop following. */}
      <button class="close" title="Close (Esc)" onClick={() => (ui.mobile.value ? closeSheet() : actions.select(null))}>×</button>
      {d.kind === 'monster' ? <MonsterCard d={d} /> : <VillagerCard d={d} />}
    </aside>
  );
}
