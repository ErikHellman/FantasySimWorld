import { afterEach, describe, expect, it } from 'vitest';
import type { Status } from '../generated/api';
import { fromStatus, ui } from './store';

const status = (founding: Status['founding']): Status => ({
  llm: { state: 'off', inFlight: 0, queued: 0, requests: 0, failures: 0, tokens: 0, latencyMs: 0, protocol: '', message: '' },
  viewers: 1, speed: 1, throttled: false, population: 0, monsters: 0, alarm: false, raid: false, part: 'morning',
  ambient: { awake: 0, eating: 0, drinking: 0, smithing: 0, relaxing: 0 },
  founding,
});

afterEach(() => {
  ui.founding.value = null;
});

describe('the founding of a new world', () => {
  it('is shown while the status says the village is being settled, and goes when it is founded', () => {
    fromStatus(status({ settled: 4, total: 13 }));
    expect(ui.founding.value).toEqual({ settled: 4, total: 13 });
    fromStatus(status(null));
    expect(ui.founding.value).toBeNull();
  });
});
