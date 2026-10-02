// Downloads the sounds listed in sources.ts and writes the game's audio:
// seamless ambient loops and effects in public/audio/, music in
// public/audio/music/, and the credits (public/audio/CREDITS.md and
// src/audio/credits.ts). Needs ffmpeg (with libopus and libmp3lame) and bsdtar.
//
//   bun tools/audio/build-audio.ts            # everything
//   bun tools/audio/build-audio.ts smithy     # only the named slots or tracks
//
// Downloads are cached in tools/audio/.cache/ (git-ignored).
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';
import { MUSIC_TRACKS, STING_TRACKS } from '../../src/audio/musicTracks';
import { AUDIO_SLOTS } from '../../src/audio/slots';
import { AMBIENT, EFFECTS, MUSIC, SOURCES, type AmbientLoop, type Effect, type Source, type SourceId } from './sources';

const ROOT = resolve(import.meta.dirname, '..', '..');
const CACHE = join(import.meta.dirname, '.cache');
const OUT = join(ROOT, 'public', 'audio');
const MUSIC_OUT = join(OUT, 'music');
const RATE = 48_000;
/** loudness the ambient loops are levelled to; the mixer's slot gains set the balance */
const AMBIENT_RMS_DB = -20;
const PEAK = 0.9;
const MUSIC_LUFS = -18;

const only = new Set(process.argv.slice(2));
const wanted = (id: string): boolean => only.size === 0 || only.has(id);

function run(cmd: string, args: readonly string[], input?: Buffer): Buffer {
  const result = spawnSync(cmd, args, { input, maxBuffer: 1 << 30 });
  if (result.status !== 0) throw new Error(`${cmd} ${args.join(' ')}\n${result.stderr?.toString().slice(-2000)}`);
  return result.stdout;
}

async function download(url: string): Promise<string> {
  const dir = join(CACHE, 'downloads');
  mkdirSync(dir, { recursive: true });
  const file = join(dir, decodeURIComponent(basename(new URL(url).pathname)));
  if (existsSync(file)) return file;
  console.log(`downloading ${url}`);
  const response = await fetch(url, { headers: { 'User-Agent': 'FantasySimWorld audio build (https://github.com/ErikHellman)' } });
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  writeFileSync(file, Buffer.from(await response.arrayBuffer()));
  return file;
}

/** The local path of a source, downloading (and unpacking) it the first time. */
async function local(source: Source): Promise<string> {
  const file = await download(source.url);
  if (!source.member) return file;
  const dir = join(CACHE, 'unpacked', basename(file));
  if (!existsSync(dir)) {
    mkdirSync(dir, { recursive: true });
    run('bsdtar', ['-xf', file, '-C', dir]);
  }
  const member = join(dir, source.member);
  if (!existsSync(member)) throw new Error(`${source.member} is not in ${basename(file)}`);
  return member;
}

/** Decodes part of a file to mono float samples at RATE. */
async function decode(id: SourceId, from = 0, seconds?: number, filter?: string): Promise<Float32Array> {
  const args = ['-v', 'error', '-ss', String(from), ...(seconds ? ['-t', String(seconds)] : []), '-i', await local(SOURCES[id])];
  if (filter) args.push('-af', filter);
  args.push('-ac', '1', '-ar', String(RATE), '-f', 'f32le', '-');
  const bytes = run('ffmpeg', args);
  return new Float32Array(bytes.buffer, bytes.byteOffset, bytes.byteLength / 4).slice();
}

function rms(samples: Float32Array): number {
  let sum = 0;
  for (const s of samples) sum += s * s;
  return Math.sqrt(sum / samples.length);
}

function peak(samples: Float32Array): number {
  let max = 0;
  for (const s of samples) max = Math.max(max, Math.abs(s));
  return max;
}

function scale(samples: Float32Array, gain: number): Float32Array {
  for (let i = 0; i < samples.length; i++) samples[i] = samples[i]! * gain;
  return samples;
}

/**
 * A loop of `length` samples from `source` (which must be `length + fade`
 * long): its tail is crossfaded, at equal power, into the bit just before
 * where the loop starts, so the end runs straight on into the beginning.
 */
function seamless(source: Float32Array, length: number, fade: number): Float32Array {
  if (source.length < length + fade) throw new Error(`source too short: ${source.length} < ${length + fade} samples`);
  const out = new Float32Array(length);
  for (let i = 0; i < length; i++) {
    const body = source[fade + i]!;
    if (i < length - fade) {
      out[i] = body;
    } else {
      const t = (i - (length - fade)) / fade;
      out[i] = body * Math.cos((t * Math.PI) / 2) + source[i - (length - fade)]! * Math.sin((t * Math.PI) / 2);
    }
  }
  return out;
}

function encode(samples: Float32Array, file: string, codec: string[]): void {
  const input = Buffer.from(samples.buffer, samples.byteOffset, samples.byteLength);
  run('ffmpeg', ['-v', 'error', '-y', '-f', 'f32le', '-ar', String(RATE), '-ac', '1', '-i', '-', ...codec, file], input);
}

async function buildLoop(loop: AmbientLoop): Promise<void> {
  const length = Math.round(loop.seconds * RATE);
  const fade = Math.round(Math.min(3, loop.seconds / 8) * RATE);
  const mix = new Float32Array(length);
  for (const layer of loop.layers) {
    const gain = layer.gain ?? 1;
    if (layer.at) {
      const shot = await decode(layer.source, layer.from ?? 0, undefined, layer.filter);
      for (const at of layer.at) {
        const start = Math.round(at * RATE);
        for (let i = 0; i < shot.length; i++) mix[(start + i) % length]! += shot[i]! * gain;
      }
    } else {
      const bed = seamless(await decode(layer.source, layer.from ?? 0, (length + fade) / RATE + 0.1, layer.filter), length, fade);
      for (let i = 0; i < length; i++) mix[i]! += bed[i]! * gain;
    }
  }
  const level = 10 ** (AMBIENT_RMS_DB / 20) / Math.max(1e-6, rms(mix));
  scale(mix, Math.min(level, PEAK / Math.max(1e-6, peak(mix))));
  encode(mix, join(OUT, `${loop.slot}.ogg`), ['-c:a', 'libopus', '-b:a', '48k']);
  encode(mix, join(OUT, `${loop.slot}.mp3`), ['-c:a', 'libmp3lame', '-b:a', '64k']);
}

async function buildEffect(effect: Effect): Promise<void> {
  let samples = await decode(effect.source, effect.from ?? 0, effect.seconds, effect.filter);
  // Start on the sound, not on the silence before it.
  const threshold = peak(samples) * 0.02;
  const first = samples.findIndex((s) => Math.abs(s) > threshold);
  samples = samples.slice(Math.max(0, first - Math.round(0.005 * RATE)));
  const fadeIn = Math.min(samples.length, Math.round(0.004 * RATE));
  for (let i = 0; i < fadeIn; i++) samples[i]! *= i / fadeIn;
  if (effect.seconds) {
    const fadeOut = Math.round(samples.length / 4);
    for (let i = 0; i < fadeOut; i++) samples[samples.length - 1 - i]! *= i / fadeOut;
  }
  scale(samples, PEAK / Math.max(1e-6, peak(samples)));
  encode(samples, join(OUT, `${effect.slot}.mp3`), ['-c:a', 'libmp3lame', '-b:a', '96k']);
}

/** Music keeps its stereo; levelled with two-pass loudnorm and faded out where it is cut short. */
async function buildMusic(id: string, source: SourceId, seconds: number | undefined): Promise<void> {
  const input = await local(SOURCES[source]);
  const probe = Number(run('ffprobe', ['-v', 'error', '-show_entries', 'format=duration', '-of', 'csv=p=0', input]).toString());
  const length = Math.min(probe, seconds ?? probe);
  const fadeOut = length < probe ? Math.min(6, length / 3) : 0.5;
  const shape = `afade=t=out:st=${(length - fadeOut).toFixed(2)}:d=${fadeOut.toFixed(2)}`;
  const target = `I=${MUSIC_LUFS}:TP=-1.5:LRA=11`;
  const measure = spawnSync('ffmpeg', ['-hide_banner', '-t', String(length), '-i', input, '-af', `${shape},loudnorm=${target}:print_format=json`, '-f', 'null', '-'], { maxBuffer: 1 << 26 });
  const json = measure.stderr.toString();
  const stats = JSON.parse(json.slice(json.lastIndexOf('{'), json.lastIndexOf('}') + 1)) as Record<string, string>;
  const measured = `measured_I=${stats.input_i}:measured_TP=${stats.input_tp}:measured_LRA=${stats.input_lra}:measured_thresh=${stats.input_thresh}:offset=${stats.target_offset}:linear=true`;
  run('ffmpeg', ['-v', 'error', '-y', '-t', String(length), '-i', input, '-af', `${shape},loudnorm=${target}:${measured}`, '-ar', '44100', '-ac', '2', '-c:a', 'libmp3lame', '-b:a', '96k', join(MUSIC_OUT, `${id}.mp3`)]);
}

function checkCoverage(): void {
  const slots = new Set([...AMBIENT.map((loop) => loop.slot), ...EFFECTS.map((effect) => effect.slot)]);
  const missingSlots = AUDIO_SLOTS.filter((slot) => !slots.has(slot.id)).map((slot) => slot.id);
  const tracks = [...MUSIC_TRACKS.map((track) => track.id), ...Object.values(STING_TRACKS)];
  const missingTracks = tracks.filter((id) => !MUSIC[id]);
  if (missingSlots.length || missingTracks.length) throw new Error(`no source for: ${[...missingSlots, ...missingTracks].join(', ')}`);
}

interface Credit {
  title: string;
  author: string;
  license: string;
  url: string;
  usedFor: string[];
}

function credits(): Credit[] {
  const byKey = new Map<string, Credit>();
  const add = (id: SourceId, use: string): void => {
    const source = SOURCES[id];
    const key = `${source.title}|${source.author}`;
    const credit = byKey.get(key) ?? { title: source.title, author: source.author, license: source.license, url: source.page, usedFor: [] };
    if (!credit.usedFor.includes(use)) credit.usedFor.push(use);
    byKey.set(key, credit);
  };
  for (const loop of AMBIENT) for (const layer of loop.layers) add(layer.source, loop.slot);
  for (const effect of EFFECTS) add(effect.source, effect.slot);
  for (const [id, track] of Object.entries(MUSIC)) add(track.source, `music: ${id}`);
  return [...byKey.values()].sort((a, b) => a.license.localeCompare(b.license) || a.author.localeCompare(b.author) || a.title.localeCompare(b.title));
}

function writeCredits(list: readonly Credit[]): void {
  const lines = [
    '# Sound credits',
    '',
    'Written by `tools/audio/build-audio.ts` from `tools/audio/sources.ts`; do not edit by hand.',
    'CC BY works are used under the Creative Commons Attribution 4.0 license (https://creativecommons.org/licenses/by/4.0/).',
    'Some sounds were cut, mixed, levelled and re-encoded for the game.',
    '',
    '| Work | Author | License | Used for |',
    '| ---- | ------ | ------- | -------- |',
    ...list.map((c) => `| [${c.title}](${c.url}) | ${c.author} | ${c.license} | ${c.usedFor.join(', ')} |`),
    '',
  ];
  writeFileSync(join(OUT, 'CREDITS.md'), lines.join('\n'));
  const quote = (text: string): string => `'${text.replaceAll('\\', '\\\\').replaceAll("'", "\\'")}'`;
  const entries = list.map((c) => `  { title: ${quote(c.title)}, author: ${quote(c.author)}, license: ${quote(c.license)}, url: ${quote(c.url)} },`);
  writeFileSync(
    join(ROOT, 'src', 'audio', 'credits.ts'),
    [
      '// Written by tools/audio/build-audio.ts from tools/audio/sources.ts; do not edit by hand.',
      '',
      'export interface SoundCredit {',
      '  title: string;',
      '  author: string;',
      '  license: string;',
      '  url: string;',
      '}',
      '',
      'export const SOUND_CREDITS: readonly SoundCredit[] = [',
      ...entries,
      '];',
      '',
    ].join('\n'),
  );
}

function report(): void {
  let total = 0;
  for (const dir of [OUT, MUSIC_OUT]) {
    for (const name of readdirSync(dir).sort()) {
      const path = join(dir, name);
      if (!statSync(path).isFile() || name.endsWith('.md')) continue;
      const size = statSync(path).size;
      total += size;
      const duration = Number(run('ffprobe', ['-v', 'error', '-show_entries', 'format=duration', '-of', 'csv=p=0', path]).toString());
      console.log(`${(dir === MUSIC_OUT ? 'music/' : '') + name}`.padEnd(32), `${(size / 1024).toFixed(0).padStart(6)} KB`, `${duration.toFixed(1).padStart(7)} s`);
    }
  }
  console.log(`total ${(total / 1024 / 1024).toFixed(1)} MB`);
}

checkCoverage();
mkdirSync(MUSIC_OUT, { recursive: true });
for (const loop of AMBIENT) if (wanted(loop.slot)) await buildLoop(loop);
for (const effect of EFFECTS) if (wanted(effect.slot)) await buildEffect(effect);
for (const [id, track] of Object.entries(MUSIC)) if (wanted(id)) await buildMusic(id, track.source, track.seconds);
writeCredits(credits());
report();
