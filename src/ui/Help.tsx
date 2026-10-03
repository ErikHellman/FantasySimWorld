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
  return (
    <div class="modal" onClick={close}>
      <section class="panel dialog help" role="dialog" aria-label="Help" onClick={(event) => event.stopPropagation()}>
        <button class="close" title="Close (Esc)" onClick={close}>×</button>
        <h2>How to watch the village</h2>
        <p>
          The village runs on the server around the clock, and everyone watching sees the same world. You cannot change
          what happens; you can only look around, follow the villagers and talk with the other people watching.
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
                  ? 'Tap a villager, a name in the villager list or a line in the chronicle to follow them; tap the “Following” chip for their page'
                  : 'Tap a villager, a name in the villager list or a line in the chronicle to follow them and open their page'}
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
              <Control keys={['Click']}>Click a villager, a name in the villager list or a line in the chronicle to follow them and open their page</Control>
              <Control keys={['Esc']}>Stop following and close their page (or close an open window)</Control>
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
            <li><strong>Top</strong>: the day and time, and 💀 when monsters are abroad.</li>
            <li><strong>Tabs</strong> along the bottom (down the side when the phone lies on its side): 👥 the villagers, 📜 the chronicle and what they have said, 💬 the chat, 👤 the villager you follow, and ☰ for how many are watching, the language model, the graveyard, settings and this help. One pane opens at a time; tap its tab again or tap the world to put it away.</li>
            <li><strong>Following</strong>: the chip above the tabs names the villager the camera follows. Tap it for their page, ✕ to let them go.</li>
            <li><strong>A villager's page</strong>: <em>About</em> has their story, wants, needs and what they carry; <em>Mind</em> their current thought and today's plan; <em>Bonds</em> how they feel about the others; <em>Memory</em> what they remember, with more dots for what mattered more.</li>
          </ul>
        ) : (
        <ul class="help-list">
          <li><strong>Top bar</strong>: the day and time, how many people are watching 👁, villagers alive 👥, monsters abroad 💀 and the graveyard 🪦 (click it to read the names). 🧠 says whether the villagers think with a language model or run on their built-in routines.</li>
          <li><strong>Villagers</strong> (left): everyone alive, what they are doing, their health, ♥ for those with a partner, and a mark when they are fighting ⚔️, asleep 💤, talking 💬 or hurt 🩹.</li>
          <li><strong>Chronicle</strong> (bottom left): the village's news, newest first. <strong>Voices</strong> next to it shows what the villagers have said aloud.</li>
          <li><strong>A villager's page</strong> (right): <em>About</em> has their story, wants, needs and what they carry; <em>Mind</em> their current thought and today's plan; <em>Bonds</em> how they feel about the others; <em>Memory</em> what they remember, with more dots for what mattered more.</li>
          <li><strong>Chat</strong> (bottom): talk with the other people watching. Click its title to open or fold it, ✕ to tuck it away. Your chat name is in ⚙ Settings, with the sound.</li>
        </ul>
        )}

        <h3>How the village works</h3>
        <ul class="help-list">
          <li>A day lasts 24 minutes at normal speed: one game minute is one second.</li>
          <li>Villagers keep to a plan for the day: work, meals, rest and time with others. They grow hungry, tired and lonely, and see to it.</li>
          <li>Five big billboards stand around the village, each with a new slogan every morning. Now and then a villager stops to read one and says what they think: some laugh, some scoff, and the gloomier notices unsettle the timid.</li>
          <li>They talk, and every conversation shifts how they feel about each other. Friendships, rivalries and love grow from it; grudges can end in a fight.</li>
          <li>From dusk the dead rise from the old graveyard. Some villagers keep watch, and when the alarm is raised the fighters come running. The dead are dangerous, and anyone can lose their nerve in a fight and run for home; the timid do so more often.</li>
          <li>Death is for good: the fallen get a grave, and now and then a newcomer arrives in the valley.</li>
          <li>With a language model connected the villagers make their own choices and plans; without one they follow their routines. Either way the world goes on.</li>
        </ul>

        <footer>
          <button class="primary" onClick={close}>Got it</button>
        </footer>
      </section>
    </div>
  );
}
