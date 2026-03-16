/* ═══════════════════════════════════════════════════════
   CodeTrackr — api.js
   Handles: API communication, HTTP requests
   ═══════════════════════════════════════════════════════ */

import { getCurrentToken } from './auth.js';

const API = '/api/v1';

export async function api(path, options = {}) {
  const headers = { 'Content-Type': 'application/json', ...(options.headers || {}) };
  const token = getCurrentToken();
  if (token) headers['Authorization'] = `Bearer ${token}`;
  
  const res = await fetch(`${API}${path}`, { ...options, headers });
  if (!res.ok) throw new Error(`API error ${res.status}`);
  return res.json();
}

// Formatting utilities (moved from main file)
export const fmt = {
  seconds: (s) => {
    if (!s || s === 0) return '0m';
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    if (h === 0) return `${m}m`;
    if (m === 0) return `${h}h`;
    return `${h}h ${m}m`;
  },
  num: (n) => n >= 1_000_000 ? (n / 1_000_000).toFixed(1) + 'M'
    : n >= 1_000 ? (n / 1_000).toFixed(1) + 'k'
      : String(n),
  date: (d) => new Date(d).toLocaleDateString('en-US', { weekday: 'short', month: 'short', day: 'numeric' }),
};
