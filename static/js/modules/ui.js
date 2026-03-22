/* ═══════════════════════════════════════════════════════
   CodeTrackr — ui.js
   Handles: UI updates, user interface state management
   ═══════════════════════════════════════════════════════ */

import { isLoggedIn, getCurrentUser, setCurrentUser } from './auth.js';
import { api } from './api.js';
import { avatarUrlForUser } from './avatar.js';

// DOM utilities
export const $ = (id) => document.getElementById(id);

// User interface updates
export async function updateUserUI() {
  const authItem = $('nav-auth-item');
  const userItem = $('nav-user-item');
  const avatar = $('nav-avatar');
  const username = $('nav-username');

  if (isLoggedIn()) {
    try {
      if (!getCurrentUser()) {
        const user = await api('/user/me');
        setCurrentUser(user);
      }
      
      const user = getCurrentUser();
      if (authItem) authItem.style.display = 'none';
      if (userItem) userItem.style.display = 'block';
      if (avatar) avatar.src = avatarUrlForUser(user);
      if (username) username.textContent = user.username;

      const publishBtn = $('btn-publish-plugin');
      if (publishBtn) publishBtn.style.display = 'inline-flex';
      const editorBtn = $('btn-open-editor');
      if (editorBtn) editorBtn.style.display = 'inline-flex';
    } catch (e) {
      console.warn('Profile fetch failed', e);
    }
  } else {
    if (authItem) authItem.style.display = 'block';
    if (userItem) userItem.style.display = 'none';
  }
}

// Pro features UI
export function applyProFeatures(user) {
  const isPro = user && user.plan === 'pro';
  const upgradeBtn = $('btn-upgrade-pro');
  const proBadge = $('dash-pro-badge');

  if (upgradeBtn) upgradeBtn.style.display = isPro ? 'none' : 'inline-flex';
  if (proBadge) proBadge.style.display = isPro ? 'inline' : 'none';
}

// Toast notifications
export function showToast(message, actions = [], durationMs = 8000, type = '') {
  const container = $('toast-container');
  if (!container) return;

  const borderColor = type === 'success' ? '#4ade80'
    : type === 'danger'  ? '#e53'
    : type === 'warning' ? '#facc15'
    : type === 'info'    ? '#60a5fa'
    : 'var(--border-focus)';

  const toast = document.createElement('div');
  toast.style.cssText = `
    background: var(--bg-card);
    border: 1px solid ${borderColor};
    border-radius: var(--radius);
    padding: 12px 16px;
    font-size: 12px;
    color: var(--text-muted);
    display: flex;
    align-items: center;
    gap: 12px;
    pointer-events: all;
    min-width: 260px;
    max-width: 380px;
    box-shadow: 0 4px 24px rgba(0,0,0,.4);
    animation: fadeInUp .2s ease;
  `;

  const text = document.createElement('span');
  text.style.flex = '1';
  text.textContent = message;
  toast.appendChild(text);

  actions.forEach(({ label, onClick }) => {
    const btn = document.createElement('button');
    btn.className = 'btn';
    btn.style.cssText = 'padding:2px 8px; font-size:11px; flex-shrink:0;';
    btn.textContent = label;
    btn.addEventListener('click', () => { onClick(); toast.remove(); });
    toast.appendChild(btn);
  });

  const close = document.createElement('button');
  close.style.cssText = 'background:none; border:none; color:var(--text-dark); cursor:pointer; font-size:14px; flex-shrink:0; padding:0;';
  close.textContent = '✕';
  close.addEventListener('click', () => toast.remove());
  toast.appendChild(close);

  container.appendChild(toast);
  if (durationMs > 0) setTimeout(() => toast.remove(), durationMs);
}

// Utility for setting element text
export function setEl(id, val) {
  const el = $(id);
  if (el) el.textContent = val;
}
