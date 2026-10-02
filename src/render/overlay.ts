import * as THREE from 'three/webgpu';
import type { Emote } from '../generated/api';

const EMOTE_GLYPH: Record<Emote, string> = {
  heart: '❤', anger: '💢', alert: '❗', thinking: '…', sleep: '💤', grief: '💧', laugh: '😄', broken_heart: '💔', gift: '🎁',
};

interface Label {
  root: HTMLDivElement;
  name: HTMLDivElement;
  bar: HTMLDivElement;
  fill: HTMLDivElement;
  bubble: HTMLDivElement;
  emote: HTMLDivElement;
  bubbleUntil: number;
  emoteUntil: number;
}

export interface LabelState {
  id: string;
  name: string;
  /** world position of the top of the head */
  x: number;
  y: number;
  z: number;
  visible: boolean;
  hp: number;
  /** show the health bar */
  hurt: boolean;
  hostile: boolean;
  selected: boolean;
}

/**
 * DOM labels floating over characters: name, health, speech bubbles, emotes.
 * All text goes in through textContent, never innerHTML, since much of it is
 * written by a language model.
 */
export class Overlay {
  private readonly labels = new Map<string, Label>();
  private readonly projected = new THREE.Vector3();
  /** names are shown when the camera is closer than this */
  nameDistance = 75;

  constructor(private readonly container: HTMLElement) {}

  private label(id: string): Label {
    let label = this.labels.get(id);
    if (!label) {
      const root = document.createElement('div');
      root.className = 'tag';
      const bubble = document.createElement('div');
      bubble.className = 'tag-bubble';
      const emote = document.createElement('div');
      emote.className = 'tag-emote';
      const name = document.createElement('div');
      name.className = 'tag-name';
      const bar = document.createElement('div');
      bar.className = 'tag-bar';
      const fill = document.createElement('div');
      fill.className = 'tag-fill';
      bar.append(fill);
      root.append(bubble, emote, name, bar);
      this.container.append(root);
      label = { root, name, bar, fill, bubble, emote, bubbleUntil: 0, emoteUntil: 0 };
      this.labels.set(id, label);
    }
    return label;
  }

  say(id: string, text: string, seconds: number, loud: boolean): void {
    const label = this.label(id);
    label.bubble.textContent = text;
    label.bubble.classList.toggle('loud', loud);
    label.bubbleUntil = performance.now() + seconds * 1000;
  }

  emote(id: string, emote: Emote): void {
    const label = this.label(id);
    label.emote.textContent = EMOTE_GLYPH[emote];
    label.emote.classList.remove('pop');
    void label.emote.offsetWidth; // restart the animation
    label.emote.classList.add('pop');
    label.emoteUntil = performance.now() + 2200;
  }

  /** Lifts speech bubbles that would cover one another, so a busy tavern stays readable. */
  private spreadBubbles(bubbles: Array<{ label: Label; x: number; y: number }>): void {
    if (bubbles.length === 0) return;
    // Lowest on screen first: those keep their place, and the ones behind are raised above them.
    bubbles.sort((a, b) => b.y - a.y);
    const placed: Array<{ left: number; right: number; top: number; bottom: number }> = [];
    for (const { label, x, y } of bubbles) {
      const width = label.bubble.offsetWidth;
      const height = label.bubble.offsetHeight;
      const below = label.root.offsetHeight - height;
      const left = x - width / 2;
      const right = x + width / 2;
      let bottom = y - below;
      for (let pass = 0; pass < placed.length; pass++) {
        const hit = placed.find((box) => left < box.right && right > box.left && bottom > box.top && bottom - height < box.bottom);
        if (!hit) break;
        bottom = hit.top - 4;
      }
      const lift = Math.min(260, y - below - bottom);
      label.bubble.style.transform = lift > 0.5 ? `translateY(${-lift.toFixed(0)}px)` : '';
      placed.push({ left, right, top: bottom - height, bottom });
    }
  }

  update(states: LabelState[], camera: THREE.Camera, cameraDistance: number, width: number, height: number): void {
    const now = performance.now();
    const seen = new Set<string>();
    const bubbles: Array<{ label: Label; x: number; y: number }> = [];
    for (const state of states) {
      seen.add(state.id);
      const label = this.label(state.id);
      this.projected.set(state.x, state.y, state.z).project(camera);
      const onScreen = state.visible && this.projected.z < 1 && Math.abs(this.projected.x) < 1.15 && Math.abs(this.projected.y) < 1.2;
      if (!onScreen) {
        label.root.style.display = 'none';
        continue;
      }
      const sx = (this.projected.x * 0.5 + 0.5) * width;
      const sy = (-this.projected.y * 0.5 + 0.5) * height;
      label.root.style.display = '';
      label.root.style.transform = `translate(${sx.toFixed(1)}px, ${sy.toFixed(1)}px) translate(-50%, -100%)`;
      label.root.classList.toggle('selected', state.selected);
      label.root.classList.toggle('hostile', state.hostile);

      const showName = state.selected || (cameraDistance < this.nameDistance && !state.hostile);
      label.name.style.display = showName ? '' : 'none';
      if (showName && label.name.textContent !== state.name) label.name.textContent = state.name;
      label.bar.style.display = state.hurt ? '' : 'none';
      if (state.hurt) label.fill.style.width = `${Math.round(Math.max(0, Math.min(1, state.hp)) * 100)}%`;
      const speaking = now < label.bubbleUntil;
      label.bubble.style.display = speaking ? '' : 'none';
      label.emote.style.display = now < label.emoteUntil ? '' : 'none';
      if (speaking) bubbles.push({ label, x: sx, y: sy });
    }
    this.spreadBubbles(bubbles);
    for (const [id, label] of this.labels) {
      if (!seen.has(id)) {
        label.root.remove();
        this.labels.delete(id);
      }
    }
  }
}
