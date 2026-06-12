// SPDX-License-Identifier: FSL-1.1-ALv2

const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");

const profileHealth = require("../static/profile-health.js");

const staticDir = path.join(__dirname, "..", "static");
const appSource = fs.readFileSync(path.join(staticDir, "app.js"), "utf8");
const htmlSource = fs.readFileSync(path.join(staticDir, "index.html"), "utf8");

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function jsonResponse(body, status = 200) {
  return {
    ok: status >= 200 && status < 300,
    status,
    headers: {
      get(name) {
        return name.toLowerCase() === "content-type"
          ? "application/json"
          : null;
      },
    },
    json: async () => body,
    text: async () => JSON.stringify(body),
  };
}

function createClassList(element) {
  function names() {
    return new Set(element.className.split(/\s+/).filter(Boolean));
  }
  function write(values) {
    element.className = [...values].join(" ");
  }
  return {
    add(...values) {
      const current = names();
      for (const value of values) current.add(value);
      write(current);
    },
    contains(value) {
      return names().has(value);
    },
    remove(...values) {
      const current = names();
      for (const value of values) current.delete(value);
      write(current);
    },
    toggle(value, force) {
      const current = names();
      const enabled = force ?? !current.has(value);
      if (enabled) {
        current.add(value);
      } else {
        current.delete(value);
      }
      write(current);
      return enabled;
    },
  };
}

class TestElement {
  constructor(tagName, id = "") {
    this.tagName = tagName.toUpperCase();
    this.id = id;
    this.children = [];
    this.listeners = new Map();
    this.parentNode = null;
    this.className = "";
    this.textContent = "";
    this.value = "";
    this.hidden = false;
    this.disabled = false;
    this.files = [];
    this.href = "";
    this.download = "";
    this.colSpan = 1;
    this.classList = createClassList(this);
  }

  addEventListener(type, handler) {
    const handlers = this.listeners.get(type) || [];
    handlers.push(handler);
    this.listeners.set(type, handlers);
  }

  append(...nodes) {
    for (const node of nodes) this.appendChild(node);
  }

  appendChild(node) {
    if (typeof node === "string") {
      node = new TestText(node);
    }
    node.parentNode = this;
    this.children.push(node);
    return node;
  }

  click() {
    return dispatch(this, "click");
  }

  remove() {
    if (!this.parentNode) return;
    this.parentNode.children = this.parentNode.children.filter(
      (child) => child !== this,
    );
    this.parentNode = null;
  }

  replaceChildren(...nodes) {
    for (const child of this.children) child.parentNode = null;
    this.children = [];
    this.textContent = "";
    this.append(...nodes);
  }
}

class TestText {
  constructor(text) {
    this.textContent = text;
    this.children = [];
    this.parentNode = null;
  }
}

class TestDocument {
  constructor(html) {
    this.body = new TestElement("body", "body");
    this.elements = new Map([["body", this.body]]);

    const idPattern = /<([a-z0-9-]+)\b[^>]*\bid="([^"]+)"/gi;
    for (const match of html.matchAll(idPattern)) {
      const element = new TestElement(match[1], match[2]);
      const classMatch = match[0].match(/\bclass="([^"]+)"/);
      const valueMatch = match[0].match(/\bvalue="([^"]*)"/);
      if (classMatch) element.className = classMatch[1];
      if (valueMatch) element.value = valueMatch[1];
      this.elements.set(element.id, element);
      this.body.appendChild(element);
    }

    this.getElementById("profile-access").value = "private";
    this.getElementById("event-profile-scope").value = "selected";
    this.getElementById("event-limit").value = "25";
  }

  createElement(tagName) {
    return new TestElement(tagName);
  }

  getElementById(id) {
    const element = this.elements.get(id);
    if (!element) throw new Error(`missing test element ${id}`);
    return element;
  }

  querySelectorAll(selector) {
    if (
      selector === "main .layout button, main .layout input, main .layout select"
    ) {
      return [...this.elements.values()].filter((element) =>
        ["BUTTON", "INPUT", "SELECT"].includes(element.tagName),
      );
    }
    return [];
  }
}

async function defaultFetch(path) {
  if (path === "/api/health") {
    return jsonResponse({ status: "ok", service: "hoststamp" });
  }
  if (path === "/api/profiles") {
    return jsonResponse({ profiles: [] });
  }
  if (path.startsWith("/api/events")) {
    return jsonResponse({ events: [] });
  }
  return jsonResponse({});
}

function loadApp(fetchHandler = defaultFetch) {
  const document = new TestDocument(htmlSource);
  const localStorageValues = new Map();
  const calls = [];
  const context = {
    Blob: class Blob {
      constructor(parts, options = {}) {
        this.parts = parts;
        this.type = options.type || "";
      }
    },
    HoststampProfileHealth: profileHealth,
    URL: {
      createObjectURL: () => "blob:test",
      revokeObjectURL: () => {},
    },
    clearInterval: () => {},
    console,
    document,
    fetch: async (requestPath, options = {}) => {
      calls.push({ path: requestPath, options });
      return fetchHandler(requestPath, options, calls);
    },
    localStorage: {
      getItem: (key) => localStorageValues.get(key) || null,
      removeItem: (key) => localStorageValues.delete(key),
      setItem: (key, value) => localStorageValues.set(key, value),
    },
    navigator: {},
    setInterval: () => 0,
    window: {
      confirm: () => true,
    },
  };
  context.globalThis = context;
  vm.createContext(context);
  vm.runInContext(appSource, context, { filename: "app.js" });
  return { calls, context, document };
}

async function dispatch(element, type, event = {}) {
  const handlers = element.listeners.get(type) || [];
  const payload = {
    preventDefault: () => {},
    target: element,
    ...event,
  };
  for (const handler of handlers) {
    await handler(payload);
  }
}

function elementText(element) {
  return [element.textContent, ...element.children.map(elementText)]
    .filter(Boolean)
    .join(" ");
}

async function waitFor(condition) {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    if (condition()) return;
    await Promise.resolve();
  }
  assert.equal(condition(), true);
}

test("backup import renders preview blockers without importing", async () => {
  const preview = deferred();
  const { calls, context, document } = loadApp((requestPath, options) => {
    if (requestPath === "/api/backup/import/preview") {
      return preview.promise;
    }
    if (requestPath === "/api/backup/import") {
      throw new Error("import should not run for blocked previews");
    }
    return defaultFetch(requestPath, options);
  });
  context.setManagementEnabled(true);

  const input = document.getElementById("import-backup-file");
  input.value = "hoststamp-backup.json";
  input.files = [
    {
      text: async () =>
        JSON.stringify({
          events: [],
          exported_at_ms: 1,
          format: "hoststamp-backup-v1",
          profile_tokens: [],
          profiles: [],
        }),
    },
  ];

  const running = dispatch(input, "change");
  assert.equal(document.getElementById("export-backup").disabled, true);
  assert.equal(document.getElementById("import-backup").disabled, true);
  assert.equal(document.getElementById("backup-status").textContent, "Reading backup");
  await waitFor(
    () => document.getElementById("backup-status").textContent === "Previewing backup",
  );

  preview.resolve(
    jsonResponse({
      valid: false,
      profile_count: 2,
      event_count: 3,
      skipped_profile_token_count: 1,
      blockers: ["backup import requires an empty database"],
    }),
  );
  await running;

  const previewElement = document.getElementById("backup-preview");
  assert.equal(previewElement.hidden, false);
  assert.match(elementText(previewElement), /profile_rows 2/);
  assert.match(elementText(previewElement), /retained_events 3/);
  assert.match(elementText(previewElement), /profile_tokens_skipped 1/);
  assert.match(elementText(previewElement), /requires an empty database/);
  assert.equal(document.getElementById("backup-status").textContent, "Backup import blocked");
  assert.match(document.getElementById("backup-status").className, /\berror\b/);
  assert.equal(document.getElementById("message").textContent, "backup import blocked");
  assert.equal(input.value, "");
  assert.equal(document.getElementById("export-backup").disabled, false);
  assert.equal(document.getElementById("import-backup").disabled, false);
  assert.equal(
    calls.some((call) => call.path === "/api/backup/import"),
    false,
  );
});

test("backup import reports invalid JSON and resets the file input", async () => {
  const { calls, context, document } = loadApp();
  context.setManagementEnabled(true);

  const input = document.getElementById("import-backup-file");
  input.value = "bad.json";
  input.files = [{ text: async () => "{" }];

  await dispatch(input, "change");

  assert.equal(
    document.getElementById("backup-status").textContent,
    "backup import file is not valid JSON",
  );
  assert.match(document.getElementById("backup-status").className, /\berror\b/);
  assert.equal(
    document.getElementById("message").textContent,
    "backup import file is not valid JSON",
  );
  assert.equal(document.getElementById("backup-preview").hidden, true);
  assert.equal(input.value, "");
  assert.equal(document.getElementById("export-backup").disabled, false);
  assert.equal(document.getElementById("import-backup").disabled, false);
  assert.equal(
    calls.some((call) => call.path === "/api/backup/import/preview"),
    false,
  );
});

test("backup import uses preview counts and cancels before import", async () => {
  let confirmText = "";
  const { calls, context, document } = loadApp((requestPath, options) => {
    if (requestPath === "/api/backup/import/preview") {
      return jsonResponse({
        valid: true,
        profile_count: 1,
        event_count: 4,
        skipped_profile_token_count: 2,
        blockers: [],
      });
    }
    if (requestPath === "/api/backup/import") {
      throw new Error("import should not run after canceling confirmation");
    }
    return defaultFetch(requestPath, options);
  });
  context.window.confirm = (message) => {
    confirmText = message;
    return false;
  };
  context.setManagementEnabled(true);

  const input = document.getElementById("import-backup-file");
  input.value = "hoststamp-backup.json";
  input.files = [
    {
      text: async () =>
        JSON.stringify({
          events: [],
          exported_at_ms: 1,
          format: "hoststamp-backup-v1",
          profile_tokens: [],
          profiles: [],
        }),
    },
  ];

  await dispatch(input, "change");

  assert.match(confirmText, /Import backup bundle/);
  assert.match(confirmText, /1 profile row/);
  assert.match(confirmText, /4 retained events/);
  assert.match(confirmText, /2 profile tokens skipped/);
  assert.equal(document.getElementById("backup-status").textContent, "Backup import canceled");
  assert.equal(document.getElementById("message").textContent, "backup import canceled");
  assert.equal(document.getElementById("backup-preview").hidden, false);
  assert.equal(input.value, "");
  assert.equal(
    calls.some((call) => call.path === "/api/backup/import"),
    false,
  );
});

test("backup import posts valid bundles and clears the preview", async () => {
  let confirmText = "";
  const { calls, context, document } = loadApp((requestPath, options) => {
    if (requestPath === "/api/backup/import/preview") {
      return jsonResponse({
        valid: true,
        profile_count: 2,
        event_count: 5,
        skipped_profile_token_count: 3,
        blockers: [],
      });
    }
    if (requestPath === "/api/backup/import") {
      return jsonResponse({
        profile_count: 2,
        event_count: 5,
        skipped_profile_token_count: 3,
      });
    }
    return defaultFetch(requestPath, options);
  });
  context.window.confirm = (message) => {
    confirmText = message;
    return true;
  };
  context.setManagementEnabled(true);

  const input = document.getElementById("import-backup-file");
  input.value = "hoststamp-backup.json";
  input.files = [
    {
      text: async () =>
        JSON.stringify({
          events: [],
          exported_at_ms: 1,
          format: "hoststamp-backup-v1",
          profile_tokens: [],
          profiles: [],
        }),
    },
  ];

  await dispatch(input, "change");

  const importCall = calls.find((call) => call.path === "/api/backup/import");
  assert.ok(importCall);
  assert.equal(importCall.options.method, "POST");
  assert.match(confirmText, /2 profile rows/);
  assert.match(confirmText, /5 retained events/);
  assert.match(confirmText, /3 profile tokens skipped/);
  assert.equal(document.getElementById("backup-status").textContent, "Backup imported");
  assert.match(document.getElementById("backup-status").className, /\bok-text\b/);
  assert.equal(
    document.getElementById("message").textContent,
    "imported backup; skipped 3 profile tokens",
  );
  assert.equal(document.getElementById("backup-preview").hidden, true);
  assert.equal(input.value, "");
  assert.equal(document.getElementById("export-backup").disabled, false);
  assert.equal(document.getElementById("import-backup").disabled, false);
});
