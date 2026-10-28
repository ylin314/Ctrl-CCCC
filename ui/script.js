(() => {
  "use strict";

  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const root = document.getElementById("toast");
  const kind = document.getElementById("kind");
  const message = document.getElementById("message");
  const thumb = document.getElementById("thumb");
  const thumbnailDataUrls = new Map();
  let thumbGeneration = 0;
  let hideTimer = 0;
  let hideWindowTimer = 0;
  let hideGeneration = 0;

  async function place() {
    await invoke("place_toast", {
      availLeft: screen.availLeft ?? 0,
      availTop: screen.availTop ?? 0,
      availWidth: screen.availWidth,
      availHeight: screen.availHeight,
      contentHeight: Math.ceil(root.getBoundingClientRect().height),
    });
  }

  // Image snapshots are fetched on demand and cached per entry, mirroring the
  // history and quick-paste panels. A newer toast always wins, so a slow fetch
  // for an older entry can never overwrite the newest one.
  async function renderThumbnail(payload) {
    const generation = ++thumbGeneration;
    thumb.replaceChildren();
    if (!payload.image_id) return;

    const image = document.createElement("img");
    image.alt = payload.message;
    const drop = () => {
      image.remove();
    };
    image.addEventListener("error", drop);
    thumb.append(image);

    let dataUrl = thumbnailDataUrls.get(payload.image_id);
    if (!dataUrl) {
      try {
        dataUrl = await invoke("get_history_image", { id: payload.image_id });
      } catch (_) {
        if (generation === thumbGeneration) drop();
        return;
      }
      if (generation !== thumbGeneration) return;
      thumbnailDataUrls.set(payload.image_id, dataUrl);
    }

    image.src = dataUrl;
  }

  listen("clipboard://changed", async ({ payload }) => {
    kind.textContent = payload.kind;
    message.textContent = payload.message;
    await renderThumbnail(payload);
    root.style.setProperty("--fade", `${payload.fade_ms}ms`);

    // A newer toast supersedes any pending dismissal of the previous one.
    window.clearTimeout(hideWindowTimer);
    hideGeneration += 1;

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
      dismissWindow(payload.fade_ms);
    }, Math.max(0, payload.duration_ms - payload.fade_ms));
  });

  // The toast window is a real top-level window: fading it out with CSS only
  // leaves an invisible hit target that swallows every click and right-click
  // aimed at whatever sits underneath. Once the fade has finished, hide the
  // window itself so the pointer events reach the application below again.
  function dismissWindow(fadeMs) {
    const generation = ++hideGeneration;
    let finished = false;

    const finish = () => {
      if (finished || generation !== hideGeneration) {
        return;
      }
      finished = true;
      window.clearTimeout(hideWindowTimer);
      invoke("hide_toast").catch(() => {});
    };

    if (fadeMs > 0) {
      root.addEventListener(
        "transitionend",
        (event) => {
          if (event.target === root && event.propertyName === "opacity") {
            finish();
          }
        },
        { once: true }
      );
      // Safety net for browsers/backends that never report the transition.
      hideWindowTimer = window.setTimeout(finish, fadeMs + 400);
    } else {
      finish();
    }
  }
})();
