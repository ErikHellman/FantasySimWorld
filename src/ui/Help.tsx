import { Fragment, type ComponentChildren } from 'preact';
import { isTouchScreen } from './layout';
import { isAdmin, ui } from './store';

/** One control: the keys or mouse movement on the left, what it does on the right. */
function Control({ keys, children }: { keys: readonly string[]; children: ComponentChildren }) {
  return (
    <>
      <dt>
        {keys.map((key, index) => (
          <Fragment key={key}>
            {index > 0 && <span class="muted"> or </span>}
            <kbd>{key}</kbd>
          </Fragment>
        ))}
      </dt>
      <dd>{children}</dd>
    </>
  );
}

/** How to watch the village and what the things on screen mean. Opened from the top bar or with the ? key. */
export function HelpPanel() {
  const admin = isAdmin();
  const phone = ui.mobile.value;
  const close = (): void => {
    ui.panel.value = 'none';
  };
  const banner = (
    <li><strong>A banner</strong> across the top says when the alarm is up, a house is on fire, a wedding or a burial is coming or under way, and when the world has slowed to 1× while the villagers think.</li>
  );
  return (
    <div class="modal" onClick={close}>
      <section class="panel dialog help" role="dialog" aria-label="Help" onClick={(event) => event.stopPropagation()}>
        <button class="close" title="Close (Esc)" onClick={close}>×</button>
        <h2>How to watch the village</h2>
        <p>
          The village runs on the server around the clock, and everyone watching sees the same world. You look around,
          follow the villagers and talk with the other people watching.
        </p>

        {isTouchScreen() && (
          <>
            <h3>With your fingers</h3>
            <dl class="controls">
              <Control keys={['One-finger drag']}>Move around the valley</Control>
              <Control keys={['Pinch']}>Zoom in and out</Control>
              <Control keys={['Two-finger twist']}>Turn the view</Control>
              <Control keys={['Two fingers up or down']}>Tilt, from low over the ground to straight down</Control>
              <Control keys={['Tap']}>
                {phone
                  ? 'Tap a villager (or one of the dead), a name in the villager list or a line in the chronicle to follow them; tap the “Following” chip for their page. Tap a building or something the village built for its page'
                  : 'Tap a villager (or one of the dead), a name in the villager list or a line in the chronicle to follow them and open their page. Tap a building or something the village built for its page'}
              </Control>
            </dl>
          </>
        )}

        {!phone && (
          <>
            <h3>Moving the camera</h3>
            <dl class="controls">
              <Control keys={['Drag', 'W A S D', 'Arrow keys']}>Move around the valley</Control>
              <Control keys={['Scroll']}>Zoom in and out</Control>
              <Control keys={['Right-drag', 'Q / E']}>Turn the view</Control>
              <Control keys={['Shift + scroll', 'R / F']}>Tilt, from low over the ground to straight down (right-drag up and down does it too)</Control>
            </dl>

            <h3>Following someone</h3>
            <dl class="controls">
              <Control keys={['Click']}>Click a villager (or one of the dead, or an animal), a name in the villager list or a line in the chronicle to follow them and open their page</Control>
              <Control keys={['Click']}>Click a building, or something the village built, for its page: who lives and is there, the work on it and its history</Control>
              <Control keys={['Esc']}>Stop following and close the page (or close an open window)</Control>
            </dl>
          </>
        )}
        <p class="muted">The camera keeps following until you move it yourself.</p>

        {!phone && (
          <>
            <h3>Other keys</h3>
            <dl class="controls">
              <Control keys={['?', 'H']}>Open or close this help</Control>
              <Control keys={['Enter']}>Send a chat message (Shift + Enter for a new line)</Control>
              {admin && <Control keys={['Space']}>Pause or resume the world, for everyone</Control>}
              {admin && <Control keys={['1', '2', '3']}>Run the world at 1×, 2× or 4×, for everyone</Control>}
            </dl>
          </>
        )}

        <h3>On the screen</h3>
        {phone ? (
          <ul class="help-list">
            <li><strong>Top</strong>: the day and time, and 💀 when the dead are abroad.</li>
            <li><strong>Tabs</strong> along the bottom (down the side when the phone lies on its side): 👥 the villagers, 📜 the chronicle and what they have said, 💬 the chat, 👤 the villager you follow, and ☰ for how many are watching, the language model, the graveyard, settings and this help. One pane opens at a time; tap its tab again or tap the world to put it away.</li>
            <li><strong>Following</strong>: the chip above the tabs names the villager the camera follows. Tap it for their page, ✕ to let them go.</li>
            <li><strong>A villager's page</strong>: <em>About</em> has their story, wants, needs, what they carry and their gold; <em>Mind</em> their current thought and today's plan; <em>Bonds</em> how they feel about the others; <em>Memory</em> what they remember, with more dots for what mattered more.</li>
            <li><strong>One of the dead</strong>: follow it for a card with its health, what it is after, where it rose and who it has slain.</li>
            <li><strong>A ghost</strong>: follow it for a card with who it was, how they died and how many nights it has left.</li>
            <li><strong>An animal</strong>: follow it for a card with what it is, what it is doing, and whose it is or who is riding it.</li>
            <li><strong>A building's page</strong>: tap a building, or something the village built, for its condition, who lives and is there, the work on it and its history.</li>
            {banner}
          </ul>
        ) : (
        <ul class="help-list">
          <li><strong>Top bar</strong>: the day and time, how many people are watching 👁, villagers alive 👥, the dead abroad 💀, the graveyard 🪦 (click it to read the names), the stores of wood 🪵, stone 🪨 and ore ⛏ once the village has gathered any, and the village chest 🪙 with 🧺 what the market's stalls hold. 🧠 says whether the villagers think with a language model or run on their built-in routines.</li>
          <li><strong>Villagers</strong> (left): everyone alive, what they are doing, their health, ♥ for those with a partner, and a mark when they are fighting ⚔️, asleep 💤, talking 💬 or hurt 🩹. A 🧒 marks a child.</li>
          <li><strong>Chronicle</strong> (bottom left): the village's news, newest first: births, weddings, deaths, fires, fights and trades. <strong>Voices</strong> next to it shows what the villagers have said aloud.</li>
          <li><strong>A villager's page</strong> (right): <em>About</em> has their story, wants, needs, what they carry and their gold; <em>Mind</em> their current thought and today's plan; <em>Bonds</em> how they feel about the others; <em>Memory</em> what they remember, with more dots for what mattered more.</li>
          <li><strong>One of the dead</strong> (right, when you follow it): its health, what it is after, where and how it rose, who it once was, and how many villagers it has slain.</li>
          <li><strong>A ghost</strong> (right, when you follow it): who it was, how they died, what it is about and how many nights it has left.</li>
          <li><strong>An animal</strong> (right, when you follow it): what it is, what it is doing, and whose it is or who is riding it. Only pets and horses have names over their heads.</li>
          <li><strong>A building's page</strong> (right, when you click a building or something the village built): its condition, who lives and is there, the work on it and what the chronicle says about it.</li>
          {banner}
          <li><strong>Chat</strong> (bottom): talk with the other people watching. Click its title to open or fold it, ✕ to tuck it away. Your chat name is in ⚙ Settings, with the sound.</li>
        </ul>
        )}

        <h3>How the village works</h3>
        <ul class="help-list">
          <li>A day lasts 24 minutes at normal speed: one game minute is one second.</li>
          <li>Villagers keep to a plan for the day: work, meals, rest and time with others. They grow hungry, tired and lonely, and see to it.</li>
          <li>Children live here too. They play, get into mischief (a pinched apple, a tall tale, a wander toward the graveyard at dusk) and never fight. After about a month they come of age and take up a trade. Couples who stay together have more of them.</li>
          <li>Some villagers sing or play, mostly of an evening outside the tavern. Whoever is idle nearby stops to listen, and a good tune leaves them heartened for a few hours: slower to tire, quicker to heal and a little braver. A poor one draws heckles.</li>
          <li>They talk, and every conversation shifts how they feel about each other. Friendships, rivalries and love grow from it; grudges can end in a fight.</li>
          <li>Anyone who stands in a villager's way hears about it: politely from most, sharply from the hot-tempered, and with a shout from someone who already has a grudge. They step aside if they can. A sharp word is remembered on both sides.</li>
          <li>A couple who stay together are married on the green, with the whole village there and a feast at the tavern after, and they move in together. When someone dies, the village buries them in the graveyard the next morning, and everyone goes to the graveside and then to the wake.</li>
          <li>From dusk the dead rise from the old graveyard. Some villagers keep watch, and when the alarm is raised the fighters come running. The dead are dangerous, and anyone can lose their nerve in a fight and run for home; the timid do so more often. They go after grown villagers before children. Follow one to see what it is after.</li>
          <li>About one in three of those the dead cut down comes back as a ghost: the same person, pale and see-through, seen only after dark. For two or three nights it drifts between its grave, its home and the homes of those it loved, keeping them from their sleep. Whoever meets it in the dark runs, or stands their ground, or grieves if they loved them; its touch leaves them cold and weak but never kills. Nobody can fight a ghost. Then it finds rest and is seen no more.</li>
          <li>If every villager is asleep, the dead may set a house alight. Everyone is woken to carry water from the well or the lake. A house that cannot be saved burns down and is rebuilt over several days, and its people sleep by the campfire meanwhile.</li>
          <li>Animals live here too. Cows, sheep, goats and horses graze the meadow south of the windmill, pigs and hens keep to the farmyard and ducks to the lake shore, and birds hop about the green and fly over the fields; an owl keeps the glade at night. Farmers tend the herd for eggs and milk. About half the homes keep a dog or a cat, which follows its keeper about and waits at the door; its keeper names it, and when a keeper dies someone else in the house takes it in. A villager setting off on a long walk with a free horse nearby rides it there, faster, and the horse waits for them, then wanders home at nightfall. Animals run from the dead, and the dead leave them be.</li>
          <li>The valley changes. Woodcutters fell trees, which grow back in a couple of weeks, miners break rocks, and farmers mow and dig. The village carries wood, stone and ore to its stores, and spends them on rebuilding, on enlarging crowded cottages and on fences, benches and crates.</li>
          <li>Coin: the village chest pays villagers for what they put down at the stores and sell at the market, and they spend it there or on one another's goods. Once a day a trader buys the surplus and refills the chest. The dead drop coin and trinkets that villagers pick up, and a villager who has saved enough pays for a bigger cottage or something for the yard. A meal and a bed are always free.</li>
          <li>Five big billboards stand around the village, each with a new slogan every morning. Now and then a villager stops to read one and says what they think: some laugh, some scoff, and the gloomier notices unsettle the timid. Villagers take the boards to heart: one that urges them to pray, eat, rest, train, shop, get to work or be friendly sends some of its readers off to do just that, most often those it cheered.</li>
          <li>Some villagers have ways of their own: a drunk who picks fights, a gossip who makes up stories, a bully, a coward, someone who naps on the grass when there is work to do, and worse. Not everyone is what they seem.</li>
          <li>Death is for good: the fallen get a grave, and a ghost only lingers a few nights. When a bed stands empty, a newcomer sooner or later arrives on the road, with a story of their own.</li>
          <li>With a language model connected the villagers make their own choices and plans; without one they follow their routines. Either way the world goes on.</li>
        </ul>

        <footer>
          <button onClick={() => (ui.panel.value = 'tech')}>About the tech</button>
          <button class="primary" onClick={close}>Got it</button>
        </footer>
      </section>
    </div>
  );
}
