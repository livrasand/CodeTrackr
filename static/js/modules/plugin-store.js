/* ═══════════════════════════════════════════════════════
   CodeTrackr — plugin-store.js
   Handles: plugin store, installation, management
   ═══════════════════════════════════════════════════════ */

import { $, showToast } from './ui.js';
import { api } from './api.js';
import { isLoggedIn, getCurrentUser } from './auth.js';

export async function loadPluginStore(filter = 'all') {
  const grid = $('plugin-store-grid');
  if (!grid) return;

  try {
    let endpoint = filter === 'installed' ? '/store/installed' : '/store';
    const data = await api(endpoint);
    const plugins = data.plugins || data.installed || [];

    let installedIds = new Set();
    if (filter === 'all' && isLoggedIn()) {
      try {
        const inst = await api('/store/installed');
        (inst.installed || []).forEach(p => installedIds.add(p.id));
      } catch (_) {}
    }

    if (plugins.length === 0) {
      grid.innerHTML = `<div style="grid-column: 1/-1; text-align:center; color:var(--text-muted); padding:40px;">No plugins found here yet.</div>`;
      return;
    }

    grid.innerHTML = plugins.map(p => {
      const icon = p.icon || '🔌';
      const pid = String(p.id);
      const isInstalled = filter === 'installed' || installedIds.has(pid);
      const avgRating = p.avg_rating ? Number(p.avg_rating).toFixed(1) : null;
      const ratingHtml = avgRating
        ? `<span style="font-size:11px; color:var(--accent);">${starsHtml(Math.round(p.avg_rating), 1)} ${avgRating}</span>`
        : '';
      const installBtn = isInstalled
        ? `<button class="btn" style="flex:1; font-size:12px; height:32px; color:var(--text-dark);" onclick="uninstallPluginFromStore('${pid}', this)">Uninstall</button>`
        : `<button class="btn" style="flex:1; font-size:12px; height:32px;" onclick="installPlugin('${pid}', this)">Install</button>`;
      const reportBtn = isLoggedIn() && !isInstalled && filter !== 'installed' && p.author_username !== getCurrentUser()?.username
        ? `<button class="btn" style="font-size:11px; height:32px; padding:0 10px; color:var(--text-dark);" onclick="openReportModal('${pid}')">⚑</button>`
        : '';
      const deleteBtn = isLoggedIn() && p.author_username === getCurrentUser()?.username
        ? `<button class="btn" style="font-size:11px; height:32px; padding:0 10px; color:#e53;" onclick="authorDeletePlugin('${pid}', this)">Delete</button>`
        : '';
      return (
        `<div class="card plugin-card" style="display:flex; flex-direction:column; cursor:pointer;" data-plugin-id="${pid}" data-installed="${isInstalled ? '1' : '0'}">` +
          `<div style="display:flex; justify-content:space-between; align-items:flex-start; margin-bottom:4px;">` +
            `<h3 style="margin:0; font-size:15px;">${icon} ${p.display_name}</h3>` +
            `<span class="key-hint" style="font-size:10px; padding:2px 6px;">v${p.version}</span>` +
          `</div>` +
          `<div style="font-size:11px; color:var(--text-dark); font-family:var(--font-mono); margin-bottom:4px;">${p.name}</div>` +
          (p.author_username ? `<div style="font-size:11px; color:var(--text-muted); margin-bottom:6px;" onclick="event.stopPropagation(); openPublicProfile('${p.author_username}')">by <span style="cursor:pointer; text-decoration:underline; color:var(--text-dark);">@${p.author_username}</span></div>` : '') +
          `<p style="font-size:12px; margin:4px 0 8px; color:var(--text-muted); flex-grow:1; line-height:1.5;">${p.description || 'No description provided.'}</p>` +
          `<div style="display:flex; gap:6px; align-items:center; margin-bottom:6px;">` +
            `<span style="font-size:11px; color:var(--text-dark);">↓ ${p.install_count || 0}</span>` +
            ratingHtml +
          `</div>` +
          `<div style="margin-top:8px; display:flex; gap:8px;" onclick="event.stopPropagation()">` +
            installBtn + reportBtn + deleteBtn +
          `</div>` +
        `</div>`
      );
    }).join('');

    // Event listener por delegación — evita onclick inline con JSON que rompe atributos HTML
    grid.querySelectorAll('.plugin-card').forEach(card => {
      card.addEventListener('click', (e) => {
        if (e.target.closest('button')) return; // no abrir modal si se clickeó un botón
        const pid = card.dataset.pluginId;
        if (pid) openPluginDetailModal(pid, installedIds);
      });
    });

  } catch (e) {
    console.warn('Store error:', e);
    grid.innerHTML = `<div style="color:var(--text-muted); padding:16px;">Error loading store.</div>`;
  }
}

export async function installPlugin(pluginId, btn) {
  if (!isLoggedIn()) {
    showToast('Please login to install plugins.', [], 4000, 'warning');
    return;
  }

  const originalText = btn.textContent;
  btn.textContent = 'Installing...';
  btn.disabled = true;

  try {
    await api(`/store/install/${pluginId}`, { method: 'POST' });
    btn.textContent = 'Installed!';
    btn.style.borderColor = 'var(--accent)';
    btn.style.color = 'var(--accent)';
    setTimeout(() => { loadPluginStore(); }, 1500);
    // Refresh plugin panels in dashboard
    import('./dashboard.js').then(module => module.loadPluginPanels());
  } catch (e) {
    console.error('Install failed:', e);
    btn.textContent = 'Error';
    btn.disabled = false;
    setTimeout(() => { btn.textContent = originalText; }, 2000);
  }
}

export async function uninstallPluginFromStore(pluginId, btn) {
  btn.textContent = 'Removing...';
  btn.disabled = true;
  try {
    await api(`/store/uninstall/${pluginId}`, { method: 'DELETE' });
    loadPluginStore();
    // Refresh plugin panels in dashboard
    import('./dashboard.js').then(module => module.loadPluginPanels());
  } catch (e) {
    btn.textContent = 'Uninstall';
    btn.disabled = false;
  }
}

function starsHtml(avg, total) {
  const filled = Math.round(avg);
  let s = '';
  for (let i = 1; i <= 5; i++) s += i <= filled ? '★' : '☆';
  return s;
}

// Placeholder for functions that need to be implemented
function openPluginDetailModal(pluginId, installedIds) {
  // Implementation needed - will be in plugin-detail.js
}

// Export functions that are used globally
window.installPlugin = installPlugin;
window.uninstallPluginFromStore = uninstallPluginFromStore;
window.loadPluginStore = loadPluginStore;
