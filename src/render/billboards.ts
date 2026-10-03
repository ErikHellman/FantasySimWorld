import * as THREE from 'three/webgpu';
import type { Billboard as BoardContent, BillboardTone } from '../generated/api';
import { BILLBOARD_HEIGHT, BILLBOARD_LIFT, BILLBOARD_WIDTH } from '../world/placeDefs';
import type { Billboard } from '../world/places';
import { LINE_HEIGHT, layoutLines } from './billboardText';

/** Canvas pixels per metre of board face. */
const PIXELS_PER_METRE = 300;
/** How far the face leans back, so it reads from the usual camera angle above. */
const LEAN = 0.12;
const FONT = '"Georgia", "Palatino Linotype", "Book Antiqua", serif';

interface Palette {
  paper: string;
  ink: string;
  accent: string;
}

/** Colours by tone: dark boards for ominous notices, parchment for the rest. */
const PALETTES: Record<BillboardTone, Palette> = {
  uplifting: { paper: '#f6e7bf', ink: '#3a2412', accent: '#d9822b' },
  absurd: { paper: '#efe8d2', ink: '#2e2140', accent: '#7b4fb8' },
  pushy: { paper: '#fbefd2', ink: '#3b1410', accent: '#c0392b' },
  ominous: { paper: '#2a2a38', ink: '#e8e2cf', accent: '#7a8f6a' },
  wholesome: { paper: '#eef3dc', ink: '#1f3320', accent: '#3f8f4f' },
};

const wood = new THREE.MeshLambertMaterial({ color: 0x6b4a2f });
const darkWood = new THREE.MeshLambertMaterial({ color: 0x4a3220 });

/** One board: posts, frame and a painted face whose canvas is redrawn when the text changes. */
class BoardView {
  readonly root = new THREE.Group();
  private readonly canvas = document.createElement('canvas');
  private readonly texture: THREE.CanvasTexture;
  private shown = '';

  constructor(board: Billboard, groundAt: (x: number, z: number) => number) {
    const w = BILLBOARD_WIDTH;
    const h = BILLBOARD_HEIGHT;
    const postX = w / 2 - 0.35;
    const offset = (x: number) => ({ x: board.pos.x + Math.cos(board.facing) * x, z: board.pos.z - Math.sin(board.facing) * x });
    const left = offset(-postX);
    const right = offset(postX);
    // Stand on the lower of the two post feet; the posts reach a little into the ground on the higher side.
    const ground = Math.min(groundAt(left.x, left.z), groundAt(right.x, right.z), groundAt(board.pos.x, board.pos.z));
    this.root.position.set(board.pos.x, ground, board.pos.z);
    this.root.rotation.y = board.facing;

    const postHeight = BILLBOARD_LIFT + h + 0.55;
    for (const x of [-postX, postX]) {
      const post = new THREE.Mesh(new THREE.BoxGeometry(0.2, postHeight, 0.2), wood);
      post.position.set(x, postHeight / 2 - 0.3, -0.18);
      this.root.add(post);
    }

    const panel = new THREE.Group();
    panel.position.set(0, BILLBOARD_LIFT + h / 2, 0);
    panel.rotation.x = -LEAN;
    const back = new THREE.Mesh(new THREE.BoxGeometry(w + 0.24, h + 0.24, 0.12), darkWood);
    const roof = new THREE.Mesh(new THREE.BoxGeometry(w + 0.6, 0.12, 0.42), wood);
    roof.position.set(0, h / 2 + 0.2, 0.06);
    this.canvas.width = Math.round(w * PIXELS_PER_METRE);
    this.canvas.height = Math.round(h * PIXELS_PER_METRE);
    this.texture = new THREE.CanvasTexture(this.canvas);
    this.texture.colorSpace = THREE.SRGBColorSpace;
    this.texture.anisotropy = 4;
    const face = new THREE.Mesh(new THREE.PlaneGeometry(w, h), new THREE.MeshLambertMaterial({ map: this.texture }));
    face.position.z = 0.065;
    panel.add(back, roof, face);
    this.root.add(panel);

    this.root.traverse((object) => {
      const mesh = object as THREE.Mesh;
      if (mesh.isMesh) {
        mesh.castShadow = true;
        mesh.receiveShadow = true;
      }
    });
  }

  show(content: BoardContent | undefined, force = false): void {
    const key = content ? `${content.tone}|${content.text}` : '';
    if (key === this.shown && !force) return;
    this.shown = key;
    this.paint(content);
    this.texture.needsUpdate = true;
  }

  private paint(content: BoardContent | undefined): void {
    const ctx = this.canvas.getContext('2d');
    if (!ctx) return;
    const { width, height } = this.canvas;
    const palette = PALETTES[content?.tone ?? 'absurd'];
    ctx.fillStyle = palette.paper;
    ctx.fillRect(0, 0, width, height);
    // A painted border, and a thin line inside it.
    const border = 22;
    ctx.fillStyle = palette.accent;
    ctx.fillRect(0, 0, width, border);
    ctx.fillRect(0, height - border, width, border);
    ctx.fillRect(0, 0, border, height);
    ctx.fillRect(width - border, 0, border, height);
    ctx.strokeStyle = palette.accent;
    ctx.lineWidth = 4;
    ctx.strokeRect(border + 14, border + 14, width - 2 * (border + 14), height - 2 * (border + 14));
    if (!content) return;

    const pad = border + 44;
    const measure = (text: string, size: number): number => {
      ctx.font = `bold ${size}px ${FONT}`;
      return ctx.measureText(text).width;
    };
    const layout = layoutLines(content.text, measure, { width: width - 2 * pad, height: height - 2 * pad }, { max: 150, min: 44 });
    ctx.font = `bold ${layout.size}px ${FONT}`;
    ctx.fillStyle = palette.ink;
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    const step = layout.size * LINE_HEIGHT;
    const top = height / 2 - ((layout.lines.length - 1) * step) / 2;
    layout.lines.forEach((line, i) => ctx.fillText(line, width / 2, top + i * step));
  }
}

/** The village's billboards, placed from the map and painted with what the server says they show. */
export class Billboards {
  readonly group = new THREE.Group();
  private readonly views = new Map<string, BoardView>();
  private content = new Map<string, BoardContent>();

  constructor(boards: readonly Billboard[], groundAt: (x: number, z: number) => number) {
    for (const board of boards) {
      const view = new BoardView(board, groundAt);
      view.show(undefined);
      this.views.set(board.id, view);
      this.group.add(view.root);
    }
    // Repaint once web fonts have settled, in case the face was first painted with a fallback.
    void document.fonts?.ready.then(() => {
      for (const [id, view] of this.views) view.show(this.content.get(id), true);
    });
  }

  show(content: ReadonlyMap<string, BoardContent>): void {
    this.content = new Map(content);
    for (const [id, view] of this.views) view.show(content.get(id));
  }
}
