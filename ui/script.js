(() => {
  "use strict";

  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const root = document.getElementById("toast");
  const kind = document.getElementById("kind");
  const message = document.getElementById("message");
  let hideTimer = 0;

  async function place() {
    await invoke("place_toast", {
      availLeft: screen.availLeft ?? 0,
      availTop: screen.availTop ?? 0,
      availWidth: screen.availWidth,
      availHeight: screen.availHeight,
      contentHeight: Math.ceil(root.getBoundingClientRect().height),
    });
  }

  listen("clipboard://changed", async ({ payload }) => {
    kind.textContent = payload.kind;
    message.textContent = payload.message;
    root.style.setProperty("--fade", `${payload.fade_ms}ms`);

    await place();
    window.clearTimeout(hideTimer);
    root.dataset.state = "hidden";
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        root.dataset.state = "visible";
      });
    });

    hideTimer = window.setTimeout(() => {
      root.dataset.state = "hidden";
    }, Math.max(0, payload.duration_ms - payload.fade_ms));
  });
})();
