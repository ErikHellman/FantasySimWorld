// Builds public/assets from the KayKit collection.
//
//   KAYKIT_DIR="/path/to/The Complete KayKit Collection v7" pnpm assets
//
// - Environment models are merged into one GLB per bundle (one named node per model).
// - Textures are stripped from every model and written once to public/assets/textures;
//   each material keeps the texture id in extras.tex so the runtime can share materials.
// - Animation files lose their mannequin meshes and are merged onto a single skeleton.
// - src/generated/assetManifest.ts lists everything, with bounds and clip durations.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { Document, NodeIO } from '@gltf-transform/core';
import { ALL_EXTENSIONS } from '@gltf-transform/extensions';
import { dedup, getBounds, mergeDocuments, prune, resample, unpartition } from '@gltf-transform/functions';
import { animations, bundles, characters, extraTextures } from './asset-list.mjs';

const ROOT = process.env.KAYKIT_DIR ?? path.join(os.homedir(), 'Documents/The Complete KayKit Collection v7');
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const OUT = path.join(REPO, 'public/assets');
const MANIFEST = path.join(REPO, 'src/generated/assetManifest.ts');
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

function boundsOf(node) {
  const { min, max } = getBounds(node);
  return [...min, ...max].map((n) => round(n));
}

function listModels(source) {
  const dir = path.join(ROOT, source.dir);
  return fs
    .readdirSync(dir)
    .filter((f) => f.endsWith('.gltf'))
    .map((f) => f.replace(/\.gltf$/, ''))
    .filter((name) => (!source.include || source.include.test(name)) && !(source.exclude && source.exclude.test(name)))
    .sort()
    .map((name) => ({ id: source.rename ? source.rename(name) : name, file: path.join(dir, `${name}.gltf`) }));
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
  return { url, texture, height: round(max[1], 2) };
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

async function buildAnimations() {
  let out = null;
  let baseNodes = null;
  const clips = {};

  for (const { file, clips: wanted } of animations.files) {
    const doc = await io.read(path.join(ROOT, file));
    doc.setLogger?.(quiet);
    stripMeshes(doc);
    const found = new Set();
    for (const animation of doc.getRoot().listAnimations()) {
      const name = animation.getName();
      if (wanted.includes(name)) {
        found.add(name);
        clips[name] = clipDuration(animation);
      } else {
        for (const channel of animation.listChannels()) channel.dispose();
        for (const sampler of animation.listSamplers()) sampler.dispose();
        animation.dispose();
      }
    }
    for (const name of wanted) if (!found.has(name)) throw new Error(`Clip ${name} not found in ${file}`);

    if (!out) {
      out = doc;
      baseNodes = new Map(doc.getRoot().listNodes().map((n) => [n.getName(), n]));
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

  await out.transform(resample(), prune({ keepLeaves: true, keepAttributes: true }), dedup(), unpartition());
  const url = 'assets/animations/rig-medium.glb';
  await io.write(path.join(REPO, 'public', url), out);
  return { url, clips };
}

function fileSize(rel) {
  return fs.statSync(path.join(REPO, 'public', rel)).size;
}

async function main() {
  fs.rmSync(OUT, { recursive: true, force: true });
  for (const dir of ['bundles', 'characters', 'animations', 'textures']) {
    fs.mkdirSync(path.join(OUT, dir), { recursive: true });
  }

  const manifest = { textures: {}, bundles: {}, characters: {}, animations: null };
  let total = 0;

  for (const bundle of bundles) {
    manifest.bundles[bundle.id] = await buildBundle(bundle);
    const size = fileSize(manifest.bundles[bundle.id].url);
    total += size;
    console.log(`bundle ${bundle.id}: ${Object.keys(manifest.bundles[bundle.id].models).length} models, ${(size / 1024).toFixed(0)} KB`);
  }
  for (const character of characters) {
    manifest.characters[character.id] = await buildCharacter(character);
    total += fileSize(manifest.characters[character.id].url);
  }
  console.log(`characters: ${characters.length}`);

  manifest.animations = await buildAnimations();
  const animSize = fileSize(manifest.animations.url);
  total += animSize;
  console.log(`animations: ${Object.keys(manifest.animations.clips).length} clips, ${(animSize / 1024).toFixed(0)} KB`);

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
  console.log(`total ${(total / 1024 / 1024).toFixed(1)} MB -> public/assets`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
