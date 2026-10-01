import type { ActivityId } from '../../world/places';
import type { AnimId } from './anims';
import type { ItemId, Needs } from '../state';

export interface ActivityDef {
  id: ActivityId;
  /** for prompts and the UI: "eat a meal at the tavern" */
  label: string;
  /** present participle for status lines: "eating" */
  verb: string;
  anim: AnimId;
  /** "bundle/model" held while doing it */
  tool?: string;
  /** default length in game minutes */
  minutes: readonly [number, number];
  /** change per game minute */
  needs?: Partial<Needs>;
  /** hp per game minute */
  heal?: number;
  /** item produced and how many game minutes of work one takes */
  yields?: { item: ItemId; everyMinutes: number };
  /** done indoors, out of sight */
  indoors?: boolean;
  kind: 'need' | 'work' | 'leisure' | 'duty';
}

const def = (d: ActivityDef): ActivityDef => d;

export const ACTIVITIES: Record<ActivityId, ActivityDef> = {
  eat: def({ id: 'eat', label: 'eat a meal at the tavern', verb: 'eating', anim: 'sit_chair', minutes: [22, 30], needs: { hunger: 3.4, social: 0.25 }, kind: 'need' }),
  drink: def({ id: 'drink', label: 'have a drink at the tavern', verb: 'drinking', anim: 'sit_chair', minutes: [40, 80], needs: { social: 0.45, energy: -0.02 }, kind: 'leisure' }),
  sleep: def({ id: 'sleep', label: 'sleep at home', verb: 'sleeping', anim: 'lie', minutes: [420, 480], needs: { energy: 0.24 }, heal: 0.22, indoors: true, kind: 'need' }),
  rest: def({ id: 'rest', label: 'sit and rest on a bench', verb: 'resting', anim: 'sit_chair', minutes: [25, 50], needs: { energy: 0.07, social: 0.08 }, heal: 0.05, kind: 'leisure' }),
  smith: def({ id: 'smith', label: 'work at the forge', verb: 'working the forge', anim: 'hammer', tool: 'tools/hammer', minutes: [90, 150], kind: 'work' }),
  farm: def({ id: 'farm', label: 'tend the vegetable plots at the farm', verb: 'farming', anim: 'dig', tool: 'tools/shovel', minutes: [90, 150], yields: { item: 'food', everyMinutes: 70 }, kind: 'work' }),
  chop_wood: def({ id: 'chop_wood', label: 'chop wood at the lumber camp', verb: 'chopping wood', anim: 'chop', tool: 'tools/axe', minutes: [80, 140], kind: 'work' }),
  fish: def({ id: 'fish', label: 'fish at the lake shore', verb: 'fishing', anim: 'fish', tool: 'tools/fishing_rod', minutes: [70, 130], yields: { item: 'food', everyMinutes: 60 }, needs: { energy: 0.02 }, kind: 'work' }),
  mine: def({ id: 'mine', label: 'dig for ore at the old mine', verb: 'mining', anim: 'pickaxe', tool: 'tools/pickaxe', minutes: [80, 140], kind: 'work' }),
  trade: def({ id: 'trade', label: 'tend a stall at the market', verb: 'tending a market stall', anim: 'work_a', minutes: [90, 150], needs: { social: 0.2 }, yields: { item: 'gift', everyMinutes: 120 }, kind: 'work' }),
  shop: def({ id: 'shop', label: 'browse the market for food or a small gift', verb: 'browsing the market', anim: 'interact', minutes: [12, 20], needs: { social: 0.15 }, kind: 'leisure' }),
  pray: def({ id: 'pray', label: 'pray or meditate in quiet', verb: 'praying', anim: 'sit_floor', minutes: [30, 55], kind: 'leisure' }),
  train: def({ id: 'train', label: 'practise fighting at the training yard', verb: 'training', anim: 'idle', minutes: [50, 90], needs: { energy: -0.04 }, kind: 'work' }),
  forage: def({ id: 'forage', label: 'gather herbs and flowers in the forest glade', verb: 'foraging', anim: 'pick_up', minutes: [60, 110], yields: { item: 'gift', everyMinutes: 55 }, kind: 'work' }),
  watch: def({ id: 'watch', label: 'stand watch at the watchtower', verb: 'standing watch', anim: 'guard', minutes: [120, 180], kind: 'duty' }),
  relax: def({ id: 'relax', label: 'sit by the campfire in the glade', verb: 'relaxing by the fire', anim: 'sit_floor', minutes: [40, 80], needs: { energy: 0.05, social: 0.12 }, kind: 'leisure' }),
  brew: def({ id: 'brew', label: 'brew potions at the herb table in the glade', verb: 'brewing potions', anim: 'work_c', minutes: [80, 130], yields: { item: 'potion', everyMinutes: 85 }, kind: 'work' }),
  mourn: def({ id: 'mourn', label: 'visit the graves', verb: 'mourning at the graves', anim: 'sit_floor', minutes: [25, 45], kind: 'leisure' }),
  stroll: def({ id: 'stroll', label: 'stroll around the village', verb: 'strolling', anim: 'walk', minutes: [25, 50], needs: { social: 0.05 }, kind: 'leisure' }),
};

export const ACTIVITY_IDS = Object.keys(ACTIVITIES) as ActivityId[];
export const ITEM_CAP = 3;
