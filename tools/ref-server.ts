// The reference server: the TypeScript Sim (and, optionally, the Brain) run at
// 20 Hz on a wall clock and served over the exact wire protocol of the Rust
// server (src/generated/api.ts), so the web client can be built and tested
// before the Rust world exists, and both servers can be compared frame by frame.
//
//   bun tools/ref-server.ts                      listens on :8080 (PORT); then `pnpm dev`
//   SEED=42 INITIAL_SPEED=0 bun tools/ref-server.ts
//   LLM_BASE_URL=http://localhost:8787/v1 LLM_MODEL=mock bun tools/ref-server.ts   (with `node tools/mock-llm.mjs`)
//   REF_ROLE=viewer | anonymous                 answer /api/me as a viewer, or 401 everything
//   IMPORT=save.json                             start from a browser export
//
// Nothing is persisted. There is no sign-in: every request is the REF_ROLE user.
import { readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { migrate } from '../src/persist/migrations';
import { createState } from '../src/sim/create';
import { WorldHost, llmFromEnv } from './ref-server/host';
import { createHandler, identityFromEnv } from './ref-server/routes';

const env = process.env;
const port = Number(env.PORT ?? 8080);
const speed = [0, 1, 2, 4].includes(Number(env.INITIAL_SPEED ?? 1)) ? Number(env.INITIAL_SPEED ?? 1) : 1;

function initialState() {
  if (env.IMPORT) {
    const save = migrate(JSON.parse(readFileSync(env.IMPORT, 'utf8')));
    if (!save) throw new Error(`${env.IMPORT} is not a save this version can read`);
    return save.state;
  }
  return createState(env.SEED ? Number(env.SEED) : Math.floor(Math.random() * 2 ** 31));
}

const llm = llmFromEnv(env);
const host = new WorldHost(initialState(), speed, llm);
const identity = identityFromEnv(env.REF_ROLE);
const server = createServer(createHandler(host, identity));
server.keepAliveTimeout = 65_000;
server.listen(port, () => {
  host.start();
  console.log(
    `reference server on http://localhost:${port} (boot ${host.bootId}, world ${host.hash}, tick ${host.sim.state.tick}, speed ${speed}, ` +
      `${identity.me ? identity.me.role : 'anonymous'}, ${llm.enabled ? `LLM ${llm.model} at ${llm.baseUrl}` : 'no LLM'})`,
  );
});

const shutdown = (): void => {
  host.stop();
  server.close();
  process.exit(0);
};
process.on('SIGINT', shutdown);
process.on('SIGTERM', shutdown);
