(() => {
  "use strict";

  const invoke = window.__TAURI__?.core?.invoke;
  const listen = window.__TAURI__?.event?.listen;
  const list = document.getElementById("quick-list");
  const empty = document.getElementById("quick-empty");
  const imageDataUrls = new Map();

  if (!invoke) {
    empty.textContent = "Tauri API 不可用";
    empty.classList.remove("hidden");
    return;
  }

  function entryKindLabel(entry) {
    if (entry.kind === "image") return "图片";
    if (entry.kind === "files") return "文件";
    return "输入";
  }

  // Image snapshots live on disk and are handed back as `data:image/png` URLs.
  // If a snapshot cannot be rendered (missing file, blocked CSP, decode error)
  // degrade to the plain "IMG" badge instead of a broken thumbnail.
  async function loadImage(entry, kind) {
    const fallback = () => {
      image.remove();
      kind.textContent = "IMG";
    };

    const image = document.createElement("img");
    image.alt = entry.preview;
    image.addEventListener("error", fallback);
    kind.append(image);

    if (imageDataUrls.has(entry.id)) {
      image.src = imageDataUrls.get(entry.id);
      return;
    }

    try {
      const dataUrl = await invoke("get_history_image", { id: entry.id });
      imageDataUrls.set(entry.id, dataUrl);
      image.src = dataUrl;
    } catch (_) {
      fallback();
    }
  }

  function createItem(entry) {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "quick-item";
    item.title = "单击输入到当前位置";

    const kind = document.createElement("span");
    kind.className = "kind";
    if (entry.image_path) {
      loadImage(entry, kind);
    } else {
      kind.textContent = entry.kind === "image" ? "IMG" : entry.kind === "files" ? "FILE" : "TXT";
    }

    const preview = document.createElement("div");
    preview.className = "preview";
    preview.textContent = entry.preview;

    const label = document.createElement("span");
    label.className = "type-label";
    label.textContent = entryKindLabel(entry);

    item.append(kind, preview, label);
    item.addEventListener("click", async () => {
      item.disabled = true;
      label.textContent = "输入中";
      try {
        await invoke("quick_paste", { id: entry.id });
      } catch (error) {
        label.textContent = "失败";
        setTimeout(() => { label.textContent = entryKindLabel(entry); }, 1200);
        item.disabled = false;
      }
    });

    return item;
  }

  async function refresh() {
    try {
      const entries = await invoke("get_history");
      empty.classList.toggle("hidden", entries.length > 0);
      list.replaceChildren(...entries.map(createItem));
    } catch (error) {
      empty.textContent = `历史加载失败：${error}`;
      empty.classList.remove("hidden");
    }
  }

  const quickTitle = document.getElementById("quick-title");
  let dragPointerId = null;
  let dragStartX = 0;
  let dragStartY = 0;
  let dragRequested = false;

  function resetDragTracking(pointerId) {
    if (dragPointerId !== null && pointerId !== dragPointerId) return;
    dragPointerId = null;
    dragRequested = false;
  }

  // A short threshold keeps an ordinary click on the header from entering the
  // OS drag loop. Once exceeded, `begin_quick_paste_drag` sets the Rust-side
  // dragging flag *before* native dragging starts, avoiding the focus-loss race.
  quickTitle.addEventListener("pointerdown", (event) => {
    if (event.button !== 0 || !(event.target instanceof Element)) return;
    if (event.target.closest("#close-button")) return;

    dragPointerId = event.pointerId;
    dragStartX = event.clientX;
    dragStartY = event.clientY;
    dragRequested = false;
    quickTitle.setPointerCapture(event.pointerId);
  });

  quickTitle.addEventListener("pointermove", (event) => {
    if (dragRequested || event.pointerId !== dragPointerId || (event.buttons & 1) === 0) {
      return;
    }

    const dragDistance = Math.hypot(event.clientX - dragStartX, event.clientY - dragStartY);
    if (dragDistance < 4) return;

    dragRequested = true;
    dragPointerId = null;
    quickTitle.releasePointerCapture(event.pointerId);
    invoke("begin_quick_paste_drag").catch(() => {
      dragRequested = false;
      invoke("set_quick_paste_dragging", { dragging: false }).catch(() => {});
    });
  });

  const stopDrag = (event) => {
    const wasTracked = event.pointerId === dragPointerId;
    const shouldClear = dragRequested || wasTracked;
    if (wasTracked) dragPointerId = null;
    dragRequested = false;
    if (shouldClear) {
      invoke("set_quick_paste_dragging", { dragging: false }).catch(() => {});
    }
  };

  quickTitle.addEventListener("pointerup", stopDrag);
  quickTitle.addEventListener("pointercancel", stopDrag);

  document.getElementById("close-button").addEventListener("click", async () => {
    await invoke("hide_quick_paste");
  });

  document.addEventListener("keydown", async (event) => {
    if (event.key === "Escape") await invoke("hide_quick_paste");
  });

  if (listen) {
    listen("clipboard://changed", refresh).catch(() => {});
  }

  window.ctrlCCCC = { refresh };
  refresh();
})();

