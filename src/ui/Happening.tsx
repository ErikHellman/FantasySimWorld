import { hotspotIcon } from './director';
import { actions, ui } from './store';

/** The top bar's switch for the Auto camera. */
export function AutoPill() {
  const on = ui.auto.value;
  return (
    <button
      class={`pill auto ${on ? 'on' : ''}`}
      aria-pressed={on}
      title={on ? 'Auto camera on: when you leave the camera alone, it follows whatever is worth watching. Click to turn it off' : 'Auto camera off: click to let the camera find what is worth watching'}
      onClick={() => actions.setAuto(!on)}
    >
      🎬 Auto
    </button>
  );
}

/**
 * Says what the Auto camera is showing, with ✕ to turn it off; on a phone, with nobody followed and Auto off, a
 * button to turn it on. Hidden while a pane or window is open on a phone.
 */
export function AutoChip({ phone }: { phone: boolean }) {
  const shot = ui.autoShot.value;
  if (phone && (ui.sheet.value !== 'none' || ui.panel.value !== 'none' || ui.selectedId.value)) return null;
  if (!shot) {
    if (!phone || ui.auto.value) return null;
    return (
      <div class="follow-chip auto-chip">
        <button class="follow-open" onClick={() => actions.setAuto(true)}>🎬 Auto camera</button>
      </div>
    );
  }
  return (
    <div class="follow-chip auto-chip">
      <button class="follow-open" title="Follow and open their page" onClick={() => actions.watch(shot)}>
        🎬 {hotspotIcon(shot.kind)} <strong>{shot.text}</strong>
      </button>
      <button class="follow-stop" title="Turn the Auto camera off" aria-label="Turn the Auto camera off" onClick={() => actions.setAuto(false)}>✕</button>
    </div>
  );
}

/** "Happening now": a card for each big thing that has just started, with a button to go and watch it. */
export function HappeningCards() {
  const cards = ui.cards.value;
  if (cards.length === 0 || ui.panel.value !== 'none') return null;
  const dismiss = (id: number): void => {
    ui.cards.value = ui.cards.value.filter((card) => card.id !== id);
  };
  return (
    <ol class="happening" aria-live="polite" aria-label="Happening now">
      {cards.map((card) => (
        <li key={card.id} class={`panel happening-card ${card.spot.kind}`}>
          <span class="happening-icon">{hotspotIcon(card.spot.kind)}</span>
          <span class="happening-text">
            <span class="happening-label">Happening now</span>
            {card.spot.text}
          </span>
          <button
            class="primary"
            onClick={() => {
              actions.watch(card.spot);
              dismiss(card.id);
            }}
          >
            Watch
          </button>
          <button class="happening-close" title="Dismiss" aria-label="Dismiss" onClick={() => dismiss(card.id)}>✕</button>
        </li>
      ))}
    </ol>
  );
}
