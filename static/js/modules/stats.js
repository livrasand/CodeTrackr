/* ═══════════════════════════════════════════════════════
   CodeTrackr — stats.js
   Handles: public statistics, data loading
   ═══════════════════════════════════════════════════════ */

import { api, fmt } from './api.js';

export async function loadPublicStats() {
  try {
    const data = await api('/stats/public');
    const devsEl = document.getElementById('stat-devs');
    if (devsEl) devsEl.textContent = `${fmt.num(data.users)} active developers`;
    const hoursEl = document.getElementById('stat-hours');
    if (hoursEl) {
      const hours = Math.floor(data.total_seconds / 3600);
      hoursEl.textContent = `Over ${fmt.num(hours)} hours logged`;
    }
  } catch (e) {
    console.warn('Could not load public stats', e);
  }
}
