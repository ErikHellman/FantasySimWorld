import { useEffect, useRef } from 'preact/hooks';
import { signal } from '@preact/signals';
import { hudAdVisible, hudAdWanted, loadAdScript, requestAd } from './ads';
import { isAdmin, ui } from './store';

/** One AdSense unit of 300×250. It asks for its ad once, when it first mounts. */
function AdUnit({ slot }: { slot: string }) {
  const ads = ui.ads.value;
  const box = useRef<HTMLModElement>(null);
  useEffect(() => {
    if (!ads || !box.current) return;
    loadAdScript(ads);
    requestAd();
  }, []);
  if (!ads) return null;
  return <ins ref={box} class="adsbygoogle ad-unit" data-ad-client={ads.client} data-ad-slot={slot} />;
}

/** The launch screen's ad has been put up; it stays up, so it is never asked for twice. */
let startShown = false;

/**
 * The ad on the launch screen, under the way in. Everyone waiting for a seat sees it; the admin does not. Before
 * `/api/me` answers it waits, unless this page is already in line (so only a guest).
 */
export function StartAd() {
  const slot = ui.ads.value?.startSlot;
  startShown ||= ui.me.value ? !isAdmin() : ui.line.value !== null;
  if (!slot || !startShown) return null;
  return (
    <aside class="ad ad-start" aria-label="Advertisement">
      <span class="ad-label">Advertisement</span>
      <AdUnit slot={slot} />
    </aside>
  );
}

/** The viewer folded the ad beside the chat away; it stays folded until the page is loaded again. */
const folded = signal(false);

/**
 * The ad beside the chat on wide screens during play; phones never get one. Folded away, it leaves a small tab that
 * brings it back.
 */
export function HudAd() {
  const state = {
    ads: ui.ads.value,
    admin: isAdmin(),
    mobile: ui.mobile.value,
    selected: ui.selectedId.value !== null,
    panel: ui.panel.value,
    sheet: ui.sheet.value,
  };
  const slot = state.ads?.hudSlot;
  if (!slot || !hudAdWanted(state)) return null;
  const visible = hudAdVisible(state);
  return (
    <>
      <aside class={`ad ad-hud panel${visible && !folded.value ? '' : ' hidden'}`} aria-label="Advertisement">
        <header class="ad-head">
          <span class="ad-label">Advertisement</span>
          <button class="ad-fold" title="Fold the ad away" onClick={() => (folded.value = true)}>▾</button>
        </header>
        <AdUnit slot={slot} />
      </aside>
      {visible && folded.value && (
        <button class="pill ad-tab" title="Show the ad again" onClick={() => (folded.value = false)}>▴ Ad</button>
      )}
    </>
  );
}
