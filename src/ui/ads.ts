/**
 * Google AdSense, when the server is configured for it (`ADS_CLIENT`; see docs/deploy.md). The script is added on first
 * need, and Google's own consent message for European viewers comes with it.
 */

import type { AdsConfig } from '../generated/api';
import type { Panel, Sheet } from './store';

declare global {
  interface Window {
    adsbygoogle?: unknown[];
    /** Google's consent message (Funding Choices), present once the ad script has loaded it */
    googlefc?: { callbackQueue?: Array<() => void>; showRevocationMessage?: () => void };
  }
}

let added = false;

/** Adds the AdSense script once per page. */
export function loadAdScript(ads: AdsConfig): void {
  if (added) return;
  added = true;
  const script = document.createElement('script');
  script.async = true;
  script.crossOrigin = 'anonymous';
  script.src = `https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js?client=${encodeURIComponent(ads.client)}`;
  document.head.append(script);
}

/** Asks AdSense to fill the newest `<ins class="adsbygoogle">` on the page. Call it once per slot. */
export function requestAd(): void {
  (window.adsbygoogle ??= []).push({});
}

/** Opens Google's consent message again, so a viewer can change what they agreed to. */
export function showAdChoices(): void {
  const fc = (window.googlefc ??= {});
  (fc.callbackQueue ??= []).push(() => fc.showRevocationMessage?.());
}

/** What decides whether the ad beside the chat shows during play. */
export interface HudAdState {
  ads: AdsConfig | null;
  admin: boolean;
  mobile: boolean;
  selected: boolean;
  panel: Panel;
  sheet: Sheet;
}

/** The ad beside the chat exists at all: ads are on with a slot for it, on a wide screen, for anyone but the admin. */
export function hudAdWanted(state: Pick<HudAdState, 'ads' | 'admin' | 'mobile'>): boolean {
  return !!state.ads?.hudSlot && !state.admin && !state.mobile;
}

/**
 * The ad beside the chat is in view: it steps aside while the viewer is reading the inspector, a site's page or a
 * dialog. It is hidden rather than removed, so coming back shows the same ad instead of asking for a new one.
 */
export function hudAdVisible(state: HudAdState): boolean {
  return hudAdWanted(state) && !state.selected && state.panel === 'none' && state.sheet === 'none';
}
