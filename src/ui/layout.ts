import { ui } from './store';

/**
 * Phones, held either way up, get the phone layout. Tablets and opened foldables are at least 600 pixels on both
 * sides and keep the desktop one; so does a desktop window unless it is made very small.
 */
export const MOBILE_QUERY = '(max-width: 599px), (max-height: 499px)';

/** Keeps `ui.mobile` and the `mobile` class on <html> in step with the screen size, including after a rotation. */
export function watchLayout(): void {
  const query = matchMedia(MOBILE_QUERY);
  const apply = (mobile: boolean): void => {
    ui.mobile.value = mobile;
    document.documentElement.classList.toggle('mobile', mobile);
    if (!mobile) ui.sheet.value = 'none';
  };
  apply(query.matches);
  query.addEventListener('change', (event) => apply(event.matches));
}

/** A phone, or a bigger screen worked with fingers rather than a mouse: the help and start card describe touch. */
export const isTouchScreen = (): boolean => ui.mobile.value || matchMedia('(pointer: coarse)').matches;
