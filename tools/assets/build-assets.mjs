// Builds public/assets from the KayKit collection.
//
//   KAYKIT_DIR="/path/to/The Complete KayKit Collection v7" pnpm assets
//
// - Environment models are merged into one GLB per bundle (one named node per model).
// - Textures are stripped from every model and written once to public/assets/textures;
//   each material keeps the texture id in extras.tex so the runtime can share materials.
// - Animation files lose their mannequin meshes and are merged onto one skeleton per rig;
//   a rig missing clips borrows them from another, retargeted by bone name.
// - src/generated/assetManifest.ts lists everything, with bounds and clip durations.
// - server/data/*.json carries the same manifest and the tile heights for the Rust server.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { Document, NodeIO } from '@gltf-transform/core';
import { ALL_EXTENSIONS } from '@gltf-transform/extensions';
import { dedup, getBounds, mergeDocuments, prune, resample, unpartition } from '@gltf-transform/functions';
import { bundles, characters, extraTextures, rigs } from './asset-list.mjs';
import { writeServerData } from './server-data.mjs';

const ROOT = process.env.KAYKIT_DIR ?? path.join(os.homedir(), 'Documents/The Complete KayKit Collection v7');
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const OUT = path.join(REPO, 'public/assets');
const MANIFEST = path.join(REPO, 'src/generated/assetManifest.ts');
const HEIGHTS = path.join(REPO, 'src/generated/tileHeights.ts');

// Ground heights are sampled from the hex tiles (and the bridge) so the sim knows
// exactly where water, sand and road are without loading any geometry.
const TILE_GRID = { nx: 32, nz: 37, minX: -1, minZ: -1.16, maxX: 1, maxZ: 1.16 };
const BRIDGE_GRID = { nx: 24, nz: 40, minX: -0.7, minZ: -1, maxX: 0.7, maxZ: 1 };
const NO_GROUND = -128;
const tileHeights = {};
let bridgeHeights = '';
const MODEL_PREFIX = 'M__';

if (!fs.existsSync(ROOT)) {
  console.error(`KayKit collection not found at ${ROOT}. Set KAYKIT_DIR.`);
  process.exit(1);
}

const io = new NodeIO().registerExtensions(ALL_EXTENSIONS);
const quiet = { debug() {}, info() {}, warn() {}, error: console.error };
io.setLogger?.(quiet);

/** texture id -> PNG bytes */
const textures = new Map();

function round(n, digits = 3) {
  const f = 10 ** digits;
  return Math.round(n * f) / f;
}

function textureId(texture, material) {
  const uri = texture.getURI();
  if (uri) return path.basename(uri).replace(/\.[^.]+$/, '');
  if (texture.getName()) return texture.getName().replace(/\.[^.]+$/, '');
  return `${material.getName()}_texture`;
}

/** Removes base colour textures, remembering them by id in material extras. */
function stripTextures(doc) {
  for (const material of doc.getRoot().listMaterials()) {
    const texture = material.getBaseColorTexture();
    if (!texture) continue;
    const id = textureId(texture, material);
    const image = texture.getImage();
    if (image && !textures.has(id)) textures.set(id, Buffer.from(image));
    material.setExtras({ ...material.getExtras(), tex: id });
    material.setBaseColorTexture(null);
  }
  for (const texture of doc.getRoot().listTextures()) texture.dispose();
}

function transformPoint(m, x, y, z) {
  return [m[0] * x + m[4] * y + m[8] * z + m[12], m[1] * x + m[5] * y + m[9] * z + m[13], m[2] * x + m[6] * y + m[10] * z + m[14]];
}

/** Highest surface under each grid cell, as int8 hundredths of a model unit, base64 encoded. */
function sampleHeights(root, grid) {
  const data = new Int8Array(grid.nx * grid.nz).fill(NO_GROUND);
  const cellX = (grid.maxX - grid.minX) / grid.nx;
  const cellZ = (grid.maxZ - grid.minZ) / grid.nz;
  root.traverse((node) => {
    const mesh = node.getMesh();
    if (!mesh) return;
    const matrix = node.getWorldMatrix();
    for (const primitive of mesh.listPrimitives()) {
      const positions = primitive.getAttribute('POSITION');
      const indices = primitive.getIndices();
      const count = indices ? indices.getCount() : positions.getCount();
      const point = (i) => {
        const index = indices ? indices.getScalar(i) : i;
        const p = positions.getElement(index, []);
        return transformPoint(matrix, p[0], p[1], p[2]);
      };
      for (let t = 0; t < count; t += 3) {
        const a = point(t);
        const b = point(t + 1);
        const c = point(t + 2);
        const det = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
        if (Math.abs(det) < 1e-10) continue;
        // Only faces that point up are ground; undersides and walls are not.
        const normalY = (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]);
        if (normalY <= 0) continue;
        const x0 = Math.max(0, Math.floor((Math.min(a[0], b[0], c[0]) - grid.minX) / cellX));
        const x1 = Math.min(grid.nx - 1, Math.floor((Math.max(a[0], b[0], c[0]) - grid.minX) / cellX));
        const z0 = Math.max(0, Math.floor((Math.min(a[2], b[2], c[2]) - grid.minZ) / cellZ));
        const z1 = Math.min(grid.nz - 1, Math.floor((Math.max(a[2], b[2], c[2]) - grid.minZ) / cellZ));
        for (let iz = z0; iz <= z1; iz++) {
          const z = grid.minZ + (iz + 0.5) * cellZ;
          for (let ix = x0; ix <= x1; ix++) {
            const x = grid.minX + (ix + 0.5) * cellX;
            const l1 = ((b[2] - c[2]) * (x - c[0]) + (c[0] - b[0]) * (z - c[2])) / det;
            const l2 = ((c[2] - a[2]) * (x - c[0]) + (a[0] - c[0]) * (z - c[2])) / det;
            const l3 = 1 - l1 - l2;
            if (l1 < -0.01 || l2 < -0.01 || l3 < -0.01) continue;
            const y = Math.max(-127, Math.min(127, Math.round((l1 * a[1] + l2 * b[1] + l3 * c[1]) * 100)));
            const i = iz * grid.nx + ix;
            if (y > data[i]) data[i] = y;
          }
        }
      }
    }
  });
  // Close pinholes where a cell centre fell exactly between triangles.
  for (let pass = 0; pass < 2; pass++) {
    const copy = Int8Array.from(data);
    for (let iz = 0; iz < grid.nz; iz++) {
      for (let ix = 0; ix < grid.nx; ix++) {
        if (copy[iz * grid.nx + ix] !== NO_GROUND) continue;
        let sum = 0;
        let count = 0;
        for (let dz = -1; dz <= 1; dz++) {
          for (let dx = -1; dx <= 1; dx++) {
            const x = ix + dx;
            const z = iz + dz;
            if (x < 0 || z < 0 || x >= grid.nx || z >= grid.nz) continue;
            const value = copy[z * grid.nx + x];
            if (value === NO_GROUND) continue;
            sum += value;
            count++;
          }
        }
        if (count >= 5) data[iz * grid.nx + ix] = Math.round(sum / count);
      }
    }
  }
  return Buffer.from(data.buffer).toString('base64');
}

function boundsOf(node) {
  const { min, max } = getBounds(node);
  return [...min, ...max].map((n) => round(n));
}

function listModels(source) {
  const dir = path.join(source.local ? REPO : ROOT, source.dir);
  return fs
    .readdirSync(dir)
    .filter((f) => /\.(gltf|glb)$/.test(f) && !f.endsWith('.gltf.glb'))
    .map((f) => ({ name: f.replace(/\.(gltf|glb)$/, ''), file: path.join(dir, f) }))
    .filter(({ name }) => (!source.include || source.include.test(name)) && !(source.exclude && source.exclude.test(name)))
    .sort((a, b) => a.name.localeCompare(b.name))
    .map(({ name, file }) => ({ id: source.rename ? source.rename(name) : name, file }));
}

async function buildBundle(bundle) {
  const out = new Document();
  out.setLogger?.(quiet);
  const main = out.createScene(bundle.id);
  out.getRoot().setDefaultScene(main);
  const models = {};

  for (const source of bundle.sources) {
    for (const { id, file } of listModels(source)) {
      if (models[id]) throw new Error(`Duplicate model id ${id} in bundle ${bundle.id}`);
      const doc = await io.read(file);
      doc.setLogger?.(quiet);
      stripTextures(doc);
      const scene = doc.getRoot().getDefaultScene() ?? doc.getRoot().listScenes()[0];
      const wrapper = doc.createNode(MODEL_PREFIX + id);
      for (const child of scene.listChildren()) {
        scene.removeChild(child);
        wrapper.addChild(child);
      }
      scene.addChild(wrapper);
      models[id] = { bounds: boundsOf(wrapper) };
      if (bundle.id === 'hex' && id.startsWith('hex_')) tileHeights[id] = sampleHeights(wrapper, TILE_GRID);
      if (bundle.id === 'hex' && id === 'building_bridge_A') bridgeHeights = sampleHeights(wrapper, BRIDGE_GRID);
      mergeDocuments(out, doc);
    }
  }

  const [first, ...rest] = out.getRoot().listScenes();
  for (const scene of rest) {
    for (const child of scene.listChildren()) first.addChild(child);
    scene.dispose();
  }
  await out.transform(dedup(), prune({ keepLeaves: true, keepAttributes: true }), unpartition());

  const url = `assets/bundles/${bundle.id}.glb`;
  await io.write(path.join(REPO, 'public', url), out);
  return { url, models };
}

async function buildCharacter(character) {
  const doc = await io.read(path.join(ROOT, character.file));
  doc.setLogger?.(quiet);
  const material = doc.getRoot().listMaterials().find((m) => m.getBaseColorTexture());
  const texture = material ? textureId(material.getBaseColorTexture(), material) : null;
  stripTextures(doc);
  const scene = doc.getRoot().getDefaultScene() ?? doc.getRoot().listScenes()[0];
  const { max } = getBounds(scene);
  await doc.transform(prune({ keepLeaves: true, keepAttributes: true }), unpartition());
  const url = `assets/characters/${character.id}.glb`;
  await io.write(path.join(REPO, 'public', url), doc);
  const scale = character.scale ?? 1;
  return { url, texture, height: round(max[1] * scale, 2), rig: character.rig ?? 'medium', scale };
}

function stripMeshes(doc) {
  for (const node of doc.getRoot().listNodes()) {
    if (node.getMesh()) node.dispose();
  }
  for (const skin of doc.getRoot().listSkins()) skin.dispose();
}

function clipDuration(animation) {
  let duration = 0;
  for (const sampler of animation.listSamplers()) {
    const input = sampler.getInput();
    if (input) duration = Math.max(duration, input.getMax([])[0]);
  }
  return round(duration, 3);
}

function quatMultiply(a, b) {
  return [
    a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
    a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
    a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
    a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
  ];
}

function quatInverse(q) {
  return [-q[0], -q[1], -q[2], q[3]];
}

function nodesByName(doc) {
  return new Map(doc.getRoot().listNodes().map((n) => [n.getName(), n]));
}

/**
 * Copies `names` from a built rig onto this rig's skeleton. Each bone keeps its
 * own rest pose and length: a rotation key becomes rest(target) * rest(source)^-1
 * * key, translation keys are dropped except the hips, which are scaled by the
 * ratio of hip heights (or, for `seated` clips, so they stay at the same height
 * in the world once drawn at `drawnAt`), and scale keys are dropped.
 */
function borrowClips(out, source, names, { seated, drawnAt = 1 }) {
  const buffer = out.getRoot().listBuffers()[0] ?? out.createBuffer();
  const targetNodes = nodesByName(out);
  const sourceHips = source.nodes.get('hips').getTranslation()[1];
  const hipScale = targetNodes.get('hips').getTranslation()[1] / sourceHips;
  const accessor = (type, array) => out.createAccessor().setType(type).setArray(array).setBuffer(buffer);
  for (const animation of source.doc.getRoot().listAnimations()) {
    if (!names.includes(animation.getName())) continue;
    const copy = out.createAnimation(animation.getName());
    const hipFactor = seated?.test(animation.getName()) ? 1 / drawnAt : hipScale;
    for (const channel of animation.listChannels()) {
      const bone = channel.getTargetNode()?.getName();
      const target = targetNodes.get(bone);
      const pathName = channel.getTargetPath();
      if (!target) throw new Error(`No ${bone} bone to borrow ${animation.getName()} onto`);
      if (pathName === 'scale' || (pathName === 'translation' && bone !== 'hips')) continue;
      const sampler = channel.getSampler();
      const values = sampler.getOutput().getArray();
      const output = new Float32Array(values.length);
      if (pathName === 'rotation') {
        const fix = quatMultiply(target.getRotation(), quatInverse(source.nodes.get(bone).getRotation()));
        for (let i = 0; i < values.length; i += 4) {
          output.set(quatMultiply(fix, [values[i], values[i + 1], values[i + 2], values[i + 3]]), i);
        }
      } else {
        for (let i = 0; i < values.length; i++) output[i] = values[i] * hipFactor;
      }
      const copied = out
        .createAnimationSampler()
        .setInterpolation(sampler.getInterpolation())
        .setInput(accessor('SCALAR', Float32Array.from(sampler.getInput().getArray())))
        .setOutput(accessor(sampler.getOutput().getType(), output));
      copy.addSampler(copied).addChannel(
        out.createAnimationChannel().setTargetNode(target).setTargetPath(pathName).setSampler(copied),
      );
    }
  }
}

/** Builds one rig's clip file; `built` holds the rigs already built, for borrowing. */
async function buildRig(rig, built) {
  let out = null;
  let baseNodes = null;
  const clips = {};

  for (const { file, clips: wanted } of rig.files) {
    const doc = await io.read(path.join(ROOT, file));
    doc.setLogger?.(quiet);
    stripMeshes(doc);
    const rename = new Map(wanted.map((clip) => (Array.isArray(clip) ? clip : [clip, clip])));
    const found = new Set();
    for (const animation of doc.getRoot().listAnimations()) {
      const name = rename.get(animation.getName());
      if (name) {
        found.add(animation.getName());
        animation.setName(name);
        clips[name] = clipDuration(animation);
      } else {
        for (const channel of animation.listChannels()) channel.dispose();
        for (const sampler of animation.listSamplers()) sampler.dispose();
        animation.dispose();
      }
    }
    for (const name of rename.keys()) if (!found.has(name)) throw new Error(`Clip ${name} not found in ${file}`);

    if (!out) {
      out = doc;
      baseNodes = nodesByName(doc);
      continue;
    }
    mergeDocuments(out, doc);
    const base = new Set(baseNodes.values());
    for (const animation of out.getRoot().listAnimations()) {
      for (const channel of animation.listChannels()) {
        const target = channel.getTargetNode();
        if (!target || base.has(target)) continue;
        const replacement = baseNodes.get(target.getName());
        if (!replacement) throw new Error(`No base bone for ${target.getName()} (${file})`);
        channel.setTargetNode(replacement);
      }
    }
    for (const node of out.getRoot().listNodes()) if (!base.has(node)) node.dispose();
    const [, ...extraScenes] = out.getRoot().listScenes();
    for (const scene of extraScenes) scene.dispose();
  }

  if (rig.borrow) {
    const source = built.get(rig.borrow.from);
    if (!source) throw new Error(`Rig ${rig.id} borrows from ${rig.borrow.from}, which is not built yet`);
    const names = Object.keys(source.entry.clips).filter((name) => !(name in clips) && !rig.borrow.exclude?.test(name));
    borrowClips(out, source, names, rig.borrow);
    for (const name of names) clips[name] = source.entry.clips[name];
  }

  await out.transform(resample(), prune({ keepLeaves: true, keepAttributes: true }), dedup(), unpartition());
  const url = `assets/animations/rig-${rig.id}.glb`;
  await io.write(path.join(REPO, 'public', url), out);
  return { doc: out, nodes: nodesByName(out), entry: { url, clips } };
}

function fileSize(rel) {
  return fs.statSync(path.join(REPO, 'public', rel)).size;
}

async function main() {
  fs.rmSync(OUT, { recursive: true, force: true });
  for (const dir of ['bundles', 'characters', 'animations', 'textures']) {
    fs.mkdirSync(path.join(OUT, dir), { recursive: true });
  }

  const manifest = { textures: {}, bundles: {}, characters: {}, rigs: {} };
  let total = 0;

  for (const bundle of bundles) {
    manifest.bundles[bundle.id] = await buildBundle(bundle);
    const size = fileSize(manifest.bundles[bundle.id].url);
    total += size;
    console.log(`bundle ${bundle.id}: ${Object.keys(manifest.bundles[bundle.id].models).length} models, ${(size / 1024).toFixed(0)} KB`);
  }
  for (const character of characters) {
    const rig = rigs.find((r) => r.id === (character.rig ?? 'medium'));
    if (!rig) throw new Error(`${character.id}: no rig ${character.rig}`);
    if (rig.borrow?.drawnAt && rig.borrow.drawnAt !== (character.scale ?? 1)) {
      throw new Error(`${character.id} is drawn at ${character.scale ?? 1}, but rig ${rig.id} seats it for ${rig.borrow.drawnAt}`);
    }
    manifest.characters[character.id] = await buildCharacter(character);
    total += fileSize(manifest.characters[character.id].url);
  }
  console.log(`characters: ${characters.length}`);

  const built = new Map();
  for (const rig of rigs) {
    const result = await buildRig(rig, built);
    built.set(rig.id, result);
    manifest.rigs[rig.id] = result.entry;
    const size = fileSize(result.entry.url);
    total += size;
    console.log(`rig ${rig.id}: ${Object.keys(result.entry.clips).length} clips, ${(size / 1024).toFixed(0)} KB`);
  }

  for (const extra of extraTextures) textures.set(extra.id, fs.readFileSync(path.join(ROOT, extra.file)));
  for (const [id, bytes] of [...textures].sort(([a], [b]) => a.localeCompare(b))) {
    const url = `assets/textures/${id}.png`;
    fs.writeFileSync(path.join(REPO, 'public', url), bytes);
    manifest.textures[id] = url;
    total += bytes.length;
  }
  console.log(`textures: ${textures.size}`);

  fs.mkdirSync(path.dirname(MANIFEST), { recursive: true });
  fs.writeFileSync(
    MANIFEST,
    `// Generated by tools/assets/build-assets.mjs. Do not edit.\n` +
      `import type { AssetManifest } from '../core/assetTypes';\n\n` +
      `export const assetManifest: AssetManifest = ${JSON.stringify(manifest, null, 1)};\n`,
  );
  fs.writeFileSync(
    HEIGHTS,
    `// Generated by tools/assets/build-assets.mjs. Do not edit.\n` +
      `// Top-surface heights of each hex tile (int8 hundredths of a model unit, base64; ${NO_GROUND} = no ground).\n` +
      `export const TILE_GRID = ${JSON.stringify(TILE_GRID)};\n` +
      `export const BRIDGE_GRID = ${JSON.stringify(BRIDGE_GRID)};\n` +
      `export const NO_GROUND = ${NO_GROUND};\n` +
      `export const BRIDGE_HEIGHTS = ${JSON.stringify(bridgeHeights)};\n` +
      `export const TILE_HEIGHTS: Record<string, string> = ${JSON.stringify(tileHeights, null, 1)};\n`,
  );
  writeServerData(REPO, manifest, { TILE_GRID, BRIDGE_GRID, NO_GROUND, BRIDGE_HEIGHTS: bridgeHeights, TILE_HEIGHTS: tileHeights });
  console.log(`total ${(total / 1024 / 1024).toFixed(1)} MB -> public/assets`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
