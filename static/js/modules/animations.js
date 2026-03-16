/* ═══════════════════════════════════════════════════════
   CodeTrackr — animations.js
   Handles: scroll animations, counters, visual effects
   ═══════════════════════════════════════════════════════ */

import { fmt } from './api.js';

export function initScrollAnimations() {
  const elements = document.querySelectorAll(
    '.feature-card, .pricing-card, .step-card, .dash-chart-card, .plugin-code-block'
  );
  elements.forEach(el => el.classList.add('animate-on-scroll'));

  const observer = new IntersectionObserver((entries) => {
    entries.forEach(entry => {
      if (entry.isIntersecting) {
        entry.target.classList.add('visible');
      }
    });
  }, { threshold: 0.1, rootMargin: '0px 0px -40px 0px' });

  elements.forEach(el => observer.observe(el));
}

export function animateCounters() {
  const counters = document.querySelectorAll('[data-target]');
  counters.forEach(counter => {
    const target = parseInt(counter.dataset.target);
    const duration = 2000;
    const start = performance.now();
    const update = (time) => {
      const elapsed = time - start;
      const progress = Math.min(elapsed / duration, 1);
      const eased = 1 - Math.pow(1 - progress, 3);
      const value = Math.floor(eased * target);
      counter.textContent = fmt.num(value);
      if (progress < 1) requestAnimationFrame(update);
    };
    requestAnimationFrame(update);
  });
}
