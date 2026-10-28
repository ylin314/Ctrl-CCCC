(() => {
  "use strict";

  const { invoke, convertFileSrc } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const form = document.getElementById("settings");
  const position = document.getElementById("position");
  const duration = document.getElementById("duration");
  const fade = document.getElementById("fade");
  const autostart = document.getElementById("autostart");
  const historyLimit = document.getElementById("history-limit");
  const quickHotkey = document.getElementById("quick-hotkey");
  const clearHotkeyButton = document.getElementById("clear-hotkey-button");
  const status = document.getElementById("status");
  const settingsView = document.getElementById("settings-view");
  const historyView = document.getElementById("history-view");
  const historyList = document.getElementById("history-list");
  const historyEmpty = document.getElementById("history-empty");
  let activeView = "settings";
  let hotkeyCaptureActive = false;

  async function loadSettings() {
    try {
      const settings = await invoke("get_settings");
      position.value = settings.position;
      duration.value = settings.duration_ms;
      fade.value = settings.fade_ms;
      autostart.checked = settings.autostart;
      historyLimit.value = settings.history_limit;
      quickHotkey.value = settings.quick_hotkey;
    } catch (error) {
      status.textContent = `加载失败：${error}`;
    }
  }

  async function loadHistory() {
    try {
      const entries = await invoke("get_history");
      historyEmpty.classList.toggle("hidden", entries.length > 0);
      historyList.replaceChildren(...entries.map((entry, index) => createHistoryItem(entry, index)));
    } catch (error) {
      historyEmpty.textContent = `历史加载失败：${error}`;
      historyEmpty.classList.remove("hidden");
    }
  }

  async function startHotkeyCapture() {
    if (hotkeyCaptureActive) return;
    quickHotkey.dataset.previousValue = quickHotkey.value;
    quickHotkey.placeholder = "正在暂停旧快捷键...";
    try {
      await invoke("set_quick_hotkey_capture", { paused: true });
      hotkeyCaptureActive = true;
      quickHotkey.dataset.capturing = "true";
      quickHotkey.placeholder = "请按下新快捷键，Esc 取消";
      status.textContent = "快捷键捕获中";
    } catch (error) {
      quickHotkey.placeholder = "点击后按下组合键";
      status.textContent = `快捷键捕获失败：${error}`;
    }
  }

  async function finishHotkeyCapture(restore = true) {
    if (!hotkeyCaptureActive) return;
    hotkeyCaptureActive = false;
    delete quickHotkey.dataset.capturing;
    quickHotkey.placeholder = "点击后按下组合键";
    if (restore && quickHotkey.dataset.previousValue) {
      quickHotkey.value = quickHotkey.dataset.previousValue;
    }
    delete quickHotkey.dataset.previousValue;
    status.textContent = "";
    try {
      await invoke("set_quick_hotkey_capture", { paused: false });
    } catch (error) {
      status.textContent = `恢复快捷键失败：${error}`;
    }
  }

  function formatTime(timestampMs) {
    return new Date(timestampMs).toLocaleString("zh-CN", { hour12: false });
  }

  function entryKindLabel(entry) {
    if (entry.kind === "image") return "图片";
    if (entry.kind === "files") return "文件";
    return "文本";
  }

  function createHistoryItem(entry) {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "history-item";
    item.title = "单击复制到当前剪贴板";

    const kind = document.createElement("span");
    kind.className = "kind";
    if (entry.image_thumbnail_path || entry.image_path) {
      loadImage(entry, kind);
    } else {
      kind.textContent = entry.kind === "image" ? "IMG" : entry.kind === "files" ? "FILE" : "TXT";
    }

    const main = document.createElement("div");
    main.className = "history-main";
    const preview = document.createElement("div");
    preview.className = "history-preview";
    preview.textContent = entry.preview;
    const time = document.createElement("div");
    time.className = "history-time";
    time.textContent = formatTime(entry.timestamp_ms);
    main.append(preview, time);

    const typeLabel = document.createElement("span");
    typeLabel.className = "history-kind";
    typeLabel.textContent = entryKindLabel(entry);

    item.append(kind, main, typeLabel);
    item.addEventListener("click", async () => {
      const originalText = typeLabel.textContent;
      try {
        await invoke("copy_history", { id: entry.id });
        typeLabel.textContent = "已复制";
        setTimeout(() => { typeLabel.textContent = originalText; }, 1200);
        await loadHistory();
      } catch (error) {
        typeLabel.textContent = "失败";
        setTimeout(() => { typeLabel.textContent = originalText; }, 1600);
      }
    });

    return item;
  }

  // Images are loaded directly from disk through the Tauri asset protocol.
  // No base64 encoding or IPC payload is needed; the webview fetches the
  // file itself, so memory usage scales with the rendered size only.
  // Prefer the downscaled thumbnail for list rendering.
  function loadImage(entry, kind) {
    const src = entry.image_thumbnail_path || entry.image_path;
    if (!src) { kind.textContent = "IMG"; return; }

    const image = document.createElement("img");
    image.alt = entry.preview;
    image.addEventListener("error", () => { image.remove(); kind.textContent = "IMG"; });
    image.src = convertFileSrc(src);
    kind.append(image);
  }

  function showSettings() {
    activeView = "settings";
    settingsView.classList.add("active");
    historyView.classList.remove("active");
  }

  function showHistory() {
    activeView = "history";
    historyView.classList.add("active");
    settingsView.classList.remove("active");
    loadHistory();
  }

  window.ctrlCCCC = { showHistory };

  document.getElementById("history-button").addEventListener("click", showHistory);
  document.getElementById("back-button").addEventListener("click", showSettings);

  quickHotkey.addEventListener("focus", startHotkeyCapture);

  quickHotkey.addEventListener("blur", () => {
    if (hotkeyCaptureActive) finishHotkeyCapture(true);
  });

  quickHotkey.addEventListener("keydown", (event) => {
    event.preventDefault();

    if (event.key === "Escape") {
      finishHotkeyCapture(true);
      quickHotkey.blur();
      return;
    }

    const isModifier = ["Meta", "Control", "Alt", "Shift"].includes(event.key);
    if (isModifier || !hotkeyCaptureActive) return;

    const parts = [];
    if (event.metaKey) parts.push("super");
    if (event.ctrlKey) parts.push("ctrl");
    if (event.altKey) parts.push("alt");
    if (event.shiftKey) parts.push("shift");
    parts.push(event.code);
    quickHotkey.value = parts.join("+");
  });

  clearHotkeyButton.addEventListener("mousedown", (event) => {
    event.preventDefault();
  });

  clearHotkeyButton.addEventListener("click", async () => {
    if (clearHotkeyButton.disabled) return;
    const previousHotkey = quickHotkey.value;
    clearHotkeyButton.disabled = true;
    hotkeyCaptureActive = false;
    delete quickHotkey.dataset.capturing;
    delete quickHotkey.dataset.previousValue;
    quickHotkey.value = "";
    quickHotkey.placeholder = "点击后按下组合键";
    status.textContent = "";

    try {
      const settings = await invoke("clear_quick_hotkey");
      quickHotkey.value = settings.quick_hotkey;
      status.textContent = "快捷输入已关闭";
    } catch (error) {
      quickHotkey.value = previousHotkey;
      status.textContent = `关闭快捷键失败：${error}`;
    } finally {
      clearHotkeyButton.disabled = false;
      quickHotkey.blur();
    }
  });

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    status.textContent = "";
    const wasCapturing = hotkeyCaptureActive;

    try {
      const settings = await invoke("save_settings", {
        settings: {
          position: position.value,
          duration_ms: Number(duration.value),
          fade_ms: Number(fade.value),
          autostart: autostart.checked,
          history_limit: Number(historyLimit.value),
          quick_hotkey: quickHotkey.value,
        },
      });

      position.value = settings.position;
      duration.value = settings.duration_ms;
      fade.value = settings.fade_ms;
      autostart.checked = settings.autostart;
      historyLimit.value = settings.history_limit;
      quickHotkey.value = settings.quick_hotkey;
      status.textContent = "已保存";

      if (wasCapturing) {
        hotkeyCaptureActive = false;
        delete quickHotkey.dataset.capturing;
        delete quickHotkey.dataset.previousValue;
        quickHotkey.placeholder = "点击后按下组合键";
      }
    } catch (error) {
      status.textContent = `保存失败：${error}`;
      if (wasCapturing) await finishHotkeyCapture(true);
    }
  });

  listen("clipboard://changed", () => {
    if (activeView === "history") loadHistory();
  });

  loadSettings();
})();
