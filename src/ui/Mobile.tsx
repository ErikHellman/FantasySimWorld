import type { RefObject } from 'preact';
import { useLayoutEffect, useRef } from 'preact/hooks';
import { ChatPanel } from './Chat';
import { Inspector } from './Inspector';
import { Chronicle, LlmPill, Roster, Speeds } from './Panes';
import { SitePanel, capitalised } from './SitePanel';
import { actions, isAdmin, isSiteId, togglePanel, toggleSheet, ui, type Sheet } from './store';

// The phone layout: the world fills the screen, a slim bar at the top keeps the time, and a tab bar opens one pane
// at a time over the world. Windows (settings, help and the like) are shared with the desktop and fill the screen.

/** The first name of the villager being followed, or the name of the site being looked at, as far as the HUD knows it yet. */
export const selectedName = (): { name: string; site: boolean } | null => {
  const id = ui.selectedId.value;
  if (!id) return null;
  if (isSiteId(id)) return { name: ui.site.value?.id === id ? capitalised(ui.site.value.name) : '…', site: true };
  const name = ui.detail.value?.id === id ? ui.detail.value.name : ui.roster.value.find((entry) => entry.id === id)?.name;
  return { name: name?.split(' ')[0] ?? '…', site: false };
};

function TopBar() {
  const clock = ui.clock.value;
  const connection = ui.connection.value;
  const speed = ui.speed.value;
  return (
    <header class="topbar">
      <div class="clock" title={clock.part}>
        <span class="clock-day">Day {clock.day}</span>
        <span class="clock-time">{clock.time}</span>
      </div>
      {speed !== 1 && <div class="pill" title="The world's speed, set by an admin">{speed === 0 ? '❚❚ paused' : `${speed}×`}</div>}
      <div class="pills">
        {connection !== 'live' && <span class="pill danger" title="The connection to the server">{connection === 'down' ? 'offline' : 'reconnecting…'}</span>}
        {ui.monsters.value > 0 && <span class="pill danger" title="Monsters abroad">💀 {ui.monsters.value}</span>}
        {ui.dragon.value && <span class="pill danger" title="A dragon over the valley">🐉</span>}
      </div>
    </header>
  );
}

function TabBar({ barRef }: { barRef: RefObject<HTMLElement | null> }) {
  const open = ui.sheet.value;
  const unread = ui.chatUnread.value;
  const selected = selectedName();
  const tab = (sheet: Sheet, icon: string, label: string, badge?: number, disabled = false) => (
    <button class={`tab ${open === sheet ? 'on' : ''}`} disabled={disabled} aria-pressed={open === sheet} onClick={() => toggleSheet(sheet)}>
      <span class="tab-icon">{icon}</span>
      <span class="tab-label">{label}</span>
      {badge ? <span class="chat-unread">{badge}</span> : null}
    </button>
  );
  return (
    <nav class="tabbar" aria-label="Panes" ref={barRef}>
      {tab('villagers', '👥', 'Villagers')}
      {tab('feed', '📜', 'Chronicle')}
      {tab('chat', '💬', 'Chat', unread)}
      {tab('character', selected?.site ? '🏠' : '👤', selected?.name ?? 'Character', 0, !selected)}
      {tab('menu', '☰', 'More')}
    </nav>
  );
}

/** Names the villager being followed or the site being looked at; tap it for the page. Hidden while a pane or window is open. */
function FollowChip() {
  const selected = selectedName();
  if (!selected || ui.sheet.value !== 'none' || ui.panel.value !== 'none') return null;
  const stop = selected.site ? 'Stop looking' : 'Stop following';
  return (
    <div class="follow-chip">
      <button class="follow-open" onClick={() => toggleSheet('character')}>
        {selected.site ? 'Looking at' : 'Following'} <strong>{selected.name}</strong> ›
      </button>
      <button class="follow-stop" title={stop} aria-label={stop} onClick={() => actions.select(null)}>✕</button>
    </div>
  );
}

/** What does not fit the phone's top bar: the other pills, the admin's controls and the windows. */
function MenuSheet() {
  const admin = isAdmin();
  const graves = ui.graves.value.length;
  const me = ui.me.value;
  return (
    <div class="menu-sheet">
      <h2>
        <img class="mark" src="/favicon.svg" alt="" width="24" height="24" />
        Fantasy Sim World
      </h2>
      <p class="muted">{ui.clock.value.part}</p>
      <div class="pills">
        <span class="pill" title="People watching">👁 {ui.viewers.value} watching</span>
        <span class="pill" title="Villagers alive">👥 {ui.population.value} alive</span>
        <LlmPill />
      </div>
      {admin && <Speeds />}
      <nav class="menu-list">
        {graves > 0 && <button onClick={() => togglePanel('graves')}>🪦 The graveyard ({graves})</button>}
        <button onClick={() => togglePanel('settings')}>⚙ Settings</button>
        <button onClick={() => togglePanel('help')}>❓ Help</button>
        {admin && <button onClick={() => togglePanel('admin')}>🛠 Admin</button>}
      </nav>
      {admin && <p class="muted small-text">You are an admin.</p>}
    </div>
  );
}

function SheetBody({ sheet }: { sheet: Sheet }) {
  switch (sheet) {
    case 'villagers':
      return <Roster />;
    case 'feed':
      return <Chronicle />;
    case 'chat':
      return <ChatPanel sheet />;
    case 'character':
      if (ui.site.value) return <SitePanel />;
      return ui.detail.value ? <Inspector /> : <p class="muted sheet-loading">Loading…</p>;
    case 'menu':
      return <MenuSheet />;
    case 'none':
      return null;
  }
}

/**
 * Keeps `ui.covered` at how much of the screen the open pane covers, or the tab bar when no pane is open, so the
 * camera can centre on the part of the world still showing.
 */
function useCoveredArea(sheetRef: RefObject<HTMLElement | null>, barRef: RefObject<HTMLElement | null>, sheet: Sheet): void {
  useLayoutEffect(() => {
    const measure = (): void => {
      const element = sheetRef.current ?? barRef.current;
      if (!element) return;
      const rect = element.getBoundingClientRect();
      // Held upright, the pane spans the bottom and covers from its top edge down; lying on its side, it stands at
      // the right and covers from its left edge across.
      ui.covered.value = rect.width > innerWidth * 0.75
        ? { right: 0, bottom: Math.max(0, innerHeight - rect.top) }
        : { right: Math.max(0, innerWidth - rect.left), bottom: 0 };
    };
    measure();
    const observer = new ResizeObserver(measure);
    if (sheetRef.current) observer.observe(sheetRef.current);
    if (barRef.current) observer.observe(barRef.current);
    addEventListener('resize', measure);
    return () => {
      observer.disconnect();
      removeEventListener('resize', measure);
      ui.covered.value = { right: 0, bottom: 0 };
    };
  }, [sheet]);
}

export function MobileHud() {
  // The followed villager can die or be let go while their page is open; then there is nothing to show.
  const sheet = ui.sheet.value === 'character' && !ui.selectedId.value ? 'none' : ui.sheet.value;
  const sheetRef = useRef<HTMLElement>(null);
  const barRef = useRef<HTMLElement>(null);
  useCoveredArea(sheetRef, barRef, sheet);
  return (
    <>
      <TopBar />
      <FollowChip />
      {sheet !== 'none' && (
        <section class={`panel sheet sheet-${sheet}`} aria-label="Pane" ref={sheetRef}>
          <SheetBody sheet={sheet} />
        </section>
      )}
      <TabBar barRef={barRef} />
    </>
  );
}
