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
      selector ===
      "main .surface-panel button, main .surface-panel input, main .surface-panel select"
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
    URLSearchParams,
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

function profileFixture(overrides = {}) {
  const config = {
    engine: "atomic-v1",
    dictionary_version: "test-dictionary",
    dictionary_version_hash: "d".repeat(64),
    blocklist_version: "test-blocklist",
    blocklist_version_hash: "b".repeat(64),
    word1: {
      enabled: true,
      lengths: [5],
      categories: ["common"],
    },
    word2: {
      enabled: true,
      lengths: [5],
      categories: ["common"],
    },
    suffix: {
      enabled: true,
      min_length: 5,
    },
    ...(overrides.config || {}),
  };
  return {
    id: "018ff2de-8cf0-71aa-9e9b-8554cc5f4fd7",
    slug: "team-a",
    access: "private",
    last_atomic_value: 0,
    config_hash: "c".repeat(64),
    ...overrides,
    config,
  };
}

function capacityFixture(overrides = {}) {
  return {
    word1_count: 10,
    word2_count: 10,
    overlapping_words: 0,
    unique_word_combinations: 100,
    suffix_variants: 60_466_176,
    suffix_bits: 25.85,
    total_variants: "6046617600",
    ...overrides,
  };
}

function eventFixture(overrides = {}) {
  return {
    id: "0190f4d2-8f84-7abc-b922-81250d2e20ac",
    created_at_ms: 1_780_800_000_000,
    source: "api",
    action: "generate",
    profile_slug: "team-a",
    profile_id: "018ff2de-8cf0-71aa-9e9b-8554cc5f4fd7",
    token_id: null,
    token_name: null,
    atomic_start: 1,
    atomic_end: 2,
    metadata: { count: 2 },
    ...overrides,
  };
}

function managementFetch({ profiles, tokens = [], history = [], events = [] }) {
  return async (requestPath) => {
    if (requestPath === "/api/profiles") {
      return jsonResponse({ profiles });
    }
    if (requestPath.startsWith("/api/capacity")) {
      return jsonResponse(capacityFixture());
    }
    if (requestPath.endsWith("/history")) {
      return jsonResponse({ profiles: history });
    }
    if (requestPath.endsWith("/tokens")) {
      return jsonResponse({ tokens });
    }
    if (requestPath.startsWith("/api/events")) {
      return jsonResponse({ events });
    }
    return defaultFetch(requestPath);
  };
}

test("profile workspace tabs isolate admin surfaces", async () => {
  const { document } = loadApp();

  assert.equal(document.getElementById("surface-server-panel").hidden, false);
  assert.equal(document.getElementById("surface-profiles-panel").hidden, true);
  assert.match(document.getElementById("surface-server").className, /\bactive\b/);

  await dispatch(document.getElementById("surface-profiles"), "click");

  assert.equal(document.getElementById("surface-server-panel").hidden, true);
  assert.equal(document.getElementById("surface-profiles-panel").hidden, false);
  assert.match(document.getElementById("surface-profiles").className, /\bactive\b/);
  assert.equal(document.getElementById("panel-overview").hidden, false);
  assert.equal(document.getElementById("panel-config").hidden, true);
  assert.match(document.getElementById("tab-overview").className, /\bactive\b/);

  await dispatch(document.getElementById("tab-config"), "click");

  assert.equal(document.getElementById("panel-overview").hidden, true);
  assert.equal(document.getElementById("panel-config").hidden, false);
  assert.match(document.getElementById("tab-config").className, /\bactive\b/);

  await dispatch(document.getElementById("tab-events"), "click");

  assert.equal(document.getElementById("panel-config").hidden, true);
  assert.equal(document.getElementById("panel-events").hidden, false);
  assert.match(document.getElementById("tab-events").className, /\bactive\b/);
});

test("profile import confirms replacement and refreshes management state", async () => {
  const existingProfile = profileFixture();
  const importedProfile = profileFixture({ last_atomic_value: 7 });
  let confirmText = "";
  let importRequest;
  let importFinished = false;
  const { context, document } = loadApp(async (requestPath, options) => {
    if (requestPath === "/api/profiles/import") {
      importRequest = JSON.parse(options.body);
      importFinished = true;
      return jsonResponse({ profile: importedProfile }, 201);
    }
    return managementFetch({
      profiles: importFinished ? [importedProfile] : [existingProfile],
      history: [importedProfile],
    })(requestPath, options);
  });
  context.window.confirm = (message) => {
    confirmText = message;
    return true;
  };

  await context.refreshProfiles();
  const input = document.getElementById("import-profile-file");
  input.files = [
    {
      text: async () =>
        JSON.stringify({
          format: "hoststamp-profile-v1",
          id: importedProfile.id,
          slug: importedProfile.slug,
          access: importedProfile.access,
          last_atomic_value: importedProfile.last_atomic_value,
          config_hash: importedProfile.config_hash,
          config: importedProfile.config,
        }),
    },
  ];

  await dispatch(input, "change");

  assert.match(confirmText, /Import over existing profile team-a/);
  assert.deepEqual(importRequest.confirmation, {
    profile: "team-a",
    action: "replace",
  });
  assert.equal(document.getElementById("profile-title").textContent, "team-a");
  assert.match(
    elementText(document.getElementById("profile-meta")),
    /last_atomic_value = 7/,
  );
  assert.equal(document.getElementById("message").textContent, "imported team-a");
  assert.match(document.getElementById("message").className, /\bok-text\b/);
});

test("profile import cancellation keeps existing profile unchanged", async () => {
  const existingProfile = profileFixture({ last_atomic_value: 3 });
  const importedProfile = profileFixture({ last_atomic_value: 7 });
  let confirmText = "";
  const { calls, context, document } = loadApp(async (requestPath, options) => {
    if (requestPath === "/api/profiles/import") {
      throw new Error("profile import should not run after cancellation");
    }
    return managementFetch({
      profiles: [existingProfile],
      history: [existingProfile],
    })(requestPath, options);
  });
  context.window.confirm = (message) => {
    confirmText = message;
    return false;
  };

  await context.refreshProfiles();
  const input = document.getElementById("import-profile-file");
  input.files = [
    {
      text: async () =>
        JSON.stringify({
          format: "hoststamp-profile-v1",
          id: importedProfile.id,
          slug: importedProfile.slug,
          access: importedProfile.access,
          last_atomic_value: importedProfile.last_atomic_value,
          config_hash: importedProfile.config_hash,
          config: importedProfile.config,
        }),
    },
  ];

  await dispatch(input, "change");

  assert.match(confirmText, /Import over existing profile team-a/);
  assert.equal(
    calls.some((call) => call.path === "/api/profiles/import"),
    false,
  );
  assert.equal(document.getElementById("profile-title").textContent, "team-a");
  assert.match(
    elementText(document.getElementById("profile-meta")),
    /last_atomic_value = 3/,
  );
  assert.notEqual(document.getElementById("message").textContent, "imported team-a");
});

test("profile config replacement posts parsed form values", async () => {
  const profile = profileFixture();
  let configRequest;
  const { context, document } = loadApp(async (requestPath, options) => {
    if (requestPath === "/api/profiles/team-a/config") {
      configRequest = JSON.parse(options.body);
      return jsonResponse({
        profile: profileFixture({
          config: {
            ...profile.config,
            word1: { ...profile.config.word1, lengths: [4, 5] },
            word2: { ...profile.config.word2, enabled: false, lengths: null },
            suffix: { enabled: false, min_length: 6 },
          },
        }),
      });
    }
    return managementFetch({ profiles: [profile] })(requestPath, options);
  });

  await context.refreshProfiles();
  document.getElementById("word1-enabled").value = "true";
  document.getElementById("word1-lengths").value = "4,5";
  document.getElementById("word1-categories").value = "common,diceware";
  document.getElementById("word2-enabled").value = "false";
  document.getElementById("word2-lengths").value = "any";
  document.getElementById("word2-categories").value = "";
  document.getElementById("suffix-enabled").value = "false";
  document.getElementById("suffix-min-length").value = "6";

  await dispatch(document.getElementById("save-config"), "click");

  assert.deepEqual(configRequest, {
    word1_enabled: true,
    word1_lengths: "4,5",
    word1_categories: ["common", "diceware"],
    word2_enabled: false,
    word2_lengths: "any",
    word2_categories: [],
    suffix_enabled: false,
    suffix_min_length: 6,
    confirmation: { profile: "team-a", action: "replace" },
  });
  assert.equal(document.getElementById("message").textContent, "config replaced");
  assert.match(document.getElementById("message").className, /\bok-text\b/);
});

test("atomic reset confirms destructive counter changes", async () => {
  const profile = profileFixture({ last_atomic_value: 12 });
  let confirmText = "";
  let resetRequest;
  const { context, document } = loadApp(async (requestPath, options) => {
    if (requestPath === "/api/profiles/team-a/reset-atomic-value") {
      resetRequest = JSON.parse(options.body);
      return jsonResponse({
        profile: profileFixture({ last_atomic_value: resetRequest.atomic_value }),
      });
    }
    return managementFetch({ profiles: [profile] })(requestPath, options);
  });
  context.window.confirm = (message) => {
    confirmText = message;
    return true;
  };

  await context.refreshProfiles();
  document.getElementById("reset-value").value = "42";
  await dispatch(document.getElementById("reset-atomic"), "click");

  assert.match(confirmText, /Reset atomic counter for team-a/);
  assert.match(confirmText, /atomic value 43/);
  assert.deepEqual(resetRequest, {
    atomic_value: 42,
    confirmation: { profile: "team-a", action: "reset" },
  });
  assert.equal(document.getElementById("message").textContent, "atomic value reset");
});

test("token create and revoke refresh token and event panels", async () => {
  const profile = profileFixture();
  const events = [eventFixture({ action: "profile.token.create", token_name: "deploy" })];
  let tokenCreated = false;
  let tokenRevoked = false;
  let createRequest;
  let revokePath;
  const { context, document } = loadApp(async (requestPath, options) => {
    if (requestPath === "/api/profiles/team-a/tokens" && options.method === "POST") {
      createRequest = JSON.parse(options.body);
      tokenCreated = true;
      return jsonResponse(
        {
          token: {
            name: "deploy",
            token_id: "tok_123",
            expires_at_ms: createRequest.expires_at_ms,
            revoked_at_ms: null,
          },
          profile_token: "hspt_test_secret",
        },
        201,
      );
    }
    if (
      requestPath === "/api/profiles/team-a/tokens/tok_123" &&
      options.method === "DELETE"
    ) {
      revokePath = requestPath;
      tokenRevoked = true;
      return jsonResponse({
        token: {
          name: "deploy",
          token_id: "tok_123",
          expires_at_ms: createRequest.expires_at_ms,
          revoked_at_ms: 1_780_800_000_000,
        },
      });
    }
    return managementFetch({
      profiles: [profile],
      tokens:
        tokenCreated && !tokenRevoked
          ? [
              {
                name: "deploy",
                token_id: "tok_123",
                expires_at_ms: createRequest.expires_at_ms,
                revoked_at_ms: null,
              },
            ]
          : [],
      events,
    })(requestPath, options);
  });

  await context.refreshProfiles();
  document.getElementById("token-name").value = "deploy";
  document.getElementById("token-expires-at-ms").value = "1893456000000";
  await dispatch(document.getElementById("create-token"), "submit");

  assert.deepEqual(createRequest, {
    name: "deploy",
    expires_at_ms: 1_893_456_000_000,
  });
  assert.equal(
    document.getElementById("created-token").textContent,
    "hspt_test_secret",
  );
  assert.match(elementText(document.getElementById("tokens-body")), /deploy/);
  assert.match(
    elementText(document.getElementById("event-detail")),
    /profile.token.create/,
  );

  const revokeButton =
    document.getElementById("tokens-body").children[0].children[3].children[0];
  await dispatch(revokeButton, "click");

  assert.equal(revokePath, "/api/profiles/team-a/tokens/tok_123");
  assert.match(elementText(document.getElementById("tokens-body")), /No tokens/);
});

test("event filters include selected profile and reset to defaults", async () => {
  const profile = profileFixture();
  const eventPaths = [];
  const { context, document } = loadApp(async (requestPath, options) => {
    if (requestPath.startsWith("/api/events")) eventPaths.push(requestPath);
    return managementFetch({
      profiles: [profile],
      events: [eventFixture()],
    })(requestPath, options);
  });

  await context.refreshProfiles();
  document.getElementById("event-action").value = "generate";
  document.getElementById("event-source").value = "api";
  document.getElementById("event-token-name").value = "deploy";
  document.getElementById("event-since-ms").value = "100";
  document.getElementById("event-until-ms").value = "200";
  document.getElementById("event-limit").value = "10";
  await dispatch(document.getElementById("apply-events"), "click");

  assert.equal(
    eventPaths.at(-1),
    "/api/events?profile=team-a&action=generate&source=api&token_name=deploy&since_ms=100&until_ms=200&limit=10",
  );
  assert.match(elementText(document.getElementById("event-detail")), /generate/);

  await dispatch(document.getElementById("reset-events"), "click");

  assert.equal(eventPaths.at(-1), "/api/events?profile=team-a&limit=25");
  assert.equal(document.getElementById("event-action").value, "");
  assert.equal(document.getElementById("event-limit").value, "25");
});

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
