/* ═══════════════════════════════════════════════════════
   CodeTrackr — plugins.js
   Handles: plugin tabs, code tabs functionality
   ═══════════════════════════════════════════════════════ */

export function initPluginTabs() {
  const tabs = document.querySelectorAll('.code-tab');
  tabs.forEach(tab => {
    tab.addEventListener('click', () => {
      // Avoid conflict with leaderboard or store tabs if they use the same class but are handled elsewhere
      if (tab.closest('#leaderboard') || tab.closest('#plugin-store')) return;

      tabs.forEach(t => t.classList.remove('active'));
      tab.classList.add('active');
      document.querySelectorAll('.code-content').forEach(c => c.classList.add('hidden'));
      const target = document.getElementById(`code-${tab.dataset.code}`);
      if (target) target.classList.remove('hidden');
    });
  });
}
