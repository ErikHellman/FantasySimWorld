import { useEffect, useState } from 'preact/hooks';
import type { GuestWait } from '../net/guestQueue';

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

/** Waiting for a seat: where this page stands in line, on the launch screen. */
export function GuestLine({ wait, problem }: { wait: GuestWait | null; problem: string | null }) {
  return (
    <div class="guest-line">
      {wait ? (
        <>
          <p>
            {wait.seats === 1 ? 'The one seat is taken.' : `All ${wait.seats} seats are taken.`} You are{' '}
            <strong>number {wait.position}</strong> in line.
          </p>
          <p class="muted">
            Checking again in <Countdown at={wait.nextAt} />. Keep this page open to keep your place.
          </p>
        </>
      ) : (
        !problem && <p class="muted">Finding you a seat…</p>
      )}
      {problem && <p class="error">{problem}</p>}
    </div>
  );
}
