import { ui } from './store';

/** The health of a boss near the camera, across the top of the screen: the whole village is fighting it. */
export function BossBar() {
  const boss = ui.boss.value;
  if (!boss) return null;
  const share = boss.maxHp > 0 ? Math.max(0, Math.min(1, boss.hp / boss.maxHp)) : 0;
  return (
    <div class="boss-bar" role="status" aria-label={`${boss.name}: ${Math.ceil(boss.hp)} of ${boss.maxHp}`}>
      <span class="boss-name">{boss.name.charAt(0).toUpperCase() + boss.name.slice(1)}</span>
      <div class="boss-meter"><div class="boss-fill" style={{ width: `${(share * 100).toFixed(1)}%` }} /></div>
    </div>
  );
}
