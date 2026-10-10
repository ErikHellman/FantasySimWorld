import { ui } from './store';

/** What the valley is built with and how the parts work together, at a glance. Opened from the help window. */
export function AboutTechPanel() {
  const close = (): void => {
    ui.panel.value = 'none';
  };
  const back = (): void => {
    ui.panel.value = 'help';
  };
  return (
    <div class="modal" onClick={close}>
      <section class="panel dialog help" role="dialog" aria-label="About the tech" onClick={(event) => event.stopPropagation()}>
        <button class="close" title="Close (Esc)" onClick={close}>×</button>
        <h2>About the tech</h2>
        <p>
          A short tour of what keeps this valley going, from the server that runs the world to the models that give the
          villagers their minds.
        </p>

        <h3>One world, many viewers</h3>
        <p>
          The village lives on a server and runs around the clock, whether anyone is watching or not. Your browser never
          simulates anything: it receives a stream of what happens and draws it. That is why everyone sees the same
          world, and why it carries on when you close the tab.
        </p>

        <h3>The server</h3>
        <ul class="help-list">
          <li>
            <strong>Rust</strong>, on Tokio and Axum. The simulation moves the world forward 20 times a second: needs,
            routines, walking, conversations, fights (with blocks and dodges), fires and the weather. It is deterministic,
            so the same start and the same commands always give the same village.
          </li>
          <li><strong>SQLite</strong> holds the world, its history and the settings, with Litestream for backups.</li>
          <li><strong>Server-sent events</strong> carry the world to each browser as one long-lived stream.</li>
          <li>It all runs as one container on <strong>Fly.io</strong>, in Stockholm.</li>
        </ul>

        <h3>In your browser</h3>
        <ul class="help-list">
          <li><strong>TypeScript</strong>, built with Vite.</li>
          <li><strong>three.js</strong> draws the valley, with WebGPU where the browser has it and WebGL 2 where it does not.</li>
          <li><strong>Preact</strong> with signals for the panes, the chat and windows like this one.</li>
          <li>
            The 3D models are the <a href="https://kaylousberg.com" target="_blank" rel="noopener noreferrer">KayKit</a>{' '}
            collection by Kay Lousberg (CC0), with a few things made for the game in Blender in the same style, the
            animals and the dragon among them; the sounds and music are CC0, public domain or CC BY.
          </li>
        </ul>

        <h3>The villagers' minds</h3>
        <p>
          The villagers think with two AI models, both hosted by{' '}
          <a href="https://berget.ai" target="_blank" rel="noopener noreferrer">Berget AI</a>, which runs open models in
          Sweden.
        </p>
        <ul class="help-list">
          <li>
            <strong>A language model</strong> makes their choices. The server asks it only at turning points: a plan for
            the day on waking, what to say in a conversation, whether to fight or flee, what to make of something that
            just happened, and a reflection each night. It answers by picking from a fixed set of actions (tool calls)
            rather than in free text. It also writes each day's words for the billboards, from what has lately happened
            in the village.
          </li>
          <li>
            <strong>An embedding model</strong> turns each memory into a list of numbers that captures its meaning. When
            a villager has a decision to make, the server finds the memories closest in meaning to the moment, so a fire
            brings back the flames at the mill even when nobody said “fire”.
          </li>
        </ul>

        <h3>How it fits together</h3>
        <ol class="help-list">
          <li>The simulation runs the village tick by tick. Walking, working and fighting never wait for a model.</li>
          <li>When a villager reaches a decision, the server recalls what they remember about it and asks the language model what they do.</li>
          <li>
            The answer is checked against the world as it is by then and becomes a command, applied between two ticks. An
            answer that comes too late, asks for the impossible or never arrives is dropped, and the villager follows
            their routine instead.
          </li>
          <li>Everything that changes goes out on the stream, and your browser draws it.</li>
        </ol>
        <p class="muted">
          With no model connected (🧠 says “local minds”), the villagers live by their built-in routines alone, and the
          world goes on all the same.
        </p>

        <footer>
          <button onClick={back}>← Help</button>
          <button class="primary" onClick={close}>Close</button>
        </footer>
      </section>
    </div>
  );
}
