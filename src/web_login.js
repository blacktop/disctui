(() => {
  if (window.top !== window || location.origin !== "https://discord.com") return;
  const originalFetch = window.fetch;
  let current;
  let sent = false;
  let timer;
  let checking = false;
  let attempts = 0;
  let authenticatedRequest = false;
  let lastStage;
  const progress = stage => {
    if (stage === lastStage) return;
    lastStage = stage;
    window.ipc.postMessage(JSON.stringify({stage}));
  };
  const apiPath = value => {
    try {
      const target = new URL(value, location.href);
      return target.origin === location.origin && /^\/api\/v\d+\//.test(target.pathname)
        ? target.pathname : "";
    } catch { return ""; }
  };
  progress("waiting");

  let properties;
  const rememberProperties = value => {
    if (typeof value !== "string" || value === properties ||
        !/^[A-Za-z0-9+/=]{2,4096}$/.test(value)) return;
    properties = value;
    window.ipc.postMessage(JSON.stringify({client_properties: value}));
  };

  const remember = value => {
    if (sent || typeof value !== "string") return;
    try { value = JSON.parse(value); } catch {}
    if (typeof value !== "string" || !/^[A-Za-z0-9_.-]{20,2048}$/.test(value)) return;
    if (value !== current) {
      current = value;
      clearTimeout(timer);
      timer = setTimeout(finish, 1000);
    }
  };

  const finish = async () => {
    if (sent || checking || !current || attempts >= 5 ||
        !(authenticatedRequest || location.pathname.startsWith("/channels/"))) return;
    const candidate = current;
    progress("verifying");
    checking = true;
    attempts++;
    try {
      const response = await originalFetch.call(window, "/api/v10/users/@me", {
        headers: {Authorization: candidate},
        cache: "no-store"
      });
      if (response.ok && current === candidate) {
        const account = await response.json();
        if (typeof account.id === "string" && current === candidate) {
          progress("ready");
          sent = true;
          window.ipc.postMessage(JSON.stringify(candidate));
        }
      }
    } catch {}
    finally {
      checking = false;
      if (!sent && attempts >= 5) progress("rejected");
    }
  };

  const originalSetItem = Storage.prototype.setItem;
  Storage.prototype.setItem = function (key, value) {
    const result = originalSetItem.call(this, key, value);
    if (key === "token") remember(value);
    return result;
  };

  const inspect = (url, body) => {
    if (/^\/api\/v\d+\/auth\//.test(apiPath(url))) remember(body?.token);
  };
  const rememberRequest = (url, authorization) => {
    if (!authorization || !apiPath(url) || /^\/api\/v\d+\/auth\//.test(apiPath(url))) return;
    authenticatedRequest = true;
    remember(authorization);
    void finish();
  };

  window.fetch = async function (input, init) {
    let authorization;
    try {
      const sent = new Headers(init?.headers ?? (input instanceof Request ? input.headers : undefined));
      authorization = sent.get("Authorization");
      if (apiPath(input instanceof Request ? input.url : input)) {
        rememberProperties(sent.get("X-Super-Properties"));
      }
    } catch {}
    const response = await originalFetch.call(this, input, init);
    if (response.ok) {
      rememberRequest(response.url, authorization);
      if (/^\/api\/v\d+\/auth\//.test(apiPath(response.url))) {
        response.clone().json().then(body => inspect(response.url, body)).catch(() => {});
      }
    }
    return response;
  };

  const headers = new WeakMap();
  const targets = new WeakMap();
  const originalHeader = XMLHttpRequest.prototype.setRequestHeader;
  XMLHttpRequest.prototype.setRequestHeader = function (name, value) {
    const header = String(name).toLowerCase();
    if (header === "authorization") headers.set(this, value);
    if (header === "x-super-properties" && apiPath(targets.get(this))) rememberProperties(value);
    return originalHeader.call(this, name, value);
  };
  const originalOpen = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function (...args) {
    headers.delete(this);
    targets.set(this, args[1]);
    this.addEventListener("load", () => {
      try {
        if (this.status >= 200 && this.status < 300) {
          rememberRequest(this.responseURL, headers.get(this));
          if (/^\/api\/v\d+\/auth\//.test(apiPath(this.responseURL))) {
            inspect(this.responseURL, this.responseType === "json" ? this.response : JSON.parse(this.responseText));
          }
        }
      } catch {}
    }, {once: true});
    return originalOpen.apply(this, args);
  };

  const poll = setInterval(() => {
    if (sent) { clearInterval(poll); return; }
    if (!location.pathname.startsWith("/channels/") || !document.body) return;
    const frame = document.createElement("iframe");
    frame.hidden = true;
    document.body.appendChild(frame);
    try { remember(frame.contentWindow.localStorage.getItem("token")); } catch {}
    finally { frame.remove(); }
    void finish();
  }, 2000);
  setTimeout(() => clearInterval(poll), 300000);
})();
