/* ═══════════════════════════════════════════════════════
   CodeTrackr — navigation.js
   Handles: navigation, hamburger menu, routing
   ═══════════════════════════════════════════════════════ */

import { $ } from './ui.js';
import { isLoggedIn } from './auth.js';
import { showLanding } from './router.js';
import { openPublicProfile } from './profile.js';

export function initNav() {
  // Hamburger
  const hamburger = $('nav-hamburger');
  const navLinks = $('nav-links');
  if (hamburger && navLinks) {
    hamburger.addEventListener('click', () => {
      navLinks.classList.toggle('open');
    });
  }

  // Smooth close on link click
  document.querySelectorAll('.nav-link').forEach(link => {
    link.addEventListener('click', () => {
      if (navLinks) navLinks.classList.remove('open');
    });
  });
}

// Handle browser back/forward
export function handlePopState() {
  const profileMatch = window.location.pathname.match(/^\/u\/([^/]+)$/);
  if (profileMatch) {
    openPublicProfile(profileMatch[1]);
  } else {
    const pp = $('public-profile');
    if (pp) pp.classList.add('hidden');
    if (isLoggedIn()) {
      const dash = $('dashboard');
      if (dash) dash.classList.remove('hidden');
    } else {
      showLanding();
    }
  }
}
