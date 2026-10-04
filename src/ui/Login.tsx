import { useEffect, useRef, useState } from 'preact/hooks';
import type { SignInOptions, SignInProvider } from '../generated/api';
import { api } from '../net/api';
import { GuestQueue, type GuestWait } from '../net/guestQueue';

/** What a provider's `?error=` means, in words. */
function problem(error: string | null): string | null {
  if (!error) return null;
  if (error === 'access_denied' || error === 'user_cancelled_authorize') return 'Sign-in was cancelled.';
  return `Sign-in did not work (${error}). Try again.`;
}

/** Why a guest is back here (`?ended=`), in words. */
function endedNote(ended: string | null): string | null {
  if (ended === 'time') return 'Your 15 minutes as a guest are up. Sign in to keep watching, or watch as a guest again.';
  if (ended === 'elsewhere') return 'You are watching as a guest in another tab.';
  if (ended === 'left') return 'You gave up your guest seat. Come back any time.';
  return null;
}

/** The button for each provider. */
const LABELS: Record<SignInProvider, string> = {
  google: 'Sign in with Google',
  apple: 'Sign in with Apple',
  microsoft: 'Sign in with Microsoft',
  linkedin: 'Sign in with LinkedIn',
  github: 'Sign in with GitHub',
  discord: 'Sign in with Discord',
};

/** A server that cannot say how it signs people in is offered the token, which every server understands. */
const UNKNOWN: SignInOptions = { open: false, token: true, providers: [], guests: false };

/** Only paths on this site, like the server's `safe_return`: "/x" but not "//evil.example" or "javascript:…". */
function onSite(path: string): string {
  return path.startsWith('/') && !path.startsWith('//') && !/[\\\s\p{Cc}]/u.test(path) ? path : '/';
}

const TICKET_KEY = 'fsw.guestTicket';

/** The place in line from before a reload of this tab, if any. */
function savedTicket(): string | null {
  try {
    return sessionStorage.getItem(TICKET_KEY);
  } catch {
    return null;
  }
}

function keepTicket(ticket: string | null): void {
  try {
    if (ticket) sessionStorage.setItem(TICKET_KEY, ticket);
    else sessionStorage.removeItem(TICKET_KEY);
  } catch {
    // Without storage a reload starts at the back of the line.
  }
}

/** Seconds until `at`, as m:ss, ticking. */
function Countdown({ at }: { at: number }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1_000);
    return () => clearInterval(timer);
  }, []);
  const seconds = Math.max(0, Math.ceil((at - now) / 1000));
  return <>{Math.floor(seconds / 60)}:{String(seconds % 60).padStart(2, '0')}</>;
}

/** Waiting for a guest seat: where this page stands in line. */
function GuestLine({ wait, problem, cancel }: { wait: GuestWait | null; problem: string | null; cancel: () => void }) {
  return (
    <div class="guest-line">
      {wait ? (
        <>
          <p>
            {wait.seats === 1 ? 'The one guest seat is taken.' : `All ${wait.seats} guest seats are taken.`} You are{' '}
            <strong>number {wait.position}</strong> in line.
          </p>
          <p class="muted">
            Checking again in <Countdown at={wait.nextAt} />. Keep this page open to keep your place.
          </p>
        </>
      ) : (
        !problem && <p class="muted">Holding your place in line…</p>
      )}
      {problem && <p class="error">{problem}</p>}
      <button onClick={cancel}>Stop waiting</button>
    </div>
  );
}

/** Where a viewer who is not signed in lands, and where a guest comes back to. The sign-in itself happens on the server. */
export function Login({ returnTo, error, ended = null }: { returnTo: string; error: string | null; ended?: string | null }) {
  const back = encodeURIComponent(returnTo);
  // Until the server answers there are no buttons: a guess would offer providers this server has switched off.
  const [options, setOptions] = useState<SignInOptions | null>(null);
  useEffect(() => {
    api.providers().then(setOptions, () => setOptions(UNKNOWN));
  }, []);
  const queue = useRef<GuestQueue | null>(null);
  const [inLine, setInLine] = useState(false);
  const [wait, setWait] = useState<GuestWait | null>(null);
  const [guestProblem, setGuestProblem] = useState<string | null>(null);
  const watchAsGuest = (ticket: string | null = null): void => {
    queue.current?.cancel();
    setInLine(true);
    setWait(null);
    setGuestProblem(null);
    queue.current = new GuestQueue({
      ask: (next) => api.guest({ ticket: next }),
      ticket,
      keep: keepTicket,
      admitted: () => location.assign(onSite(returnTo)),
      waiting: (next) => {
        setGuestProblem(null);
        setWait(next);
      },
      failed: setGuestProblem,
    });
    queue.current.start();
  };
  const stopWaiting = (): void => {
    queue.current?.cancel();
    queue.current = null;
    setInLine(false);
  };
  // A reload while waiting carries on in the same place in line.
  useEffect(() => {
    const ticket = savedTicket();
    if (ticket) watchAsGuest(ticket);
  }, []);
  const message = problem(error);
  const note = endedNote(ended);
  if (options?.open) {
    return (
      <div class="start">
        <div class="start-card">
          <h1>Fantasy Sim World</h1>
          <p>Sign-in is off on this server (local development).</p>
          <div class="signin">
            <a class="button primary big" href={onSite(returnTo)}>Enter the valley</a>
          </div>
        </div>
      </div>
    );
  }
  return (
    <div class="start">
      <div class="start-card">
        <h1>Fantasy Sim World</h1>
        <p>
          A small village in a valley, watched together. Sign in to look in on it
          {options?.guests ? ', or watch as a guest for 15 minutes.' : '.'}
        </p>
        {message && <p class="error">{message}</p>}
        {note && <p class="ended">{note}</p>}
        {inLine && <GuestLine wait={wait} problem={guestProblem} cancel={stopWaiting} />}
        <div class="signin" style={inLine ? { display: 'none' } : undefined}>
          {options?.providers.map((provider) => (
            <a key={provider} class="button primary big" href={`/auth/${provider}/start?return_to=${back}`}>
              {LABELS[provider]}
            </a>
          ))}
          {options?.token && options.providers.length === 0 && (
            <a class="button primary big" href="/auth/token">Sign in with the access token</a>
          )}
          {options?.guests && (
            <button class="button big" onClick={() => watchAsGuest()}>Watch as a guest</button>
          )}
        </div>
        <p class="muted small-text">
          Only people the admin has invited can {options?.guests ? 'sign in' : 'get in'}.
          {options?.token && options.providers.length > 0 && (
            <>
              {' '}Running a server with a shared token? <a href="/auth/token">Sign in with the token</a>.
            </>
          )}
        </p>
      </div>
    </div>
  );
}
