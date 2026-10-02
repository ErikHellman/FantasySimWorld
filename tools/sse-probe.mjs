// Checks that the event stream survives the hosting platform's proxy: headers
// arrive at once, nothing is buffered or compressed, ticks keep their 50 ms
// rhythm, and an idle stream (server started with INITIAL_SPEED=0) outlives
// the proxy's idle timeout on keep-alives alone.
//
//   AUTH_TOKEN=… node tools/sse-probe.mjs https://world.example.com [seconds]
//
// Exits non-zero when a check fails. For the HTTP version the browser gets,
// run `curl -sI --http2 https://world.example.com/healthz` (look for HTTP/2).
const base = (process.argv[2] ?? 'http://localhost:8080').replace(/\/$/, '');
const seconds = Number(process.argv[3] ?? 60);
const token = process.env.AUTH_TOKEN ?? '';

const problems = [];
const started = performance.now();
const response = await fetch(`${base}/api/stream`, {
  headers: { authorization: `Bearer ${token}`, accept: 'text/event-stream' },
});
const headersAfter = performance.now() - started;
console.log(`HTTP ${response.status} after ${headersAfter.toFixed(0)} ms`);
for (const name of ['content-type', 'cache-control', 'x-accel-buffering', 'content-encoding', 'server', 'via']) {
  const value = response.headers.get(name);
  if (value !== null) console.log(`  ${name}: ${value}`);
}
if (response.status !== 200) {
  console.error('The stream did not open (is AUTH_TOKEN right?)');
  process.exit(1);
}
if (response.headers.get('content-encoding')) problems.push('the stream is compressed, which makes proxies buffer it');
if (headersAfter > 2000) problems.push(`headers took ${headersAfter.toFixed(0)} ms`);

const decoder = new TextDecoder();
const reader = response.body.getReader();
const gaps = [];
let buffer = '';
let lastChunk = performance.now();
let firstEventAt = null;
let ticks = 0;
let keepAlives = 0;
let lastTick = null;
let skipped = 0;
let ended = false;

const deadline = started + seconds * 1000;
const timer = setTimeout(() => reader.cancel(), seconds * 1000);
for (;;) {
  const { value, done } = await reader.read().catch(() => ({ done: true }));
  if (done) {
    ended = performance.now() < deadline - 100;
    break;
  }
  const now = performance.now();
  gaps.push(now - lastChunk);
  lastChunk = now;
  buffer += decoder.decode(value, { stream: true });
  let end;
  while ((end = buffer.indexOf('\n\n')) >= 0) {
    const block = buffer.slice(0, end);
    buffer = buffer.slice(end + 2);
    if (block.startsWith(':')) {
      keepAlives++;
      continue;
    }
    firstEventAt ??= now - started;
    const event = /^event: (.*)$/m.exec(block)?.[1];
    const id = /^id: (.*)$/m.exec(block)?.[1];
    if (event === 'tick' && id) {
      const tick = Number(id.split(':')[1]);
      if (lastTick !== null && tick !== lastTick + 1) skipped += tick - lastTick - 1;
      lastTick = tick;
      ticks++;
    }
  }
}
clearTimeout(timer);

const elapsed = (performance.now() - started) / 1000;
const sorted = [...gaps].sort((a, b) => a - b);
const quantile = (q) => (sorted.length ? sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))] : 0);
console.log(`first event after ${firstEventAt?.toFixed(0) ?? '-'} ms`);
console.log(`${ticks} ticks in ${elapsed.toFixed(1)} s (${(ticks / elapsed).toFixed(1)}/s), ${skipped} missing, ${keepAlives} keep-alives`);
console.log(`gap between reads: p50 ${quantile(0.5).toFixed(0)} ms, p99 ${quantile(0.99).toFixed(0)} ms, max ${quantile(1).toFixed(0)} ms`);

if (firstEventAt === null) problems.push('no event arrived (buffered?)');
if (ended) problems.push(`the stream ended after ${elapsed.toFixed(1)} s`);
if (skipped > 0) problems.push(`${skipped} ticks missing`);
if (ticks > 0 && quantile(0.99) > 1000) problems.push('reads arrive in bursts (the proxy is buffering)');
if (ticks === 0 && seconds > 20 && keepAlives === 0) problems.push('an idle stream got no keep-alives');
if (problems.length) {
  for (const problem of problems) console.error(`FAIL ${problem}`);
  process.exit(1);
}
console.log('OK');
