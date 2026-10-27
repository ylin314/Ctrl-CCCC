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

  async function loadImage(entry, image) {
    if (imageDataUrls.has(entry.id)) {
      image.src = imageDataUrls.get(entry.id);
      return;
    }
    try {
      const dataUrl = await invoke("get_history_image", { id: entry.id });
      imageDataUrls.set(entry.id, dataUrl);
      image.src = dataUrl;
    } catch (_) {
      image.remove();
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
      const image = document.createElement("img");
      image.alt = entry.preview;
      loadImage(entry, image);
      kind.append(image);
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
  quickTitle.addEventListener("mousedown", (event) => {
    if (event.button === 0) {
      invoke("set_quick_paste_dragging", { dragging: true }).catch(() => {});
    }
  });
  window.addEventListener("mouseup", () => {
    invoke("set_quick_paste_dragging", { dragging: false }).catch(() => {});
  });

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

