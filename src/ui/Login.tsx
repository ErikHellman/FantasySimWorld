import { useEffect, useState } from 'preact/hooks';
import type { SignInOptions } from '../generated/api';
import { api } from '../net/api';

/** What a provider's `?error=` means, in words. */
function problem(error: string | null): string | null {
  if (!error) return null;
  if (error === 'access_denied' || error === 'user_cancelled_authorize') return 'Sign-in was cancelled.';
  return `Sign-in did not work (${error}). Try again.`;
}

/** Where a viewer who is not signed in lands. The sign-in itself happens on the server. */
export function Login({ returnTo, error }: { returnTo: string; error: string | null }) {
  const back = encodeURIComponent(returnTo);
  // Until the server answers, offer every way in; a server that does not know `/auth/providers` gets the same.
  const [options, setOptions] = useState<SignInOptions>({ token: true, google: true, apple: true });
  useEffect(() => {
    api.providers().then(setOptions, () => {});
  }, []);
  const message = problem(error);
  return (
    <div class="start">
      <div class="start-card">
        <h1>Fantasy Sim World</h1>
        <p>A small village in a valley, watched together. Sign in to look in on it.</p>
        {message && <p class="error">{message}</p>}
        <div class="signin">
          {options.google && <a class="button primary big" href={`/auth/google/start?return_to=${back}`}>Sign in with Google</a>}
          {options.apple && <a class="button primary big" href={`/auth/apple/start?return_to=${back}`}>Sign in with Apple</a>}
          {!options.google && !options.apple && options.token && (
            <a class="button primary big" href="/auth/token">Sign in with the access token</a>
          )}
        </div>
        <p class="muted small-text">
          Only people the admin has invited can get in.
          {options.token && (options.google || options.apple) && (
            <>
              {' '}Running a server with a shared token? <a href="/auth/token">Sign in with the token</a>.
            </>
          )}
        </p>
      </div>
    </div>
  );
}
