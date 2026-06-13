// SPDX-License-Identifier: FSL-1.1-ALv2

const assert = require("node:assert/strict");
const test = require("node:test");

const health = require("../static/profile-health.js");

const baseProfile = {
  access: "private",
  last_atomic_value: 2,
  config: {
    engine: "atomic-v1",
    suffix: {
      enabled: true,
    },
  },
};

test("capacityHealth uses BigInt for remaining profile capacity", () => {
  const profile = {
    ...baseProfile,
    last_atomic_value: "9007199254740993",
  };
  const capacity = {
    total_variants: "9007199254741995",
  };

  assert.equal(health.integerBigInt("bad"), null);
  assert.equal(health.integerBigInt("9007199254740993"), 9007199254740993n);
  assert.deepEqual(health.capacityHealth(profile, capacity), {
    total: "9007199254741995",
    remaining: "1002",
    remainingValue: 1002n,
  });
});

test("capacityHealth degrades incomplete capacity data without throwing", () => {
  assert.deepEqual(health.capacityHealth(null, null), {
    total: "not loaded",
    remaining: "not loaded",
    remainingValue: null,
  });
  assert.deepEqual(
    health.capacityHealth(baseProfile, { total_variants: "not-a-number" }),
    {
      total: "not-a-number",
      remaining: "not available",
      remainingValue: null,
    },
  );
});

test("tokenHealth buckets active, expired, expiring, and revoked tokens", () => {
  const now = 1_000_000;
  const dayMs = 24 * 60 * 60 * 1000;
  const tokens = [
    { expires_at_ms: null, revoked_at_ms: null },
    { expires_at_ms: now + dayMs, revoked_at_ms: null },
    { expires_at_ms: now - 1, revoked_at_ms: null },
    { expires_at_ms: now + dayMs, revoked_at_ms: now },
  ];

  assert.deepEqual(health.tokenHealth(tokens, now), {
    active: 2,
    expired: 1,
    expiring: 1,
    revoked: 1,
    total: 4,
    summary: "2 active / 4 total",
  });
});

test("tokenStatus uses the same seven-day expiring threshold as tokenHealth", () => {
  const now = 1_000_000;
  const dayMs = 24 * 60 * 60 * 1000;

  assert.equal(
    health.tokenStatus({ expires_at_ms: null, revoked_at_ms: null }, now),
    "active",
  );
  assert.equal(
    health.tokenStatus({ expires_at_ms: now + 8 * dayMs, revoked_at_ms: null }, now),
    "active",
  );
  assert.equal(
    health.tokenStatus({ expires_at_ms: now + 7 * dayMs, revoked_at_ms: null }, now),
    "expiring",
  );
  assert.equal(
    health.tokenStatus({ expires_at_ms: now - 1, revoked_at_ms: null }, now),
    "expired",
  );
  assert.equal(
    health.tokenStatus({ expires_at_ms: now + dayMs, revoked_at_ms: now }, now),
    "revoked",
  );
});

test("historyHealth summarizes active and replaced rows", () => {
  assert.equal(health.historyHealth(null), "not loaded");
  assert.equal(health.historyHealth([]), "none");
  assert.equal(health.historyHealth([{ replaced_at_ms: null }]), "1 active row");
  assert.equal(
    health.historyHealth([{ replaced_at_ms: 1 }, { replaced_at_ms: null }]),
    "2 rows, 1 replaced",
  );
});

test("profileHealthWarnings reports operator-visible risks", () => {
  const now = 1_000_000;
  const profile = {
    ...baseProfile,
    access: "public",
    last_atomic_value: 10,
    config: {
      engine: "legacy-v0",
      suffix: {
        enabled: false,
      },
    },
  };
  const warnings = health.profileHealthWarnings(
    profile,
    { total_variants: "10" },
    [
      { expires_at_ms: now - 1, revoked_at_ms: null },
      { expires_at_ms: now + 1, revoked_at_ms: null },
    ],
    [{ replaced_at_ms: 1 }, { replaced_at_ms: null }],
    now,
  );

  assert.deepEqual(warnings, [
    "public profile access",
    "unsupported engine legacy-v0",
    "suffix disabled",
    "capacity exhausted",
    "1 expired token",
    "1 token expiring within 7 days",
    "replaced profile history",
  ]);
});

test("profileHealthWarnings reports low remaining capacity", () => {
  assert.deepEqual(
    health.profileHealthWarnings(
      baseProfile,
      { total_variants: "1002" },
      [],
      [],
      1_000_000,
    ),
    ["capacity below 1,000 names"],
  );
});
