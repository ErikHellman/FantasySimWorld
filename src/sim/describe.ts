import { findPlace, getSlot } from '../world/places';
import { ACTIVITIES } from './data/activities';
import { MONSTERS } from './data/monsters';
import type { Character, Monster, SimState } from './state';
import { nameOf } from './util';

/** What a character is doing right now, in a few words ("eating at The Gilded Tankard"). */
export function describeActivity(state: SimState, c: Character): string {
  if (c.combat.yielded) return 'beaten, at someone\'s mercy';
  if (c.convoId) {
    const convo = state.conversations[c.convoId];
    const otherId = convo ? (convo.a === c.id ? convo.b : convo.a) : null;
    return otherId ? `talking with ${nameOf(state, otherId)}` : 'talking';
  }
  const intent = c.intent;
  if (!intent) return c.asleep ? 'sleeping' : 'standing about';
  switch (intent.kind) {
    case 'go_to':
      return `walking to ${findPlace(intent.placeId)?.name ?? 'somewhere'}`;
    case 'activity': {
      const def = ACTIVITIES[intent.activity];
      if (intent.activity === 'sleep') return c.asleep ? 'asleep at home' : 'heading home to sleep';
      if (intent.activity === 'stroll') return 'strolling around the village';
      const place = c.slotId ? findPlace(getSlot(c.slotId).placeId) : null;
      if (c.act.phase !== 'perform') return place ? `on the way to ${place.name} to ${shortLabel(def.label)}` : `going to ${shortLabel(def.label)}`;
      return place ? `${def.verb} at ${place.name}` : def.verb;
    }
    case 'talk':
      return `going to talk to ${nameOf(state, intent.targetId)}`;
    case 'give':
      return `taking something to ${nameOf(state, intent.targetId)}`;
    case 'fight': {
      if (intent.mode === 'monster') {
        const monster: Monster | undefined = state.monsters[intent.targetId];
        return monster ? `fighting a ${MONSTERS[monster.kind].label}` : 'fighting';
      }
      return intent.mode === 'brawl' ? `brawling with ${nameOf(state, intent.targetId)}` : `attacking ${nameOf(state, intent.targetId)}`;
    }
    case 'flee':
      return c.inside ? 'hiding indoors' : 'running for safety';
    case 'wait':
      return c.anim === 'sit_floor' ? 'sitting on the ground, recovering' : 'pausing a moment';
  }
}

function shortLabel(label: string): string {
  return label.replace(/ at the .*$| in the .*$/, '');
}
