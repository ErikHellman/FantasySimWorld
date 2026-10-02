/** Where a viewer who is not signed in lands. The sign-in itself happens on the server. */
export function Login({ returnTo }: { returnTo: string }) {
  const back = encodeURIComponent(returnTo);
  return (
    <div class="start">
      <div class="start-card">
        <h1>Fantasy Sim World</h1>
        <p>A small village in a valley, watched together. Sign in to look in on it.</p>
        <div class="signin">
          <a class="button primary big" href={`/auth/google/start?return_to=${back}`}>Sign in with Google</a>
          <a class="button primary big" href={`/auth/apple/start?return_to=${back}`}>Sign in with Apple</a>
        </div>
        <p class="muted small-text">
          Only people the admin has invited can get in. Running a server with a shared token?{' '}
          <a href="/auth/token">Sign in with the token</a>.
        </p>
      </div>
    </div>
  );
}
