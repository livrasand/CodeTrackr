/* ═══════════════════════════════════════════════════════
   CodeTrackr — leaderboard.js
   Handles: leaderboard loading, tabs, real-time updates
   ═══════════════════════════════════════════════════════ */

import { $ } from './ui.js';
import { api, fmt } from './api.js';

let currentLbTab = 'global';
let lbInterval = null;

export function getCurrentLbTab() {
  return currentLbTab;
}

export function initLeaderboard() {
  bindLbTabs();
  loadLeaderboard('global').then(injectLanguageTabs);

  // Auto-refresh every 30s
  if (lbInterval) clearInterval(lbInterval);
  lbInterval = setInterval(() => {
    loadLeaderboard(currentLbTab);
  }, 30_000);
}

function bindLbTabs() {
  const container = $('lb-tabs');
  if (!container) return;
  container.querySelectorAll('.code-tab').forEach(tab => {
    tab.addEventListener('click', () => {
      container.querySelectorAll('.code-tab').forEach(t => t.classList.remove('active'));
      tab.classList.add('active');
      currentLbTab = tab.dataset.tab;
      loadLeaderboard(currentLbTab);
    });
  });
}

async function injectLanguageTabs() {
  const container = $('lb-tabs');
  if (!container) return;

  try {
    const data = await api('/leaderboards/global?limit=50');
    const entries = data.leaderboard || [];

    // Count language occurrences
    const langCount = {};
    entries.forEach(e => {
      if (e.top_language) {
        langCount[e.top_language] = (langCount[e.top_language] || 0) + 1;
      }
    });

    // Top 3 languages sorted by frequency
    const top3 = Object.entries(langCount)
      .sort((a, b) => b[1] - a[1])
      .slice(0, 3)
      .map(([lang]) => lang);

    if (top3.length === 0) return;

    // Remove any previously injected language tabs
    container.querySelectorAll('.code-tab[data-dynamic]').forEach(el => el.remove());

    // Find the live indicator span to insert before it
    const liveSpan = container.querySelector('span');

    top3.forEach(lang => {
      const btn = document.createElement('button');
      btn.className = 'code-tab';
      btn.dataset.tab = lang.toLowerCase();
      btn.dataset.dynamic = '1';
      btn.textContent = lang;
      btn.addEventListener('click', () => {
        container.querySelectorAll('.code-tab').forEach(t => t.classList.remove('active'));
        btn.classList.add('active');
        currentLbTab = lang.toLowerCase();
        loadLeaderboard(currentLbTab);
      });
      container.insertBefore(btn, liveSpan);
    });
  } catch (e) {
    // No data yet, no tabs injected
  }
}

export async function loadLeaderboard(tab) {
  const loadingEl = $('lb-loading');
  const rowsEl = $('lb-rows');
  if (!rowsEl) return;

  rowsEl.innerHTML = Array.from({ length: 5 }, () => `
    <tr>
      <td colspan="5" style="padding: 16px; opacity: 0.5;">Loading fast stats...</td>
    </tr>
  `).join('');

  if (loadingEl) loadingEl.style.display = 'none';

  const hireOnly = $('lb-filter-hire')?.checked;

  try {
    let endpoint = tab === 'global'
      ? '/leaderboards/global?limit=10'
      : `/leaderboards/language/${tab}?limit=10`;

    if (hireOnly) endpoint += '&available_for_hire=true';

    const data = await api(endpoint);
    const entries = data.leaderboard || [];

    if (entries.length === 0) {
      rowsEl.innerHTML = `<tr><td colspan="5" style="text-align:center; padding: 24px; color: var(--text-muted);">No data yet this week. Start coding!</td></tr>`;
      return;
    }

    rowsEl.innerHTML = entries.map((e) => {
      const username = e.username || '';
      let displayName = username || 'Unknown';
      if (displayName.length > 12) displayName = displayName.substring(0, 10) + '...';
      const nameCell = username
        ? `<a href="javascript:void(0)" onclick="openPublicProfile('${username}')" style="color:var(--text-main); text-decoration:underline; cursor:pointer;">${displayName}</a>`
        : displayName;
      return `
        <tr>
          <td class="td-main">${nameCell}</td>
          <td class="td-main">${fmt.seconds(e.seconds)}</td>
          <td>${e.top_language || e.language || '—'}</td>
          <td>${e.top_editor || '—'}</td>
          <td>${e.top_os || '—'}</td>
        </tr>
      `;
    }).join('');
  } catch (e) {
    rowsEl.innerHTML = `<tr><td colspan="5" style="text-align:center; padding: 24px; color: var(--text-muted);">Leaderboard loading failed.</td></tr>`;
  }
}
