/* ═══════════════════════════════════════════════════════
   CodeTrackr — websocket.js
   Handles: WebSocket connections, real-time updates
   ═══════════════════════════════════════════════════════ */

import { getCurrentToken } from './auth.js';
import { api } from './api.js';

let WS = null;
let _wsRetryDelay = 5000;
let _wsRetryCount = 0;
const _wsMaxRetries = 5;

export async function connectWebSocket() {
  const token = getCurrentToken();
  if (!token) return;
  if (_wsRetryCount >= _wsMaxRetries) return;
  const dash = document.getElementById('dashboard');
  if (!dash || dash.classList.contains('hidden')) return;

  let ticket;
  try {
    const data = await api('/ws-ticket', { method: 'POST' });
    ticket = data.ticket;
  } catch (e) {
    _wsRetryCount++;
    _wsRetryDelay = Math.min(_wsRetryDelay * 2, 60000);
    setTimeout(connectWebSocket, _wsRetryDelay);
    return;
  }

  const proto = location.protocol === 'https:' ? 'wss' : 'ws';
  WS = new WebSocket(`${proto}://${location.host}/ws?ticket=${encodeURIComponent(ticket)}`);

  WS.onopen = () => { _wsRetryDelay = 5000; _wsRetryCount = 0; };
  WS.onclose = (e) => {
    if (e.code === 1000) return;
    // Closed due to tab suspension / device sleep — reconnect when page becomes visible again
    if (document.hidden) return;
    _wsRetryCount++;
    _wsRetryDelay = Math.min(_wsRetryDelay * 2, 60000);
    if (_wsRetryCount < _wsMaxRetries) setTimeout(connectWebSocket, _wsRetryDelay);
  };
  WS.onerror = () => {};

  WS.onmessage = (event) => {
    try {
      const data = JSON.parse(event.data);
      if (data.type === 'heartbeat') {
        // Pulse the today card
        const todayCard = document.getElementById('dcard-today');
        if (todayCard) {
          todayCard.style.borderColor = 'var(--purple-500)';
          setTimeout(() => { todayCard.style.borderColor = ''; }, 800);
        }
        // Refresh stats debounced
        clearTimeout(connectWebSocket._refreshTimer);
        connectWebSocket._refreshTimer = setTimeout(async () => {
          const { loadDashSummary } = await import('./dashboard.js');
          const { loadDashDaily } = await import('./dashboard.js');
          loadDashSummary();
          loadDashDaily();
        }, 3000);
      }
      if (data.type === 'leaderboard_update') {
        // Import dynamically to avoid circular dependencies
        import('./leaderboard.js').then(module => {
          module.loadLeaderboard(module.getCurrentLbTab());
        });
      }
    } catch { }
  };
}

export function getWebSocket() {
  return WS;
}
