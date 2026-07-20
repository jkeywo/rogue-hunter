// Rogue Hunter — browser bootstrap. All behaviour lives in the WASM client;
// this file only wires DOM events to it.

import init, { WebClient } from "./pkg/rh_web.js";

async function main() {
  await init();
  const client = new WebClient(Date.now());
  // Exposed for end-to-end tests and console diagnostics.
  window.__client = client;

  const canvas = document.getElementById("map");
  canvas.width = client.canvas_width();
  canvas.height = client.canvas_height();

  const render = () => client.render();

  document.addEventListener("keydown", (event) => {
    // Let the browser handle copy/paste and devtools shortcuts.
    if (event.ctrlKey || event.metaKey || event.altKey) {
      return;
    }
    if (client.handle_key(event.key, false)) {
      event.preventDefault();
    }
    render();
  });

  document.addEventListener("paste", (event) => {
    const text = event.clipboardData?.getData("text");
    if (text) {
      client.handle_paste(text);
      event.preventDefault();
      render();
    }
  });

  const canvasPoint = (event) => {
    const rect = canvas.getBoundingClientRect();
    return [event.clientX - rect.left, event.clientY - rect.top];
  };
  canvas.addEventListener("mousemove", (event) => {
    const [x, y] = canvasPoint(event);
    client.hover(x, y);
    render();
  });
  canvas.addEventListener("mouseleave", () => {
    client.hover_clear();
    render();
  });
  canvas.addEventListener("mousedown", (event) => {
    const [x, y] = canvasPoint(event);
    client.click(x, y);
    render();
    pumpWalk();
  });

  // A click-to-walk takes one step per tick rather than resolving inside a
  // single frame, which would read as a teleport and hide whatever the
  // hunter walked into.
  let walkTimer = null;
  function pumpWalk() {
    if (walkTimer !== null) return;
    if (!client.walking()) return;
    walkTimer = setInterval(() => {
      const more = client.step_walk();
      render();
      if (!more) {
        clearInterval(walkTimer);
        walkTimer = null;
      }
    }, 90);
  }

  // Hovering a menu row moves the highlight, so the detail pane follows the
  // pointer and confirming does what the highlight shows.
  document.addEventListener("mouseover", (event) => {
    const row = event.target.closest?.("[data-choice]");
    if (row && client.hover_row(Number(row.dataset.choice))) {
      // Only when the highlight actually moved: redrawing swaps the node out
      // from under the pointer, which would cancel the click being made on it.
      render();
    }
  });

  // Clicks on menus, splash options, and the copy button (delegated,
  // since panels re-render every frame).
  document.addEventListener("click", (event) => {
    const action = event.target.closest?.("[data-action]");
    if (action) {
      client.do_action(Number(action.dataset.action));
      render();
      return;
    }
    const choice = event.target.closest?.("[data-choice]");
    if (choice) {
      client.choose(Number(choice.dataset.choice));
      render();
      return;
    }
    if (event.target.id === "copy-code") {
      const code = client.share_code();
      if (code) {
        navigator.clipboard?.writeText(code);
        event.target.textContent = "Copied";
      }
    }
  });

  render();
}

main().catch((error) => {
  document.body.innerHTML =
    `<pre style="color:#ff5f5f">Rogue Hunter failed to start:\n${error}</pre>`;
});
