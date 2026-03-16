/* ═══════════════════════════════════════════════════════
   CodeTrackr — router.js
   Handles: client-side routing, view management
   ═══════════════════════════════════════════════════════ */

import { isLoggedIn } from './auth.js';

// Route detection and handling
export function detectRoute() {
  // Detect /u/:username route
  const profileMatch = window.location.pathname.match(/^\/u\/([^/]+)$/);
  if (profileMatch) {
    const username = profileMatch[1];
    return { type: 'profile', username };
  }
  
  // Detect dashboard route
  if (isLoggedIn() && window.location.pathname === '/') {
    return { type: 'dashboard' };
  }
  
  return { type: 'landing' };
}

// View management
export function showLanding() {
  document.body.querySelectorAll('section, footer, .hero').forEach(el => {
    el.style.display = '';
  });
  const dash = document.getElementById('dashboard');
  if (dash) dash.classList.add('hidden');
  const pp = document.getElementById('public-profile');
  if (pp) pp.classList.add('hidden');
  const nav = document.getElementById('nav');
  if (nav) nav.style.display = '';
}

export function showDashboard() {
  // Hide landing sections
  ['hero', 'stats', 'leaderboard', 'features', 'build', 'plugins', 'pricing', 'about', 'footer', 'nav', 'public-profile'].forEach(id => {
    const el = document.getElementById(id);
    if (el) el.classList.add('hidden');
  });
  const dash = document.getElementById('dashboard');
  if (dash) dash.classList.remove('hidden');
}

export function hideAllViews() {
  ['hero', 'stats', 'leaderboard', 'features', 'build', 'plugins', 'pricing', 'about', 'footer', 'nav', 'dashboard', 'public-profile'].forEach(id => {
    const el = document.getElementById(id);
    if (el) el.classList.add('hidden');
  });
}
