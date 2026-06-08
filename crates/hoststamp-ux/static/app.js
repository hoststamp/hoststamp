// SPDX-License-Identifier: FSL-1.1-ALv2

const state = {
  profiles: [],
  profileHistory: null,
  events: null,
  selectedEventId: null,
  selected: null,
  adminToken: localStorage.getItem("hoststamp.adminToken") || "",
  unlocked: false,
};

const el = (id) => document.getElementById(id);

function headers(json = false) {
  const value = {};
  if (json) value["content-type"] = "application/json";
  if (state.adminToken) value.authorization = `Bearer ${state.adminToken}`;
  return value;
}

function setMessage(text, kind = "muted") {
  const message = el("message");
  message.className = kind;
  message.textContent = text;
}

async function api(path, options = {}) {
  const response = await fetch(path, {
    cache: "no-store",
    ...options,
    headers: {
      ...headers(options.json),
      ...(options.headers || {}),
    },
  });
  if (!response.ok) {
    const text = await response.text();
    const error = new Error(text || `HTTP ${response.status}`);
    error.status = response.status;
    throw error;
  }
  if (response.status === 204) return null;
  const type = response.headers.get("content-type") || "";
  return type.includes("application/json")
    ? response.json()
    : response.text();
}

function setManagementEnabled(enabled) {
  state.unlocked = enabled;
  document.body.classList.toggle("locked", !enabled);
  el("auth-gate").hidden = enabled;
  for (const control of document.querySelectorAll(
    "main .layout button, main .layout input, main .layout select",
  )) {
    control.disabled = !enabled;
  }
}

function clearProfileState() {
  state.profiles = [];
  state.profileHistory = null;
  state.events = null;
  state.selectedEventId = null;
  state.selected = null;
  renderProfiles();
  renderProfile();
  renderProfileHistory(null);
  renderEvents(null);
  resetProfileForms();
  renderCapacity(null);
  renderTokens(null);
  el("created-token").textContent = "";
}

function resetProfileForms() {
  el("generate-count").value = "1";
  el("regenerate-value").value = "1";
  el("regenerate-count").value = "1";
  el("reset-value").value = "0";
  el("clone-profile-slug").value = "";
  el("results").replaceChildren();
  el("created-token").textContent = "";
}

function showAuthGate(title, detail) {
  el("auth-gate-title").textContent = title;
  el("auth-gate-detail").textContent = detail;
}

function lockManagement(title, detail) {
  setManagementEnabled(false);
  showAuthGate(title, detail);
  clearProfileState();
}

function unlockManagement() {
  setManagementEnabled(true);
  showAuthGate("Unlocked", "Profile management is available.");
}

function authErrorMessage(error) {
  if (error.status === 503) {
    return [
      "Server admin token is not configured.",
      "Restart Hoststamp with HOSTSTAMP_ADMIN_TOKEN set, then enter it here.",
    ];
  }
  if (error.status === 401 || error.status === 403) {
    return [
      "Auth required",
      "Enter a valid admin bearer token to unlock profile management.",
    ];
  }
  return ["Admin API unavailable", error.message];
}

function slugPath(slug) {
  return encodeURIComponent(slug);
}

function cell(text, className) {
  const td = document.createElement("td");
  if (className) td.className = className;
  td.textContent = String(text);
  return td;
}

function emptyRow(text, colspan) {
  const row = document.createElement("tr");
  const td = cell(text, "empty");
  td.colSpan = colspan;
  row.appendChild(td);
  return row;
}

function actionCell(event) {
  const td = document.createElement("td");
  const button = document.createElement("button");
  button.type = "button";
  button.className = "compact";
  button.textContent = "View";
  button.addEventListener("click", () => selectEvent(event.id));
  td.appendChild(button);
  return td;
}

function selectedProfile() {
  return state.profiles.find((profile) => profile.slug === state.selected);
}

function formatLengths(lengths) {
  return lengths === null ? "any" : lengths.join(",");
}

function parseLengthsInput(value) {
  const trimmed = value.trim();
  return trimmed.toLowerCase() === "any" ? "any" : trimmed;
}

function renderProfiles() {
  const root = el("profiles");
  root.replaceChildren();
  if (!state.profiles.length) {
    const empty = document.createElement("div");
    empty.className = "empty";
    empty.textContent = "No profiles";
    root.appendChild(empty);
    return;
  }
  for (const profile of state.profiles) {
    const button = document.createElement("button");
    button.type = "button";
    button.className =
      "profile-row" + (profile.slug === state.selected ? " active" : "");
    const slug = document.createElement("span");
    slug.className = "mono";
    slug.textContent = profile.slug;
    const access = document.createElement("span");
    access.className = "badge";
    access.textContent = profile.access;
    button.append(slug, access);
    button.addEventListener("click", () => selectProfile(profile.slug));
    root.appendChild(button);
  }
}

function renderProfile() {
  const profile = selectedProfile();
  el("profile-title").textContent = profile ? profile.slug : "Profile";
  el("profile-access").value = profile?.access || "private";
  if (!profile) {
    el("profile-meta").textContent = "No profile selected";
    return;
  }
  const meta = el("profile-meta");
  const id = document.createElement("div");
  id.className = "mono";
  id.textContent = `id = ${profile.id}`;
  const atomic = document.createElement("div");
  atomic.textContent = `last_atomic_value = ${profile.last_atomic_value}`;
  const hash = document.createElement("div");
  hash.className = "mono";
  hash.textContent = `config_hash = ${profile.config_hash}`;
  meta.replaceChildren(id, atomic, hash);
  const config = profile.config;
  el("word1-enabled").value = String(config.word1.enabled);
  el("word1-lengths").value = formatLengths(config.word1.lengths);
  el("word1-categories").value = config.word1.categories.join(",");
  el("word2-enabled").value = String(config.word2.enabled);
  el("word2-lengths").value = formatLengths(config.word2.lengths);
  el("word2-categories").value = config.word2.categories.join(",");
  el("suffix-enabled").value = String(config.suffix.enabled);
  el("suffix-min-length").value = config.suffix.min_length;
}

async function copyResult(value) {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(value);
  } else {
    const input = document.createElement("textarea");
    input.value = value;
    input.setAttribute("readonly", "");
    input.style.position = "fixed";
    input.style.opacity = "0";
    document.body.appendChild(input);
    input.select();
    document.execCommand("copy");
    input.remove();
  }
  setMessage("copied", "ok-text");
}

function renderResults(hostnames) {
  const root = el("results");
  root.replaceChildren();
  for (const generated of hostnames) {
    const item = document.createElement("li");
    const hostname = document.createElement("span");
    hostname.className = "mono";
    hostname.textContent = generated.hostname;
    item.appendChild(hostname);
    if (generated.atomic_value !== undefined) {
      const meta = document.createElement("span");
      meta.className = "badge";
      meta.textContent = `#${generated.atomic_value}`;
      item.appendChild(meta);
    }
    const copy = document.createElement("button");
    copy.type = "button";
    copy.className = "compact";
    copy.textContent = "Copy";
    copy.addEventListener("click", async () => {
      try {
        await copyResult(generated.hostname);
      } catch (error) {
        setMessage(error.message, "error");
      }
    });
    item.appendChild(copy);
    root.appendChild(item);
  }
}

function renderCapacity(report) {
  if (!report) {
    el("capacity-body").replaceChildren(emptyRow("No profile selected", 2));
    return;
  }
  const rows = [
    ["word1_words", report.word1_count ?? "disabled"],
    ["word2_words", report.word2_count ?? "disabled"],
    ["overlapping_words", report.overlapping_words],
    ["unique_word_combinations", report.unique_word_combinations],
    ["fixed_suffix_variants", report.suffix_variants ?? "disabled"],
    ["suffix_bits", report.suffix_bits ?? 0],
    ["total_variants", report.total_variants],
  ];
  const body = el("capacity-body");
  body.replaceChildren(
    ...rows.map(([key, value]) => {
      const row = document.createElement("tr");
      row.append(cell(key), cell(value, "mono"));
      return row;
    }),
  );
}

function renderProfileHistory(profiles) {
  const body = el("history-body");
  body.replaceChildren();
  if (!profiles) {
    body.appendChild(emptyRow("No profile selected", 5));
    return;
  }
  if (!profiles.length) {
    body.appendChild(emptyRow("No history", 5));
    return;
  }
  for (const profile of profiles) {
    const row = document.createElement("tr");
    const lifecycle = profile.replaced_at_ms === null ? "active" : "replaced";
    row.append(
      cell(lifecycle),
      cell(profile.id, "mono"),
      cell(profile.last_atomic_value, "mono"),
      cell(profile.replaced_at_ms ?? "n/a", "mono"),
      cell(profile.replaced_by_id ?? "n/a", "mono"),
    );
    body.appendChild(row);
  }
}

function renderTokens(tokens) {
  const body = el("tokens-body");
  body.replaceChildren();
  if (!tokens) {
    body.appendChild(emptyRow("No profile selected", 4));
    return;
  }
  if (!tokens.length) {
    body.appendChild(emptyRow("No tokens", 4));
    return;
  }
  for (const token of tokens) {
    const row = document.createElement("tr");
    const action = document.createElement("td");
    const button = document.createElement("button");
    button.className = "danger";
    button.type = "button";
    button.textContent = "Revoke";
    button.addEventListener("click", () => revokeToken(token.token_id));
    action.appendChild(button);
    row.append(
      cell(token.name),
      cell(token.token_id, "mono"),
      cell(token.expires_at_ms ?? "n/a", "mono"),
      action,
    );
    body.appendChild(row);
  }
}

function renderEvents(events) {
  const body = el("events-body");
  body.replaceChildren();
  if (!events) {
    state.selectedEventId = null;
    body.appendChild(emptyRow("No events loaded", 7));
    renderEventDetail(null);
    return;
  }
  if (!events.length) {
    state.selectedEventId = null;
    body.appendChild(emptyRow("No events", 7));
    renderEventDetail(null);
    return;
  }
  if (!events.some((event) => event.id === state.selectedEventId)) {
    state.selectedEventId = events[0].id;
  }
  for (const event of events) {
    const row = document.createElement("tr");
    row.className =
      "event-row" + (event.id === state.selectedEventId ? " selected" : "");
    row.append(
      cell(formatTimestamp(event.created_at_ms), "mono"),
      cell(event.action),
      cell(event.profile_slug ?? "n/a", "mono"),
      cell(event.source),
      cell(event.token_name ?? "n/a"),
      cell(formatAtomicRange(event.atomic_start, event.atomic_end), "mono"),
      actionCell(event),
    );
    body.appendChild(row);
  }
  renderEventDetail(events.find((event) => event.id === state.selectedEventId));
}

function selectEvent(id) {
  state.selectedEventId = id;
  renderEvents(state.events);
}

function renderEventDetail(event) {
  const root = el("event-detail");
  root.replaceChildren();
  if (!event) {
    const empty = document.createElement("div");
    empty.className = "empty";
    empty.textContent = "No event selected";
    root.appendChild(empty);
    return;
  }

  const fields = [
    ["id", event.id],
    ["created_at_ms", event.created_at_ms],
    ["time", formatTimestamp(event.created_at_ms)],
    ["action", event.action],
    ["source", event.source],
    ["profile_slug", event.profile_slug ?? "n/a"],
    ["profile_id", event.profile_id ?? "n/a"],
    ["token_name", event.token_name ?? "n/a"],
    ["token_id", event.token_id ?? "n/a"],
    ["atomic_range", formatAtomicRange(event.atomic_start, event.atomic_end)],
  ];
  const list = document.createElement("dl");
  for (const [label, value] of fields) {
    const term = document.createElement("dt");
    term.textContent = label;
    const detail = document.createElement("dd");
    detail.className = "mono";
    detail.textContent = String(value);
    list.append(term, detail);
  }

  const metadata = document.createElement("pre");
  metadata.textContent = JSON.stringify(event.metadata ?? {}, null, 2);
  root.append(list, metadata);
}

function formatAtomicRange(start, end) {
  if (start === null || start === undefined || end === null || end === undefined) {
    return "n/a";
  }
  return start === end ? String(start) : `${start}-${end}`;
}

function formatTimestamp(value) {
  const timestamp = Number(value);
  if (!Number.isFinite(timestamp)) return "n/a";
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) return String(value);
  return date.toISOString().replace("T", " ").replace(/\.\d{3}Z$/, "Z");
}

function resetEventFilters() {
  el("event-profile-scope").value = "selected";
  for (const id of [
    "event-action",
    "event-source",
    "event-token-name",
    "event-since-ms",
    "event-until-ms",
  ]) {
    el(id).value = "";
  }
  el("event-limit").value = "25";
}

async function refreshHealth() {
  const dot = el("status-dot");
  const text = el("status-text");
  try {
    const payload = await api("/api/health");
    dot.classList.add("ok");
    text.textContent = `${payload.service} online`;
  } catch (error) {
    dot.classList.remove("ok");
    text.textContent = "API offline";
  }
}

async function refreshProfiles() {
  const payload = await api("/api/profiles");
  state.profiles = payload.profiles;
  unlockManagement();
  if (!state.selected && state.profiles.length) {
    state.selected = state.profiles[0].slug;
  }
  if (
    state.selected &&
    !state.profiles.some((profile) => profile.slug === state.selected)
  ) {
    state.selected = state.profiles[0]?.slug || null;
  }
  renderProfiles();
  renderProfile();
  if (state.selected) {
    await Promise.allSettled([
      refreshCapacity(),
      refreshProfileHistory(),
      refreshTokens(),
      refreshEvents(),
    ]);
  } else {
    renderCapacity(null);
    renderProfileHistory(null);
    renderTokens(null);
    renderEvents(null);
  }
}

async function validateAdminToken() {
  try {
    await refreshProfiles();
    setMessage("admin token accepted", "ok-text");
  } catch (error) {
    const [title, detail] = authErrorMessage(error);
    lockManagement(title, detail);
    setMessage(detail, "error");
  }
}

async function selectProfile(slug) {
  if (!state.unlocked) return;
  state.selected = slug;
  resetProfileForms();
  renderProfiles();
  renderProfile();
  await Promise.allSettled([
    refreshCapacity(),
    refreshProfileHistory(),
    refreshTokens(),
    refreshEvents(),
  ]);
}

async function createProfile(event) {
  event.preventDefault();
  if (!state.unlocked) return;
  const slug = el("new-profile-slug").value.trim();
  if (!slug) return;
  await api("/api/profiles", {
    method: "POST",
    json: true,
    body: JSON.stringify({ slug }),
  });
  el("new-profile-slug").value = "";
  state.selected = slug;
  await refreshProfiles();
  setMessage(`created ${slug}`, "ok-text");
}

async function cloneProfile(event) {
  event.preventDefault();
  if (!state.unlocked || !state.selected) return;
  const targetSlug = el("clone-profile-slug").value.trim();
  if (!targetSlug) return;
  const cloned = await api(`/api/profiles/${slugPath(state.selected)}/clone`, {
    method: "POST",
    json: true,
    body: JSON.stringify({ target_slug: targetSlug }),
  });
  el("clone-profile-slug").value = "";
  state.selected = cloned.profile.slug;
  await refreshProfiles();
  setMessage(`cloned ${cloned.profile.slug}`, "ok-text");
}

async function refreshCapacity() {
  if (!state.unlocked || !state.selected) return;
  const payload = await api(
    `/api/capacity?profile=${slugPath(state.selected)}`,
  );
  renderCapacity(payload);
}

async function refreshProfileHistory() {
  if (!state.unlocked || !state.selected) return;
  const payload = await api(`/api/profiles/${slugPath(state.selected)}/history`);
  state.profileHistory = payload.profiles;
  renderProfileHistory(state.profileHistory);
}

async function refreshEvents() {
  if (!state.unlocked) return;
  const params = new URLSearchParams();
  if (el("event-profile-scope").value === "selected" && state.selected) {
    params.set("profile", state.selected);
  }
  const fields = [
    ["action", "event-action"],
    ["source", "event-source"],
    ["token_name", "event-token-name"],
    ["since_ms", "event-since-ms"],
    ["until_ms", "event-until-ms"],
    ["limit", "event-limit"],
  ];
  for (const [key, id] of fields) {
    const value = el(id).value.trim();
    if (value) params.set(key, value);
  }
  const suffix = params.toString();
  const payload = await api(`/api/events${suffix ? `?${suffix}` : ""}`);
  state.events = payload.events;
  renderEvents(state.events);
}

async function resetEvents() {
  resetEventFilters();
  await refreshEvents();
}

async function generate() {
  if (!state.unlocked || !state.selected) return;
  const count = el("generate-count").value || "1";
  const payload = await api(
    `/api/generate?format=json&profile=${slugPath(state.selected)}&count=${encodeURIComponent(count)}`,
    { method: "POST" },
  );
  renderResults(payload.hostnames);
  await refreshProfiles();
  setMessage("generated", "ok-text");
}

async function regenerate() {
  if (!state.unlocked || !state.selected) return;
  const value = el("regenerate-value").value || "1";
  const count = el("regenerate-count").value || "1";
  const payload = await api(
    `/api/regenerate?format=json&profile=${slugPath(state.selected)}&atomic_value=${encodeURIComponent(value)}&count=${encodeURIComponent(count)}`,
  );
  renderResults(payload.hostnames);
  await refreshEvents();
  setMessage("regenerated", "ok-text");
}

async function saveAccess() {
  if (!state.unlocked || !state.selected) return;
  const access = el("profile-access").value;
  await api(`/api/profiles/${slugPath(state.selected)}/access`, {
    method: "PATCH",
    json: true,
    body: JSON.stringify({ access }),
  });
  await refreshProfiles();
  setMessage("access updated", "ok-text");
}

async function exportProfile() {
  if (!state.unlocked || !state.selected) return;
  const payload = await api(`/api/profiles/${slugPath(state.selected)}/export`);
  const data = JSON.stringify(payload, null, 2);
  const blob = new Blob([data], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = `hoststamp-${payload.slug}.profile.json`;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
  await refreshEvents();
  setMessage(`exported ${payload.slug}`, "ok-text");
}

function startImportProfile() {
  if (!state.unlocked) return;
  const input = el("import-profile-file");
  input.value = "";
  input.click();
}

async function importProfile(event) {
  if (!state.unlocked) return;
  const file = event.target.files[0];
  if (!file) return;
  const payload = JSON.parse(await file.text());
  if (!payload.slug) {
    throw new Error("profile import file is missing a slug");
  }
  const exists = state.profiles.some((profile) => profile.slug === payload.slug);
  if (exists) {
    const accepted = window.confirm(
      [
        `Import over existing profile ${payload.slug}?`,
        "",
        "This preserves the imported profile ID and atomic counter.",
        "If the config differs, existing profile tokens and deterministic history may no longer match the active profile.",
      ].join("\n"),
    );
    if (!accepted) return;
    payload.confirmation = { profile: payload.slug, action: "replace" };
  }
  const imported = await api("/api/profiles/import", {
    method: "POST",
    json: true,
    body: JSON.stringify(payload),
  });
  state.selected = imported.profile.slug;
  await refreshProfiles();
  setMessage(`imported ${imported.profile.slug}`, "ok-text");
}

async function saveConfig() {
  if (!state.unlocked || !state.selected) return;
  const slug = state.selected;
  const body = {
    word1_enabled: el("word1-enabled").value === "true",
    word1_lengths: parseLengthsInput(el("word1-lengths").value),
    word1_categories: el("word1-categories")
      .value.split(",")
      .map((value) => value.trim())
      .filter(Boolean),
    word2_enabled: el("word2-enabled").value === "true",
    word2_lengths: parseLengthsInput(el("word2-lengths").value),
    word2_categories: el("word2-categories")
      .value.split(",")
      .map((value) => value.trim())
      .filter(Boolean),
    suffix_enabled: el("suffix-enabled").value === "true",
    suffix_min_length: Number(el("suffix-min-length").value),
    confirmation: { profile: slug, action: "replace" },
  };
  await api(`/api/profiles/${slugPath(slug)}/config`, {
    method: "PATCH",
    json: true,
    body: JSON.stringify(body),
  });
  await refreshProfiles();
  setMessage("config replaced", "ok-text");
}

async function resetAtomic() {
  if (!state.unlocked || !state.selected) return;
  const slug = state.selected;
  const atomicValue = Number(el("reset-value").value || "0");
  const accepted = window.confirm(
    [
      `Reset atomic counter for ${slug}?`,
      "",
      `The next generated hostname will use atomic value ${atomicValue + 1}.`,
      "Lowering this value can duplicate names that were already issued.",
      "Raising it skips part of the deterministic sequence.",
    ].join("\n"),
  );
  if (!accepted) return;
  await api(`/api/profiles/${slugPath(slug)}/reset-atomic-value`, {
    method: "POST",
    json: true,
    body: JSON.stringify({
      atomic_value: atomicValue,
      confirmation: { profile: slug, action: "reset" },
    }),
  });
  await refreshProfiles();
  setMessage("atomic value reset", "ok-text");
}

async function refreshTokens() {
  if (!state.unlocked || !state.selected) return;
  const payload = await api(`/api/profiles/${slugPath(state.selected)}/tokens`);
  renderTokens(payload.tokens);
}

async function createToken(event) {
  event.preventDefault();
  if (!state.unlocked || !state.selected) return;
  const name = el("token-name").value.trim();
  if (!name) return;
  if (!/^[a-z0-9](?:[a-z0-9_.-]{0,62}[a-z0-9])?$/.test(name)) {
    throw new Error(
      "token names must be lowercase letters, digits, hyphen, underscore, or dot",
    );
  }
  const expiresAt = el("token-expires-at-ms").value.trim();
  const body = { name };
  if (expiresAt) {
    const expiresAtMs = Number(expiresAt);
    if (!Number.isSafeInteger(expiresAtMs) || expiresAtMs <= 0) {
      throw new Error("expires_at_ms must be a positive integer");
    }
    body.expires_at_ms = expiresAtMs;
  }
  const payload = await api(`/api/profiles/${slugPath(state.selected)}/tokens`, {
    method: "POST",
    json: true,
    body: JSON.stringify(body),
  });
  el("token-name").value = "";
  el("token-expires-at-ms").value = "";
  el("created-token").textContent = payload.profile_token;
  await refreshTokens();
  await refreshEvents();
}

async function revokeToken(tokenId) {
  if (!state.unlocked || !state.selected) return;
  await api(
    `/api/profiles/${slugPath(state.selected)}/tokens/${encodeURIComponent(tokenId)}`,
    { method: "DELETE" },
  );
  await refreshTokens();
  await refreshEvents();
}

function wire(id, event, handler) {
  el(id).addEventListener(event, async (ev) => {
    try {
      await handler(ev);
    } catch (error) {
      setMessage(error.message, "error");
    }
  });
}

el("admin-token").value = state.adminToken;
wire("save-token", "click", async () => {
  state.adminToken = el("admin-token").value.trim();
  if (!state.adminToken) {
    localStorage.removeItem("hoststamp.adminToken");
    lockManagement(
      "Auth required",
      "Enter the admin bearer token to unlock profile management.",
    );
    setMessage("admin token required", "error");
    return;
  }
  localStorage.setItem("hoststamp.adminToken", state.adminToken);
  await validateAdminToken();
});
wire("refresh-profiles", "click", refreshProfiles);
wire("refresh-capacity", "click", refreshCapacity);
wire("refresh-history", "click", refreshProfileHistory);
wire("refresh-events", "click", refreshEvents);
wire("apply-events", "click", refreshEvents);
wire("reset-events", "click", resetEvents);
wire("create-profile", "submit", createProfile);
wire("clone-profile", "submit", cloneProfile);
wire("generate", "click", generate);
wire("regenerate", "click", regenerate);
wire("save-access", "click", saveAccess);
wire("export-profile", "click", exportProfile);
wire("import-profile", "click", startImportProfile);
wire("import-profile-file", "change", importProfile);
wire("save-config", "click", saveConfig);
wire("reset-atomic", "click", resetAtomic);
wire("refresh-tokens", "click", refreshTokens);
wire("create-token", "submit", createToken);

refreshHealth();
setInterval(refreshHealth, 5000);
setManagementEnabled(false);
clearProfileState();
if (state.adminToken) {
  validateAdminToken();
} else {
  lockManagement(
    "Auth required",
    "Enter the admin bearer token to unlock profile management.",
  );
}
