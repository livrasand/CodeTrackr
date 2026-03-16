/* ═══════════════════════════════════════════════════════
   CodeTrackr — dashboard.js
   Handles: dashboard loading, stats, charts, user data
   ═══════════════════════════════════════════════════════ */

import { showDashboard } from './router.js';
import { $, setEl, applyProFeatures } from './ui.js';
import { api, fmt } from './api.js';
import { getCurrentUser, setCurrentUser, getCurrentToken } from './auth.js';
import { connectWebSocket } from './websocket.js';
import { openPublicProfile } from './profile.js';

export async function loadDashboard() {
  showDashboard();

  // Set greeting and date
  const hour = new Date().getHours();
  const greeting = hour < 12 ? 'Good morning' : hour < 18 ? 'Good afternoon' : 'Good evening';

  try {
    const user = await api('/user/me');
    setCurrentUser(user);
    
    const nameEl = $('dash-username');
    if (nameEl) nameEl.textContent = user.username;
    const avatarEl = $('dash-avatar');
    if (avatarEl && user.avatar_url) avatarEl.src = user.avatar_url;
    const greetEl = $('dash-greeting');
    if (greetEl) greetEl.textContent = `${greeting}, ${user.display_name || user.username}!`;
    applyProFeatures(user);
    const editorBtn = $('btn-open-editor');
    if (editorBtn) editorBtn.style.display = 'inline-flex';
  } catch (e) {
    console.warn('Could not load user:', e);
  }

  const dateEl = $('dash-date');
  if (dateEl) dateEl.textContent = new Date().toLocaleDateString('en-US', { weekday: 'long', year: 'numeric', month: 'long', day: 'numeric' });

  // Load stats
  await Promise.allSettled([
    loadDashSummary(),
    loadDashDaily(),
    loadDashLanguages(),
    loadDashProjects(),
    loadDashWorkTypes(),
    loadDashSessions(),
    loadApiKey(),
    loadPluginPanels(),
    loadAdminPanelIfNeeded(),
    loadProfileSettings(),
  ]);

  // Init key action buttons
  initKeyActions();

  // Connect WebSocket
  connectWebSocket();
}

async function loadDashSummary() {
  try {
    const [week, allTime, streaks] = await Promise.all([
      api('/stats/summary?range=7d'),
      api('/stats/summary?range=all'),
      api('/stats/streaks'),
    ]);

    const todayStart = new Date(); todayStart.setHours(0, 0, 0, 0);
    const todayData = await api(`/stats/summary?start=${todayStart.toISOString()}`);

    setEl('dcard-today-val', fmt.seconds(todayData.total_seconds));
    setEl('dcard-today-lang', todayData.top_language ? `Top: ${todayData.top_language}` : '—');
    setEl('dcard-week-val', fmt.seconds(week.total_seconds));
    setEl('dcard-week-proj', week.top_project ? `Top: ${week.top_project}` : '—');
    setEl('dcard-streak-val', streaks.longest_streak ?? 0);
    setEl('dcard-lang-val', week.top_language || '—');
    setEl('dash-streak', streaks.current_streak ?? 0);
  } catch (e) {
    console.warn('Summary error:', e);
  }
}

async function loadDashDaily() {
  try {
    const { daily } = await api('/stats/daily?range=7d');
    const container = $('chart-daily-container');
    if (!container || !daily) return;
    container.innerHTML = '';
    const maxVal = Math.max(...daily.map(d => d.seconds), 1);
    daily.forEach(d => {
      const bar = document.createElement('div');
      bar.className = 'chart-bar';
      const pct = Math.max((d.seconds / maxVal) * 100, 2);
      bar.style.height = `${pct}%`;
      bar.innerHTML = `<div class="chart-bar-tooltip">${fmt.seconds(d.seconds)}<br/>${d.date}</div>`;
      container.appendChild(bar);
    });
  } catch (e) {
    console.warn('Daily chart error:', e);
  }
}

async function loadDashLanguages() {
  try {
    const { languages } = await api('/stats/languages?range=7d');
    const container = $('lang-bars');
    if (!container || !languages) return;
    container.innerHTML = '';
    const langColors = {
      rust: '#f74c00', python: '#3776ab', typescript: '#3178c6',
      javascript: '#f7df1e', go: '#00add8', java: '#ed8b00',
      'c++': '#00599c', swift: '#fa7343', kotlin: '#7f52ff',
    };
    languages.slice(0, 6).forEach(l => {
      const color = langColors[l.language.toLowerCase()] || 'var(--purple-500)';
      container.insertAdjacentHTML('beforeend', `
        <div class="lang-bar-item">
          <div class="lang-bar-header">
            <span class="lang-bar-name">${l.language}</span>
            <span class="lang-bar-pct">${l.percentage.toFixed(1)}%</span>
          </div>
          <div class="lang-bar-track">
            <div class="lang-bar-fill" style="width: ${l.percentage}%; background: ${color}"></div>
          </div>
        </div>
      `);
    });
    const topLang = languages[0];
    if (topLang) {
      setEl('dcard-lang-val', topLang.language);
      setEl('dcard-lang-pct', `${topLang.percentage.toFixed(0)}% this week`);
    }
  } catch (e) {
    console.warn('Language error:', e);
  }
}

async function loadDashProjects() {
  try {
    const { projects } = await api('/stats/projects?range=7d');
    const container = $('dash-projects-list');
    if (!container || !projects || projects.length === 0) return;
    const maxSec = Math.max(...projects.map(p => p.seconds), 1);
    container.innerHTML = projects.slice(0, 8).map(p => {
      const pct = Math.round((p.seconds / maxSec) * 100);
      return `
        <div style="margin-bottom:8px;">
          <div style="display:flex; justify-content:space-between; font-size:11px; color:var(--text-dark); margin-bottom:3px;">
            <span style="font-family:var(--font-mono);">${p.project}</span><span>${fmt.seconds(p.seconds)}</span>
          </div>
          <div style="background:var(--border); border-radius:2px; height:4px;">
            <div style="background:var(--text-dark); width:${pct}%; height:100%; border-radius:2px;"></div>
          </div>
        </div>`;
    }).join('');
  } catch (e) {
    console.warn('Projects error:', e);
  }
}

async function loadDashWorkTypes() {
  const container = $('dash-work-types-bars');
  if (!container) return;
  try {
    const { work_types, total_seconds } = await api('/stats/work-types?range=7d');
    if (!work_types || total_seconds === 0) {
      container.innerHTML = `<span style="font-size:12px; color:var(--text-muted);">No data yet.</span>`;
      return;
    }
    const workColors = {
      'Writing code':    'var(--accent)',
      'Debugging':       '#e05252',
      'Reading code':    '#5b9bd5',
      'Config / tooling':'var(--text-dark)',
    };
    const sorted = [...work_types].sort((a, b) => b.seconds - a.seconds);
    container.innerHTML = sorted.map(wt => {
      const pct = wt.percentage.toFixed(1);
      const color = workColors[wt.type] || 'var(--text-dark)';
      return `
        <div style="margin-bottom:10px;">
          <div style="display:flex; justify-content:space-between; font-size:11px; color:var(--text-dark); margin-bottom:3px;">
            <span>${wt.type}</span>
            <span>${pct}% &nbsp;<span style="color:var(--text-muted);">${fmt.seconds(wt.seconds)}</span></span>
          </div>
          <div style="background:var(--border); border-radius:2px; height:5px;">
            <div style="background:${color}; width:${pct}%; height:100%; border-radius:2px; transition:width .4s;"></div>
          </div>
        </div>`;
    }).join('');
  } catch (e) {
    console.warn('Work types error:', e);
  }
}

async function loadDashSessions() {
  const container = $('dash-sessions-list');
  const countEl = $('dash-sessions-count');
  if (!container) return;
  try {
    const { sessions, total_sessions } = await api('/stats/sessions?range=7d');
    if (countEl) countEl.textContent = total_sessions ? `${total_sessions} total` : '';
    if (!sessions || sessions.length === 0) {
      container.innerHTML = `<span style="font-size:12px; color:var(--text-muted);">No sessions yet.</span>`;
      return;
    }
    const workIcon = { 'writing': '✏', 'debugging': '🐛', 'reading': '👁', 'config': '⚙' };
    container.innerHTML = sessions.slice(0, 6).map(s => {
      const start = new Date(s.start);
      const timeStr = start.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit' });
      const dateStr = start.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
      const icon = workIcon[s.dominant_work_type] || '💻';
      return `
        <div style="display:flex; align-items:center; gap:10px; padding:6px 0; border-bottom:1px solid var(--border);">
          <span style="font-size:14px; flex-shrink:0;">${icon}</span>
          <div style="flex:1; min-width:0;">
            <div style="display:flex; justify-content:space-between; font-size:11px;">
              <span style="font-family:var(--font-mono); color:var(--text-main); overflow:hidden; text-overflow:ellipsis; white-space:nowrap;">${s.project}</span>
              <span style="color:var(--text-dark); flex-shrink:0; margin-left:8px;">${fmt.seconds(s.duration_seconds)}</span>
            </div>
            <div style="font-size:10px; color:var(--text-muted); margin-top:2px;">
              ${dateStr} ${timeStr}${s.top_language ? ' · ' + s.top_language : ''}${s.dominant_work_type ? ' · ' + s.dominant_work_type : ''}
            </div>
          </div>
        </div>`;
    }).join('');
  } catch (e) {
    console.warn('Sessions error:', e);
  }
}

async function loadApiKey() {
  try {
    const { keys } = await api('/keys');
    const keyEl = $('dash-apikey');
    if (keyEl && keys && keys.length > 0) {
      keyEl.textContent = `${keys[0].key_prefix}••••••••••••••••••••`;
    }
  } catch (e) {
    console.warn('Key error:', e);
  }
}

function formatPanelValue(data) {
  if (!data || typeof data !== 'object') return String(data);
  // Buscar campos comunes de valor legible
  for (const key of ['value', 'count', 'total', 'total_seconds', 'message', 'text', 'result']) {
    if (data[key] !== undefined) {
      if (key === 'total_seconds') return fmt.seconds(data[key]);
      return String(data[key]);
    }
  }
  // Si tiene daily con goal_seconds y actual_seconds, mostrar progreso
  if (data.daily && data.daily.goal_seconds !== undefined) {
    const pct = Math.round((data.daily.actual_seconds / data.daily.goal_seconds) * 100) || 0;
    return `${fmt.seconds(data.daily.actual_seconds)} / ${fmt.seconds(data.daily.goal_seconds)} (${pct}%)`;
  }
  // Primer valor numérico o string encontrado
  for (const val of Object.values(data)) {
    if (typeof val === 'number') return String(val);
    if (typeof val === 'string' && val.length < 60) return val;
  }
  return '—';
}

async function loadPluginPanels() {
  try {
    const { panels } = await api('/plugins/panels');
    const container = $('dash-plugins');
    if (!container) return;
    container.innerHTML = '';
    if (!panels || panels.length === 0) return;

    const uniquePanels = panels.filter((p, i, arr) => arr.findIndex(x => x.panel === p.panel) === i);
    for (const panel of uniquePanels) {
      const div = document.createElement('div');
      div.className = 'plugin-panel card';
      div.style.width = '100%';
      div.id = `plugin-panel-${panel.panel}`;
      div.setAttribute('data-panel-name', panel.panel);
      div.innerHTML = `
        <div style="display:flex; justify-content:space-between; align-items:flex-start;">
          <div style="display:flex; align-items:center; gap:8px;">
            <span class="panel-drag-handle" title="Drag to reorder" style="cursor:grab; color:var(--text-dark); font-size:14px; line-height:1; user-select:none;">⠿</span>
            <h4 style="margin:0 0 0;">${panel.title}</h4>
          </div>
          <button class="btn" style="padding:2px 8px; font-size:10px; color:var(--text-dark);" onclick="uninstallPlugin('${panel.plugin_id || panel.panel}', this)">✕</button>
        </div>
        <div class="panel-val" id="pval-${panel.panel}" style="margin-top:8px;"></div>
      `;
      container.appendChild(div);

      // Plugin update gating: only run the accepted script
      if (panel.plugin_id) {
        const panelEl = $(`pval-${panel.panel}`);
        const latestVersion = panel.version;
        const latestScript = panel.script || '';
        const acceptedVersion = panel.accepted_version || null;
        const acceptedScript = panel.accepted_script || '';

        if (!acceptedVersion) {
          // First install — require manual review before running
          const { showToast } = await import('./ui.js');
          showToast(
            `${panel.title} — review script before activating`,
            [{
              label: 'Review & Activate',
              onClick: () => openPluginDiffModal(panel.title, null, latestVersion, '', latestScript, panel.plugin_id)
            }],
            0
          );
        } else if (acceptedVersion === latestVersion) {
          // Up to date — run accepted script
          if (acceptedScript && panelEl) runPluginScript(acceptedScript, panelEl, getCurrentToken());
        } else {
          // Update available — run OLD accepted script, show toast
          if (acceptedScript && panelEl) runPluginScript(acceptedScript, panelEl, getCurrentToken());
          const { showToast } = await import('./ui.js');
          showToast(
            `${panel.title} — update available (${latestVersion})`,
            [{
              label: 'Update',
              onClick: async () => {
                await api(`/store/plugin/${panel.plugin_id}/accept`, { method: 'POST' });
                loadPluginPanels();
              }
            }, {
              label: 'View changes',
              onClick: () => openPluginDiffModal(panel.title, acceptedVersion, latestVersion, acceptedScript, latestScript, panel.plugin_id)
            }],
            0
          );
        }
      }
    }
  } catch (e) {
    console.warn('Plugins error:', e);
  }

  // Initialize drag-and-drop for panel reordering
  if (typeof Sortable !== 'undefined') {
    new Sortable(container, {
      animation: 150,
      handle: '.panel-drag-handle',
      ghostClass: 'panel-drag-ghost',
      forceFallback: true,
      fallbackClass: 'panel-drag-ghost',
      onEnd: function(evt) {
        if (evt.oldIndex === evt.newIndex) return;
        const panelNames = Array.from(container.children).map(el => el.getAttribute('data-panel-name'));
        api('/dashboard/order', { method: 'POST', body: JSON.stringify({ panel_names: panelNames }) });
      }
    });
  }
}

function runPluginScript(script, container, token) {
  try {
    // Wrap the script so it exposes a render(container, token) function
    const fn = new Function('container', 'token', script);
    fn(container, token);
  } catch (e) {
    container.textContent = '⚠ Plugin error';
    console.warn('Plugin script error:', e);
  }
}

// API Key Actions
function initKeyActions() {
  const copyBtn = $('btn-copy-key');
  if (copyBtn) {
    copyBtn.addEventListener('click', () => {
      const key = $('dash-apikey')?.textContent;
      if (key) {
        navigator.clipboard.writeText(key).then(() => {
          copyBtn.textContent = 'Copied!';
          setTimeout(() => { copyBtn.textContent = 'Copy'; }, 2000);
        });
      }
    });
  }

  const newKeyBtn = $('btn-new-key');
  if (newKeyBtn) {
    newKeyBtn.addEventListener('click', async () => {
      try {
        const result = await api('/keys', {
          method: 'POST',
          body: JSON.stringify({ name: 'New Key' }),
        });
        const keyEl = $('dash-apikey');
        if (keyEl && result.key) {
          keyEl.textContent = result.key.key;
          const { showToast } = await import('./ui.js');
          showToast(`New API key created! Copy it now: ${result.key.key}`, [], 12000, 'success');
        }
      } catch (e) {
        const { showToast } = await import('./ui.js');
        showToast('Failed to create key: ' + e.message, [], 4000, 'danger');
      }
    });
  }
}

// Admin panel
export async function loadAdminPanelIfNeeded() {
  const user = getCurrentUser();
  if (!user) return;
  if (!user.is_admin) return;
  const panel = $('admin-panel');
  if (panel) panel.style.display = 'block';
  await Promise.allSettled([loadAdminPlugins(), loadAdminReports()]);
}

// Profile settings
async function loadProfileSettings() {
  const user = getCurrentUser();
  if (!user) return;
  const bioEl = $('profile-bio');
  const websiteEl = $('profile-website');
  const linkEl = $('dash-profile-link');

  if (bioEl) bioEl.value = user.bio || '';
  if (websiteEl) websiteEl.value = user.website || '';
  if (linkEl) {
    linkEl.href = `javascript:void(0)`;
    linkEl.onclick = () => {
      openPublicProfile(user.username);
    };
    linkEl.textContent = `↗ @${user.username}`;
  }

  const togMap = {
    'ptog-public': 'is_public',
    'ptog-activity': 'profile_show_activity',
    'ptog-streak': 'profile_show_streak',
    'ptog-languages': 'profile_show_languages',
    'ptog-projects': 'profile_show_projects',
    'ptog-plugins': 'profile_show_plugins',
    'ptog-hire': 'available_for_hire',
  };
  for (const [elId, field] of Object.entries(togMap)) {
    const el = $(elId);
    if (el) el.checked = !!user[field];
  }
}

// Placeholder for functions that need to be implemented
async function loadAdminPlugins() {
  // Implementation needed
}

async function loadAdminReports() {
  // Implementation needed
}

function openPluginDiffModal(displayName, prevVersion, newVersion, oldScript, newScript, pluginId) {
  // Implementation needed
}

// Export functions that are used elsewhere
export { loadApiKey, initKeyActions, formatPanelValue, loadPluginPanels, runPluginScript };
