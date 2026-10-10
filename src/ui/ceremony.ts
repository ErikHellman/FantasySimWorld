import type { Ceremony } from '../generated/api';

function joined(names: readonly string[]): string {
  if (names.length <= 1) return names[0] ?? 'the dead';
  return `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`;
}

/** The banner for the wedding, funeral or trial the village has arranged, in its current phase. */
export function ceremonyBanner(ceremony: Ceremony): string {
  const names = joined(ceremony.names);
  const place = ceremony.placeName;
  if (ceremony.kind === 'trial') {
    const accused = ceremony.names[0] ?? 'someone';
    switch (ceremony.phase) {
      case 'announced': return `⚖️ ${accused} is to be tried on ${place} at ${ceremony.time}`;
      case 'gathering':
      case 'rite': return `⚖️ ${accused} is on trial on ${place}`;
      default: return `⚖️ The trial of ${accused} is over`;
    }
  }
  if (ceremony.kind === 'wedding') {
    switch (ceremony.phase) {
      case 'announced': return `💍 ${names} marry on ${place} at ${ceremony.time}`;
      case 'gathering':
      case 'rite': return `💍 ${names} are being married on ${place}`;
      default: return `🍻 The wedding feast for ${names} is on at the tavern`;
    }
  }
  switch (ceremony.phase) {
    case 'announced': return `🕯️ The village buries ${names} at ${ceremony.time}`;
    case 'gathering':
    case 'rite': return `🕯️ The village is burying ${names}`;
    default: return `🍻 The wake for ${names} is at the tavern`;
  }
}
