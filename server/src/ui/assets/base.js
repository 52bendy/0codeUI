(function () {
  "use strict";

  var STORAGE_KEY = "intimeui.theme";
  var root = document.documentElement;
  var toggle = document.getElementById("theme-toggle");
  var toast = document.getElementById("toast");
  var toastTimer = 0;

  function readStoredTheme() {
    try {
      var value = window.localStorage.getItem(STORAGE_KEY);
      return value === "light" || value === "dark" ? value : null;
    } catch (error) {
      return null;
    }
  }

  function storeTheme(theme) {
    try {
      window.localStorage.setItem(STORAGE_KEY, theme);
    } catch (error) {
      /* 存储被禁用时仅当前页面生效，不影响交互。 */
    }
  }

  function preferredTheme() {
    if (window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches) {
      return "dark";
    }
    return "light";
  }

  function currentTheme() {
    var explicit = root.getAttribute("data-theme");
    return explicit === "light" || explicit === "dark" ? explicit : preferredTheme();
  }

  function syncToggle(theme) {
    if (!toggle) {
      return;
    }
    toggle.setAttribute("aria-pressed", theme === "dark" ? "true" : "false");
    toggle.setAttribute(
      "aria-label",
      theme === "dark" ? "切换到明亮主题" : "切换到暗色主题"
    );
  }

  function applyTheme(theme, persist) {
    root.setAttribute("data-theme", theme);
    syncToggle(theme);
    if (persist) {
      storeTheme(theme);
    }
  }

  function showToast(message) {
    if (!toast || !message) {
      return;
    }
    toast.textContent = message;
    toast.classList.add("is-visible");
    window.clearTimeout(toastTimer);
    toastTimer = window.setTimeout(function () {
      toast.classList.remove("is-visible");
    }, 2200);
  }

  function copyText(text) {
    if (!text) {
      return Promise.resolve(false);
    }
    if (navigator.clipboard && typeof navigator.clipboard.writeText === "function") {
      return navigator.clipboard.writeText(text).then(
        function () {
          return true;
        },
        function () {
          return fallbackCopy(text);
        }
      );
    }
    return Promise.resolve(fallbackCopy(text));
  }

  function fallbackCopy(text) {
    var area = document.createElement("textarea");
    area.value = text;
    area.setAttribute("readonly", "");
    area.style.position = "fixed";
    area.style.opacity = "0";
    document.body.appendChild(area);
    area.select();
    var copied = false;
    try {
      copied = document.execCommand("copy");
    } catch (error) {
      copied = false;
    }
    document.body.removeChild(area);
    return copied;
  }

  applyTheme(readStoredTheme() || currentTheme(), false);

  if (toggle) {
    toggle.addEventListener("click", function () {
      var next = currentTheme() === "dark" ? "light" : "dark";
      applyTheme(next, true);
    });
  }

  document.addEventListener("click", function (event) {
    var target = event.target;
    if (!target || !target.closest) {
      return;
    }
    var copyButton = target.closest("[data-copy]");
    if (copyButton) {
      var sourceId = copyButton.getAttribute("data-copy");
      var source = sourceId ? document.getElementById(sourceId) : null;
      copyText(source ? source.textContent.trim() : "").then(function (copied) {
        showToast(copied ? "已复制" : "复制失败，请手动选择");
      });
      return;
    }
    var demoButton = target.closest("[data-demo-action]");
    if (demoButton) {
      showToast(demoButton.getAttribute("data-demo-action"));
    }
  });
})();
