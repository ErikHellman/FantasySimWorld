import { useEffect, useState } from 'preact/hooks';
import type { SignInOptions, SignInProvider } from '../generated/api';
import { api } from '../net/api';

/** What a provider's `?error=` means, in words. */
function problem(error: string | null): string | null {
  if (!error) return null;
  if (error === 'access_denied' || error === 'user_cancelled_authorize') return 'Sign-in was cancelled.';
  return `Sign-in did not work (${error}). Try again.`;
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
const UNKNOWN: SignInOptions = { open: false, token: true, providers: [] };

/** Only paths on this site, like the server's `safe_return`: "/x" but not "//evil.example" or "javascript:…". */
function onSite(path: string): string {
  return path.startsWith('/') && !path.startsWith('//') && !/[\\\s\p{Cc}]/u.test(path) ? path : '/';
}

/** Where a viewer who is not signed in lands. The sign-in itself happens on the server. */
export function Login({ returnTo, error }: { returnTo: string; error: string | null }) {
  const back = encodeURIComponent(returnTo);
  // Until the server answers there are no buttons: a guess would offer providers this server has switched off.
  const [options, setOptions] = useState<SignInOptions | null>(null);
  useEffect(() => {
    api.providers().then(setOptions, () => setOptions(UNKNOWN));
  }, []);
  const message = problem(error);
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
        <p>A small village in a valley, watched together. Sign in to look in on it.</p>
        {message && <p class="error">{message}</p>}
        <div class="signin">
          {options?.providers.map((provider) => (
            <a key={provider} class="button primary big" href={`/auth/${provider}/start?return_to=${back}`}>
              {LABELS[provider]}
            </a>
          ))}
          {options?.token && options.providers.length === 0 && (
            <a class="button primary big" href="/auth/token">Sign in with the access token</a>
          )}
        </div>
        <p class="muted small-text">
          Only people the admin has invited can get in.
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
