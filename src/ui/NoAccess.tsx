/** Shown after a sign-in that worked but whose address is not on the allowlist. */
export function NoAccess({ email }: { email: string | null }) {
  return (
    <div class="start">
      <div class="start-card">
        <h1>Not on the list</h1>
        <p>
          {email ? <>You signed in as <strong>{email}</strong>, but</> : 'You signed in, but'} that address has not been
          invited to watch this valley yet.
        </p>
        <p class="muted">Ask the admin to let you in: they can see your attempt and allow it. Then sign in again.</p>
        <a class="button primary big" href="/login">Sign in again</a>
      </div>
    </div>
  );
}
