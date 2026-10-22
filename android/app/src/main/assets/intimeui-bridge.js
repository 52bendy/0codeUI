(function () {
  "use strict";
  if (window.__INTIMEUI_SDK_LOADED__) {
    return;
  }
  window.__INTIMEUI_SDK_LOADED__ = true;

  var nativeBridge = window.IntimeUINative;
  var sequence = 0;
  var readySent = false;
  var lastScrollDepth = 0;

  function post(type, payload) {
    if (!nativeBridge || typeof nativeBridge.postMessage !== "function") {
      return null;
    }
    var message = {
      v: 1,
      id: "m_" + (++sequence) + "_" + Date.now().toString(36),
      type: type,
      payload: payload || {}
    };
    try {
      var raw = nativeBridge.postMessage(JSON.stringify(message));
      return raw ? JSON.parse(raw) : null;
    } catch (error) {
      return null;
    }
  }

  function isInteractive(node) {
    return !!(node && node.closest &&
      node.closest('a,button,[role="button"],[data-intimeui-action],input,select,textarea,label'));
  }

  function readScrollDepth() {
    var doc = document.documentElement;
    var max = (doc.scrollHeight || 0) - (window.innerHeight || 0);
    if (max <= 0) {
      return 0;
    }
    var offset = window.scrollY || doc.scrollTop || 0;
    return Math.min(1, Math.max(0, offset / max));
  }

  function sendTelemetry(kind, extra) {
    var payload = extra || {};
    payload.kind = kind;
    post("TELEMETRY_SIGNAL", payload);
  }

  function sendReady() {
    if (readySent) {
      return;
    }
    readySent = true;
    var root = document.documentElement;
    post("RENDER_READY", {
      page: (root && root.dataset.intimeuiPage) || "UNKNOWN",
      scrollDepth: readScrollDepth()
    });
  }

  function pageContext() {
    var root = document.documentElement;
    return {
      page: (root && root.dataset.intimeuiPage) || "UNKNOWN",
      entity: (root && root.dataset.intimeuiEntity) || ""
    };
  }

  document.addEventListener("click", function (event) {
    sendTelemetry("TAP", {interactive: isInteractive(event.target)});
  }, true);

  window.addEventListener("scroll", function () {
    lastScrollDepth = readScrollDepth();
    sendTelemetry("SCROLL", {depth: lastScrollDepth});
  }, {passive: true});

  document.addEventListener("visibilitychange", function () {
    if (document.visibilityState === "hidden") {
      post("PAGE_HIDE", {depth: lastScrollDepth});
    }
  });
  window.addEventListener("pagehide", function () {
    post("PAGE_HIDE", {depth: lastScrollDepth});
  });

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", sendReady);
  } else {
    sendReady();
  }

  var api = window.IntimeUI || {};
  api.version = "1.0.0";
  api.triggerIntent = function (action, data, target) {
    var context = pageContext();
    return post("INTENT_TRIGGER", {
      action: action,
      data: data || {},
      target: target || {},
      page: context.page,
      entity: context.entity
    });
  };
  api.setTheme = function (theme) {
    post("SET_THEME", {theme: theme});
  };
  if (typeof api.onConfirmation !== "function") {
    api.onConfirmation = null;
  }
  if (typeof api.onReceipt !== "function") {
    api.onReceipt = null;
  }
  if (typeof api.onIntentResult !== "function") {
    api.onIntentResult = null;
  }
  if (typeof api.onIntentError !== "function") {
    api.onIntentError = null;
  }
  window.IntimeUI = api;

  window.__INTIMEUI_BRIDGE__ = {
    receive: function (message) {
      var msg = typeof message === "string" ? JSON.parse(message) : message;
      if (!msg || !msg.type) {
        return;
      }
      var payload = msg.payload || {};
      if (msg.type === "CONFIRMATION_CARD" && typeof window.IntimeUI.onConfirmation === "function") {
        window.IntimeUI.onConfirmation(payload);
      } else if (msg.type === "EXECUTION_RECEIPT" && typeof window.IntimeUI.onReceipt === "function") {
        window.IntimeUI.onReceipt(payload);
      } else if (msg.type === "INTENT_RESULT" && typeof window.IntimeUI.onIntentResult === "function") {
        window.IntimeUI.onIntentResult(payload);
      } else if (msg.type === "INTENT_ERROR" && typeof window.IntimeUI.onIntentError === "function") {
        window.IntimeUI.onIntentError(payload);
      } else if (msg.type === "THEME") {
        var theme = payload.theme === "dark" ? "dark" : "light";
        document.documentElement.setAttribute("data-theme", theme);
        if (typeof window.IntimeUI.onTheme === "function") {
          window.IntimeUI.onTheme(theme);
        }
      }
    }
  };
})();
