/* ═══════════════════════════════════════════════════════
   CodeTrackr — api.js
   Handles: API communication, HTTP requests
   ═══════════════════════════════════════════════════════ */

import { getCurrentToken, getRefreshToken, refreshAccessToken, logout } from './auth.js';

const API = '/api/v1';
let refreshInFlight = null;

const MAX_CONCURRENT = 10;
const MIN_INTERVAL_MS = 150; // ~7 req/s to avoid server burst limits
const MAX_RETRIES = 5;

let inFlight = 0;
let lastRequestAt = 0;
let pumpTimer = null;
const queue = [];
const inflightGets = new Map();
let meCache = null;
let meCacheAt = 0;
const ME_CACHE_MS = 10000;

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function schedule(task) {
  return new Promise((resolve, reject) => {
    queue.push({ task, resolve, reject });
    pump();
  });
}

function pump() {
  if (inFlight >= MAX_CONCURRENT || queue.length === 0) return;
  const now = Date.now();
  const wait = Math.max(0, MIN_INTERVAL_MS - (now - lastRequestAt));
  if (wait > 0) {
    if (pumpTimer) return;
    pumpTimer = setTimeout(() => {
      pumpTimer = null;
      pump();
    }, wait);
    return;
  }

  const { task, resolve, reject } = queue.shift();
  inFlight += 1;
  lastRequestAt = Date.now();
  Promise.resolve()
    .then(task)
    .then((result) => {
      inFlight -= 1;
      resolve(result);
      pump();
    })
    .catch((err) => {
      inFlight -= 1;
      reject(err);
      pump();
    });
}

async function refreshIfNeeded() {
  if (refreshInFlight) return refreshInFlight;
  refreshInFlight = refreshAccessToken()
    .catch((err) => {
      refreshInFlight = null;
      throw err;
    })
    .then((data) => {
      refreshInFlight = null;
      return data;
    });
  return refreshInFlight;
}

async function fetchWithRetry(url, options) {
  let res;
  for (let attempt = 0; attempt <= MAX_RETRIES; attempt += 1) {
    res = await fetch(url, options);
    if (res.status !== 429) return res;

    // Retry-After handling
    let waitMs = 0;
    const retryAfter = res.headers.get('Retry-After');
    if (retryAfter) {
      if (!Number.isNaN(Number(retryAfter))) {
        waitMs = Number(retryAfter) * 1000;
      } else {
        // Handle Date format: "Wed, 21 Oct 2015 07:28:00 GMT"
        const date = Date.parse(retryAfter);
        if (!Number.isNaN(date)) {
          waitMs = Math.max(0, date - Date.now());
        }
      }
    }

    // Default backoff if Retry-After is missing or invalid:
    // 500ms, 1s, 2s, 4s, 8s + jitter
    if (waitMs <= 0) {
      const backoffMs = Math.min(500 * (2 ** attempt), 10000);
      const jitter = Math.floor(Math.random() * 500);
      waitMs = backoffMs + jitter;
    }

    console.warn(`Rate limited (429) on ${url}. Retrying in ${waitMs}ms (attempt ${attempt + 1}/${MAX_RETRIES})`);
    await sleep(waitMs);
  }
  return res;
}

async function parseJson(res) {
  if (res.status === 204) return null;
  const text = await res.text();
  if (!text) return null;
  return JSON.parse(text);
}

async function apiInternal(path, options = {}) {
  const headers = { 'Content-Type': 'application/json', ...(options.headers || {}) };
  let token = getCurrentToken();
  if (token) headers['Authorization'] = `Bearer ${token}`;

  const url = `${API}${path}`;
  const res = await fetchWithRetry(url, { credentials: 'same-origin', ...options, headers });

  if (res.status === 401) {
    const refreshToken = getRefreshToken();
    if (refreshToken) {
      try {
        await refreshIfNeeded();
        token = getCurrentToken();
        const retryHeaders = { 'Content-Type': 'application/json', ...(options.headers || {}) };
        if (token) retryHeaders['Authorization'] = `Bearer ${token}`;
        const retryRes = await fetchWithRetry(url, { credentials: 'same-origin', ...options, headers: retryHeaders });
        if (!retryRes.ok) throw new Error(`API error ${retryRes.status}`);
        return parseJson(retryRes);
      } catch (e) {
        logout();
        throw new Error('API error 401');
      }
    } else if (token) {
      logout();
    }
  }

  if (!res.ok) {
    const errBody = await res.text().catch(() => '');
    let errMsg = `API error ${res.status}`;
    try { const j = JSON.parse(errBody); if (j.error) errMsg = j.error; } catch (_) {}
    throw new Error(errMsg);
  }
  return parseJson(res);
}

export async function api(path, options = {}) {
  const method = (options.method || 'GET').toUpperCase();
  const dedupeKey = method === 'GET' ? `${method} ${path}` : null;
  const isMe = method === 'GET' && path === '/user/me';

  if (isMe && meCache && (Date.now() - meCacheAt) < ME_CACHE_MS) {
    return meCache;
  }

  if (dedupeKey && inflightGets.has(dedupeKey)) {
    return inflightGets.get(dedupeKey);
  }

  const requestPromise = schedule(() => apiInternal(path, options))
    .then((data) => {
      if (isMe) {
        meCache = data;
        meCacheAt = Date.now();
      }
      return data;
    })
    .catch((err) => {
      if (isMe && meCache) return meCache;
      throw err;
    });
  if (dedupeKey) {
    inflightGets.set(dedupeKey, requestPromise);
    requestPromise.finally(() => inflightGets.delete(dedupeKey));
  }

  return requestPromise;
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
