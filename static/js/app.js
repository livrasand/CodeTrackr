/* ═══════════════════════════════════════════════════════
   CodeTrackr — app.js (Modular Version)
   Entry point: imports and orchestrates all modules
   ═══════════════════════════════════════════════════════ */

// Core modules
import { initAuth, initLandingPage } from './modules/init.js';
import { initNav, handlePopState } from './modules/navigation.js';
import { logout } from './modules/auth.js';
import { applyActiveTheme } from './modules/theme.js';
import { connectWebSocket } from './modules/websocket.js';
import { getCurrentToken } from './modules/auth.js';
import { showLanding } from './modules/router.js';
import { getWebSocket } from './modules/websocket.js';
import './modules/billing.js'; // Import billing to register startCheckout globally

// Global exports for inline onclick handlers
window.logout = logout;

// Main initialization
document.addEventListener('DOMContentLoaded', async () => {
  await initAuth();
  initNav();
  await applyActiveTheme();
  
  if (window.location.pathname === '/' || window.location.pathname === '') {
    await initLandingPage();
  }
});

// Handle browser back/forward
window.addEventListener('popstate', handlePopState);

// Handle visibility change for WebSocket reconnection
document.addEventListener('visibilitychange', () => {
  const dash = document.getElementById('dashboard');
  if (document.visibilityState === 'visible' && getCurrentToken() && dash && !dash.classList.contains('hidden')) {
    const ws = getWebSocket();
    if (!ws || ws.readyState === WebSocket.CLOSED) {
      connectWebSocket();
    }
  }
});

console.log('CodeTrackr modular app loaded');

