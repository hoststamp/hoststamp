// SPDX-License-Identifier: FSL-1.1-ALv2

(() => {
  let currentVersion = null;

  async function checkForUpdate() {
    try {
      const response = await fetch("/assets/dev-version", { cache: "no-store" });
      if (!response.ok) return;
      const nextVersion = await response.text();
      if (!currentVersion) {
        currentVersion = nextVersion;
        return;
      }
      if (nextVersion && nextVersion !== currentVersion) {
        location.reload();
      }
    } catch {
      // The server may be restarting. Try again on the next interval.
    }
  }

  setInterval(checkForUpdate, 750);
  checkForUpdate();
})();
