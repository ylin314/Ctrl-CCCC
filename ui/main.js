(() => {
  "use strict";

  const { invoke } = window.__TAURI__.core;
  const form = document.getElementById("settings");
  const position = document.getElementById("position");
  const duration = document.getElementById("duration");
  const fade = document.getElementById("fade");
  const autostart = document.getElementById("autostart");
  const status = document.getElementById("status");

  async function load() {
    try {
      const settings = await invoke("get_settings");
      position.value = settings.position;
      duration.value = settings.duration_ms;
      fade.value = settings.fade_ms;
      autostart.checked = settings.autostart;
    } catch (error) {
      status.textContent = `加载失败：${error}`;
    }
  }

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    status.textContent = "";

    try {
      const settings = await invoke("save_settings", {
        settings: {
          position: position.value,
          duration_ms: Number(duration.value),
          fade_ms: Number(fade.value),
          autostart: autostart.checked,
        },
      });

      position.value = settings.position;
      duration.value = settings.duration_ms;
      fade.value = settings.fade_ms;
      autostart.checked = settings.autostart;
      status.textContent = "已保存";
    } catch (error) {
      status.textContent = `保存失败：${error}`;
    }
  });

  load();
})();
