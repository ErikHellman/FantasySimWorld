import { useState } from 'preact/hooks';
import type { CreditGroup, CreditOffer } from '../generated/api';
import { HttpError } from '../net/api';
import { actions, offerBlocked, ui } from './store';

const GROUPS: readonly { group: CreditGroup; title: string; blurb: string }[] = [
  { group: 'gift', title: 'Gifts', blurb: 'For the villager you have selected.' },
  { group: 'monster', title: 'The dead', blurb: 'Rises where your view is centred, and crumbles after a few hours of the village’s day.' },
  { group: 'blessing', title: 'Blessings', blurb: 'For the whole village.' },
  { group: 'threat', title: 'Calamities', blurb: 'For the whole village, whether it likes it or not.' },
];

/** What credits buy. Opened from the credits pill; the server decides whether each thing can happen. */
export function ShopPanel() {
  const credits = ui.credits.value;
  const [status, setStatus] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const close = (): void => {
    ui.panel.value = 'none';
  };
  if (!credits) return null;
  const selected = ui.roster.value.find((entry) => entry.id === ui.selectedId.value);

  const buy = async (offer: CreditOffer): Promise<void> => {
    setBusy(true);
    setStatus(null);
    try {
      await actions.buy(offer);
      // Step aside so the viewer sees it rise where they were looking.
      if (offer.target === 'point') close();
      else setStatus(`${offer.name}: done.`);
    } catch (error) {
      setStatus(error instanceof HttpError ? error.message : 'The server did not answer. Try again.');
    } finally {
      setBusy(false);
    }
  };

  return (
    <div class="modal" onClick={close}>
      <section class="panel dialog shop" role="dialog" aria-label="Credits" onClick={(event) => event.stopPropagation()}>
        <button class="close" title="Close (Esc)" onClick={close}>×</button>
        <h2>✦ {credits.balance} credits</h2>
        <p class="muted">
          You earn {credits.perMinute} a minute while this tab is in front of you, up to {credits.cap}. Spend them and the
          villagers will know it was {ui.chatName.value ? <b>{ui.chatName.value}</b> : 'you'}: they give thanks, they plead,
          and when enough of them pray to you in a day, you earn a little more.
        </p>
        {status && <p class="shop-status" role="status">{status}</p>}
        {GROUPS.map(({ group, title, blurb }) => {
          const offers = credits.offers.filter((offer) => offer.group === group);
          if (offers.length === 0) return null;
          return (
            <div key={group} class="shop-group">
              <h3>{title}</h3>
              <p class="muted small-text">{group === 'gift' && selected ? `For ${selected.name}.` : blurb}</p>
              <ul class="shop-offers">
                {offers.map((offer) => {
                  const blocked = offerBlocked(offer, credits.balance, ui.selectedId.value);
                  return (
                    <li key={offer.id}>
                      <div class="shop-what">
                        <b>{offer.name}</b>
                        <span class="muted small-text">{blocked ?? offer.description}</span>
                      </div>
                      <button disabled={busy || blocked !== null} onClick={() => void buy(offer)}>✦ {offer.cost}</button>
                    </li>
                  );
                })}
              </ul>
            </div>
          );
        })}
      </section>
    </div>
  );
}
