import { useEffect, useState } from 'preact/hooks';
import { guestTimeLeft, ui } from './store';

/** The login page, coming back to this one after signing in. */
export const signInHref = (): string => `/login?return_to=${encodeURIComponent(location.pathname + location.search)}`;

/** How long this guest has left, in words, kept fresh; null for everyone who signed in. */
export function useGuestTimeLeft(): string | null {
  const endsAt = ui.guestEndsAt.value;
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (endsAt === null) return;
    const timer = setInterval(() => setNow(Date.now()), 5_000);
    return () => clearInterval(timer);
  }, [endsAt]);
  return endsAt === null ? null : guestTimeLeft(endsAt - now);
}

/** The top bar's reminder that a guest's visit is short, and the way to make it longer. */
export function GuestPill() {
  const left = useGuestTimeLeft();
  return (
    <a class="pill guest" href={signInHref()} title="Watching as a guest. Sign in to stay longer, chat and earn credits.">
      ⏳ Guest, {left}
    </a>
  );
}

/** A line for the start and menu screens. */
export function GuestNote() {
  const left = useGuestTimeLeft();
  return (
    <p class="muted small-text">
      Watching as a guest, {left}. <a href={signInHref()}>Sign in</a> to stay longer, chat and earn credits.
    </p>
  );
}
