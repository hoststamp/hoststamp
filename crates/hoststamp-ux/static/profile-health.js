// SPDX-License-Identifier: FSL-1.1-ALv2

(function loadProfileHealth(root) {
  "use strict";

  function integerBigInt(value) {
    const text = String(value);
    return /^-?\d+$/.test(text) ? BigInt(text) : null;
  }

  function capacityHealth(profile, capacity) {
    if (!profile || !capacity) {
      return {
        total: "not loaded",
        remaining: "not loaded",
        remainingValue: null,
      };
    }

    const total = integerBigInt(capacity.total_variants);
    const lastAtomic = integerBigInt(profile.last_atomic_value);
    if (total === null || lastAtomic === null) {
      return {
        total: capacity.total_variants,
        remaining: "not available",
        remainingValue: null,
      };
    }

    const remaining = total - lastAtomic;
    return {
      total: total.toString(),
      remaining: remaining.toString(),
      remainingValue: remaining,
    };
  }

  function tokenHealth(tokens, now = Date.now()) {
    if (!tokens) {
      return {
        summary: "not loaded",
        active: 0,
        expired: 0,
        expiring: 0,
        revoked: 0,
        total: 0,
      };
    }

    const weekMs = 7 * 24 * 60 * 60 * 1000;
    const summary = {
      active: 0,
      expired: 0,
      expiring: 0,
      revoked: 0,
      total: tokens.length,
    };
    for (const token of tokens) {
      if (token.revoked_at_ms !== null) {
        summary.revoked += 1;
        continue;
      }
      if (token.expires_at_ms !== null && token.expires_at_ms <= now) {
        summary.expired += 1;
        continue;
      }
      summary.active += 1;
      if (token.expires_at_ms !== null && token.expires_at_ms - now <= weekMs) {
        summary.expiring += 1;
      }
    }
    return {
      ...summary,
      summary: `${summary.active} active / ${summary.total} total`,
    };
  }

  function historyHealth(profiles) {
    if (!profiles) return "not loaded";
    if (!profiles.length) return "none";
    const replaced = profiles.filter(
      (profile) => profile.replaced_at_ms !== null,
    ).length;
    return replaced
      ? `${profiles.length} rows, ${replaced} replaced`
      : "1 active row";
  }

  function profileHealthWarnings(profile, capacity, tokens, history, now = Date.now()) {
    const warnings = [];
    if (profile.access === "public") {
      warnings.push("public profile access");
    }
    if (profile.config.engine !== "atomic-v1") {
      warnings.push(`unsupported engine ${profile.config.engine}`);
    }
    if (!profile.config.suffix.enabled) {
      warnings.push("suffix disabled");
    }

    const capacityState = capacityHealth(profile, capacity);
    if (capacityState.remainingValue !== null) {
      if (capacityState.remainingValue <= 0n) {
        warnings.push("capacity exhausted");
      } else if (capacityState.remainingValue <= 1000n) {
        warnings.push("capacity below 1,000 names");
      }
    }

    const tokenState = tokenHealth(tokens, now);
    if (tokenState.expired > 0) {
      warnings.push(
        `${tokenState.expired} expired token${tokenState.expired === 1 ? "" : "s"}`,
      );
    }
    if (tokenState.expiring > 0) {
      warnings.push(
        `${tokenState.expiring} token${tokenState.expiring === 1 ? "" : "s"} expiring within 7 days`,
      );
    }
    if (history?.some((row) => row.replaced_at_ms !== null)) {
      warnings.push("replaced profile history");
    }

    return warnings;
  }

  const helpers = {
    capacityHealth,
    historyHealth,
    integerBigInt,
    profileHealthWarnings,
    tokenHealth,
  };

  if (typeof module !== "undefined" && module.exports) {
    module.exports = helpers;
  }
  root.HoststampProfileHealth = helpers;
})(typeof globalThis !== "undefined" ? globalThis : this);
