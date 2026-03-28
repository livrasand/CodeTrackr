/* ═══════════════════════════════════════════════════════
   CodeTrackr — init.js
   Handles: application initialization, orchestration
   ═══════════════════════════════════════════════════════ */

import { 
  handleOAuthCallback, 
  getTokenFromHash, 
  setToken, 
  loadStoredToken, 
  isLoggedIn 
} from './auth.js';
import { detectRoute, showLanding } from './router.js';
import { updateUserUI } from './ui.js';
import { loadDashboard } from './dashboard.js';
import { openPublicProfile } from './profile.js';

// Main initialization function - now much cleaner
export async function initAuth() {
  // Handle OAuth callback if present
  const isRedirecting = handleOAuthCallback();
  if (isRedirecting) return;

  // Handle token from hash or storage
  const token = getTokenFromHash();
  if (token) {
    setToken(token);
  } else {
    loadStoredToken();
  }

  // Route handling
  const route = detectRoute();
  
  if (route.type === 'profile') {
    if (isLoggedIn()) {
      await updateUserUI();
    }
    await openPublicProfile(route.username);
    return;
  }

  if (isLoggedIn()) {
    await updateUserUI();
    if (document.getElementById('dashboard')) {
      await loadDashboard();
    }
  }
}

// Landing page initialization
export async function initLandingPage() {
  const { initScrollAnimations } = await import('./animations.js');
  const { initLeaderboard } = await import('./leaderboard.js');
  const { initPluginTabs } = await import('./plugins.js');
  const { loadPublicStats } = await import('./stats.js');
  const { loadPluginStore } = await import('./plugin-store.js');
  const { isLoggedIn } = await import('./auth.js');

  initScrollAnimations();
  initLeaderboard();
  initPluginTabs();
  loadPublicStats();

  // Plugin Store initialization
  if (isLoggedIn()) {
    const publishBtn = document.getElementById('btn-publish-plugin');
    if (publishBtn) publishBtn.style.display = 'inline-flex';
    const editorBtn = document.getElementById('btn-open-editor');
    if (editorBtn) editorBtn.style.display = 'inline-flex';
    const extensionsSection = document.getElementById('extensions');
    if (extensionsSection) extensionsSection.style.display = 'none';
  }
  loadPluginStore();

  // Animate counters when hero stats come into view
  const statsSection = document.querySelector('.hero-stats');
  if (statsSection) {
    const { animateCounters } = await import('./animations.js');
    const obs = new IntersectionObserver((entries) => {
      entries.forEach(e => {
        if (e.isIntersecting) {
          animateCounters();
          obs.disconnect();
        }
      });
    }, { threshold: 0.5 });
    obs.observe(statsSection);
  }
}
