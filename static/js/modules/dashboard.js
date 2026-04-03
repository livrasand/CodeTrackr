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
import { avatarUrlForUser } from './avatar.js';

export async function loadDashboard() {
  showDashboard();

  // Set greeting and date
  const hour = new Date().getHours();
  const greeting = hour < 12 ? 'Good morning' : hour < 18 ? 'Good afternoon' : 'Good evening';

  try {
    const cachedUser = getCurrentUser();
    const user = cachedUser || await api('/user/me');
    if (!cachedUser) setCurrentUser(user);
    
    const nameEl = $('dash-username');
    if (nameEl) nameEl.textContent = user.username;
    const avatarEl = $('dash-avatar');
    if (avatarEl) avatarEl.src = avatarUrlForUser(user);
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
  await loadDashStats();

  // Init key action buttons
  initKeyActions();

  // Connect WebSocket
  connectWebSocket();
}

async function loadDashStats() {
  try {
    const todayStart = new Date(); todayStart.setHours(0, 0, 0, 0);
    // Realizamos solo 2 peticiones en lugar de 8+: una para el dashboard completo (7d) y otra para hoy
    const [dash, todayData] = await Promise.all([
      api('/stats/dashboard?range=7d'),
      api(`/stats/summary?start=${todayStart.toISOString()}`)
    ]);

    // 1. Summary
    if (dash.summary) {
      setEl('dcard-today-val', fmt.seconds(todayData.total_seconds));
      setEl('dcard-today-lang', todayData.top_language ? `Top: ${todayData.top_language}` : '—');
      setEl('dcard-week-val', fmt.seconds(dash.summary.total_seconds));
      setEl('dcard-week-proj', dash.summary.top_project ? `Top: ${dash.summary.top_project}` : '—');
      setEl('dcard-lang-val', dash.summary.top_language || '—');
    }
    if (dash.streaks) {
      setEl('dcard-streak-val', dash.streaks.longest ?? 0);
      setEl('dash-streak', dash.streaks.current ?? 0);
    }

    // 2. Daily Chart
    if (dash.daily) {
      const container = $('chart-daily-container');
      if (container) {
        container.innerHTML = '';
        const maxVal = Math.max(...dash.daily.map(d => d.seconds), 1);
        dash.daily.forEach(d => {
          const bar = document.createElement('div');
          bar.className = 'chart-bar';
          const pct = Math.max((d.seconds / maxVal) * 100, 2);
          bar.style.height = `${pct}%`;
          bar.innerHTML = `<div class="chart-bar-tooltip">${fmt.seconds(d.seconds)}<br/>${d.date}</div>`;
          container.appendChild(bar);
        });
      }
    }

    // 3. Languages
    if (dash.languages) {
      const container = $('lang-bars');
      if (container) {
        container.innerHTML = '';
        const langColors = {
          rust: '#f74c00', python: '#3776ab', typescript: '#3178c6',
          javascript: '#f7df1e', go: '#00add8', java: '#ed8b00',
          'c++': '#00599c', swift: '#fa7343', kotlin: '#7f52ff',
        };
        dash.languages.slice(0, 6).forEach(l => {
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
        if (dash.languages[0]) {
          setEl('dcard-lang-val', dash.languages[0].language);
          setEl('dcard-lang-pct', `${dash.languages[0].percentage.toFixed(0)}% this week`);
        }
      }
    }

    // 4. Projects
    if (dash.projects) {
      const container = $('dash-projects-list');
      if (container && dash.projects.length > 0) {
        const maxSec = Math.max(...dash.projects.map(p => p.seconds), 1);
        container.innerHTML = dash.projects.slice(0, 8).map(p => {
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
      }
    }

    // 5. Work Types
    if (dash.work_types) {
      const container = $('dash-work-types-bars');
      if (container) {
        const { types, total_seconds } = dash.work_types;
        if (!types || total_seconds === 0) {
          container.innerHTML = `<span style="font-size:12px; color:var(--text-muted);">No data yet.</span>`;
        } else {
          const workColors = {
            'Writing code':    'var(--accent)', 'Debugging': '#e05252',
            'Reading code':    '#5b9bd5', 'Config / tooling': 'var(--text-dark)',
          };
          const sorted = [...types].sort((a, b) => b.seconds - a.seconds);
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
        }
      }
    }

    // Load remaining non-combined parts
    await Promise.allSettled([
      loadDashSessions(),
      loadApiKey(),
      loadPluginPanels(),
      loadAdminPanelIfNeeded(),
      loadProfileSettings(),
    ]);

  } catch (e) {
    console.warn('Dashboard stats error:', e);
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
    const workIcon = { 'writing': '✎', 'debugging': '✦', 'reading': '◉', 'config': '⚙' };
    container.innerHTML = sessions.slice(0, 6).map(s => {
      const start = new Date(s.start);
      const timeStr = start.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit' });
      const dateStr = start.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
      const icon = workIcon[s.dominant_work_type] || '▸';
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
    const listEl = $('dash-apikey-list');
    const countEl = $('dash-apikey-count');
    const newBtn = $('btn-new-key');
    if (!listEl) return;

    if (countEl) countEl.textContent = `(${(keys || []).length}/5)`;
    if (newBtn) newBtn.disabled = (keys || []).length >= 5;

    listEl.innerHTML = '';
    if (!keys || keys.length === 0) {
      listEl.innerHTML = '<span style="font-size:12px; color:var(--text-muted);">No keys yet.</span>';
      return;
    }

    const securityNote = document.createElement('p');
    securityNote.style.cssText = 'font-size:11px; color:var(--text-dark); margin:0 0 10px; font-family:var(--font-mono); line-height:1.6;';
    securityNote.textContent = '⚠ Full keys are shown only once at creation time and are never stored in plaintext. For security, they cannot be retrieved after creation. Create a new key if you lost access to one.';
    listEl.appendChild(securityNote);

    keys.forEach(k => {
      const row = document.createElement('div');
      row.dataset.keyId = k.id;
      row.style.cssText = 'display:flex; align-items:center; gap:8px; font-size:12px; font-family:var(--font-mono); padding:4px 0; border-bottom:1px solid var(--border);';
      row.innerHTML = `
        <span data-name-label="${k.id}" style="flex:1; color:var(--text-muted); cursor:pointer;" title="Click to rename">${k.name}</span>
        <code style="color:var(--text-main); flex-shrink:0;">${k.key_prefix}••••••••••••</code>
        <button class="btn" data-delete-key="${k.id}" style="padding:2px 8px; font-size:11px; color:var(--text-dark); flex-shrink:0;">✕</button>
      `;
      listEl.appendChild(row);
    });

    listEl.querySelectorAll('[data-name-label]').forEach(label => {
      label.addEventListener('click', () => {
        const id = label.dataset.nameLabel;
        const current = label.textContent;
        const input = document.createElement('input');
        input.value = current;
        input.style.cssText = 'flex:1; background:var(--bg-input); border:1px solid var(--border-focus); color:var(--text-main); padding:2px 6px; font-size:12px; font-family:var(--font-mono); border-radius:var(--radius-sm); outline:none;';
        label.replaceWith(input);
        input.focus();
        input.select();

        const save = async () => {
          const newName = input.value.trim();
          if (!newName || newName === current) {
            input.replaceWith(label);
            return;
          }
          try {
            await api(`/keys/${id}`, { method: 'PATCH', body: JSON.stringify({ name: newName }) });
            label.textContent = newName;
          } catch (e) {
            label.textContent = current;
            const { showToast } = await import('./ui.js');
            showToast('Failed to rename key: ' + e.message, [], 4000, 'danger');
          }
          input.replaceWith(label);
        };

        input.addEventListener('blur', save);
        input.addEventListener('keydown', e => {
          if (e.key === 'Enter') { e.preventDefault(); input.blur(); }
          if (e.key === 'Escape') { input.value = current; input.blur(); }
        });
      });
    });

    listEl.querySelectorAll('[data-delete-key]').forEach(btn => {
      btn.addEventListener('click', () => {
        const id = btn.dataset.deleteKey;
        const row = listEl.querySelector(`[data-key-id="${id}"]`);
        const nameEl = row?.querySelector('[data-name-label]');
        const keyName = nameEl?.textContent || 'this key';

        const overlay = document.createElement('div');
        overlay.style.cssText = 'position:fixed; inset:0; background:rgba(0,0,0,.5); z-index:1000; display:flex; align-items:center; justify-content:center;';
        overlay.innerHTML = `
          <div style="background:var(--bg-card); border:1px solid var(--border); border-radius:var(--radius); padding:24px; max-width:340px; width:90%; font-size:13px;">
            <p style="color:var(--text-main); margin:0 0 6px; font-weight:500;">Delete API key?</p>
            <p style="color:var(--text-muted); margin:0 0 20px; font-size:12px;">
              <code style="color:var(--text-main);">${keyName}</code> will be permanently deleted. Any application using it will stop working immediately.
            </p>
            <div style="display:flex; gap:8px; justify-content:flex-end;">
              <button class="btn" id="cancel-delete-key" style="font-size:12px; padding:4px 14px;">Cancel</button>
              <button class="btn" id="confirm-delete-key" style="font-size:12px; padding:4px 14px; border-color:var(--border-focus); color:var(--text-main);">Delete</button>
            </div>
          </div>
        `;

        document.body.appendChild(overlay);

        overlay.querySelector('#cancel-delete-key').addEventListener('click', () => overlay.remove());
        overlay.addEventListener('click', e => { if (e.target === overlay) overlay.remove(); });

        overlay.querySelector('#confirm-delete-key').addEventListener('click', async () => {
          overlay.remove();
          try {
            await api(`/keys/${id}`, { method: 'DELETE' });
            await loadApiKey();
          } catch (e) {
            const { showToast } = await import('./ui.js');
            showToast('Failed to delete key: ' + e.message, [], 4000, 'danger');
          }
        });
      });
    });
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

const _panelDataMap = new Map();

async function loadPluginPanels() {
  const container = $('dash-plugins');
  if (!container) return;
  try {
    const { panels } = await api('/plugins/panels');
    container.innerHTML = '';
    _panelDataMap.clear();
    if (!panels || panels.length === 0) return;

    const uniquePanels = panels
      .filter((p, i, arr) => arr.findIndex(x => x.panel === p.panel) === i)
      .filter(p => p.plugin_type !== 'lifecycle' && p.widget_type !== null && p.widget_type !== undefined);
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
          // Guardar datos del panel en el Map para referenciarlos desde el botón
          _panelDataMap.set(panel.plugin_id, { title: panel.title, latestVersion, latestScript });
          // First install — require manual review before running
          if (panelEl) {
            panelEl.innerHTML = `
              <div style="display:flex; flex-direction:column; align-items:center; gap:8px; padding:16px; color:var(--text-muted); text-align:center;">
                <span style="font-size:12px;">Review the plugin script before activating.</span>
                <button class="btn" style="font-size:11px; padding:4px 12px;" onclick="_openPanelDiff('${panel.plugin_id}')">Review &amp; Activate</button>
              </div>`;
          }
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
    // Leer CSS variables del documento padre para inyectarlas en el iframe
    const rootStyle = getComputedStyle(document.documentElement);
    const cssVarNames = [
      '--bg', '--bg-card', '--bg-input', '--bg-hover',
      '--text-main', '--text-muted', '--text-dark',
      '--border', '--border-focus', '--border-main',
      '--accent', '--radius', '--radius-sm',
      '--font-main', '--font-mono',
    ];
    const cssVarsBlock = cssVarNames
      .map(v => `${v}: ${rootStyle.getPropertyValue(v).trim()};`)
      .filter(line => !line.endsWith(': ;'))
      .join('\n      ');

    // Detectar si el tema activo es oscuro comparando la luminosidad de --bg
    const bgColor = rootStyle.getPropertyValue('--bg').trim();
    const isDark = (() => {
      const m = bgColor.match(/(\d+),\s*(\d+),\s*(\d+)/);
      if (m) {
        const lum = 0.299 * +m[1] + 0.587 * +m[2] + 0.114 * +m[3];
        return lum < 128;
      }
      // Fallback: comparar con valor hex
      if (bgColor.startsWith('#')) {
        const hex = bgColor.slice(1);
        const r = parseInt(hex.slice(0,2), 16);
        const g = parseInt(hex.slice(2,4), 16);
        const b = parseInt(hex.slice(4,6), 16);
        return (0.299 * r + 0.587 * g + 0.114 * b) < 128;
      }
      return window.matchMedia('(prefers-color-scheme: dark)').matches;
    })();
    const colorScheme = isDark ? 'dark' : 'light';

    // Adaptar el script para tema claro/oscuro antes de embeber en el iframe
    const emptyCell = isDark ? 'rgba(255,255,255,0.06)' : 'rgba(0,0,0,0.06)';
    const adaptedScript = script.replace(/rgba\(255,255,255,0\.0[0-9]+\)/g, emptyCell);

    // ID único por iframe para filtrar mensajes postMessage entre múltiples plugins
    const frameId = 'ct-' + Math.random().toString(36).slice(2);

    // Recoger CSS vars actuales para enviarlas al iframe
    const cssVars = {};
    cssVarNames.forEach(v => {
      const val = rootStyle.getPropertyValue(v).trim();
      if (val) cssVars[v] = val;
    });

    // Crear sandbox iframe para aislar el código del plugin
    const iframe = document.createElement('iframe');
    iframe.style.cssText = 'width:100%; height:0; border:none; border-radius:4px; display:block; overflow:hidden;';
    iframe.sandbox = 'allow-scripts';
    iframe.scrolling = 'no';

    // Crear HTML estático para el sandbox — el script llega via postMessage
    const sandboxHTML = `<!DOCTYPE html>
<html style="color-scheme:${colorScheme};">
<head>
<meta charset="utf-8">
<style>
:root { ${cssVarsBlock} --is-dark: ${isDark ? 1 : 0}; }
body { margin:0; padding:12px; font-family:var(--font-main, system-ui); font-size:14px; background:transparent; color:var(--text-main, inherit); }
</style>
</head>
<body>
<div id="plugin-container"></div>
<script>
(function() {
  var FRAME_ID = '${frameId}';
  var safeAPI = {
    container: document.getElementById('plugin-container'),
    token: null,
    isDark: ${isDark ? 'true' : 'false'},
    fetch: function(url, opts) {
      if (url.startsWith('/') || url.includes(location.hostname)) return fetch(url, opts);
      throw new Error('External requests not allowed');
    }
  };
  function _notifyHeight() {
    parent.postMessage({ type: 'ct-plugin-resize', frameId: FRAME_ID, height: document.body.scrollHeight }, '*');
  }
  window.addEventListener('message', function(e) {
    if (!e.data || e.data.__ct_type !== 'run' || e.data.frameId !== FRAME_ID) return;
    safeAPI.token = e.data.token || null;
    // Aplicar CSS vars del tema padre al :root del iframe
    if (e.data.cssVars) {
      var root = document.documentElement;
      Object.keys(e.data.cssVars).forEach(function(k) {
        root.style.setProperty(k, e.data.cssVars[k]);
      });
    }
    var container = safeAPI.container;
    var token = safeAPI.token;
    var isDark = safeAPI.isDark;
    var fetch = safeAPI.fetch.bind(safeAPI);
    try {
      new Function('container', 'token', 'isDark', 'fetch', e.data.script)(container, token, isDark, fetch);
    } catch(err) {
      safeAPI.container.innerHTML = '<span style="color:red;">⚠ Plugin error: ' + err.message + '</span>';
    }
    _notifyHeight();
    if (typeof MutationObserver !== 'undefined') {
      new MutationObserver(_notifyHeight).observe(document.body, { childList: true, subtree: true, characterData: true });
    }
    setTimeout(_notifyHeight, 500);
    setTimeout(_notifyHeight, 1500);
    setTimeout(_notifyHeight, 3000);
  });
  parent.postMessage({ __ct_type: 'ready', _dash: true, frameId: FRAME_ID }, '*');
})();
<\/script>
</body>
</html>`;

    // Registrar listeners ANTES de asignar srcdoc para evitar race condition
    const _onMsg = (e) => {
      if (!e.data) return;
      // Sandbox listo: enviar script + cssVars vía postMessage con frameId
      if (e.data.__ct_type === 'ready' && e.data._dash && e.data.frameId === frameId) {
        iframe.contentWindow.postMessage({ __ct_type: 'run', frameId, script: adaptedScript, token: token || '', cssVars }, '*');
        return;
      }
      // Ajustar altura SOLO del iframe que reportó, identificado por frameId
      if (e.data.type === 'ct-plugin-resize' && e.data.frameId === frameId && iframe.isConnected) {
        iframe.style.height = Math.max(e.data.height, 0) + 'px';
      }
    };
    window.addEventListener('message', _onMsg);

    iframe.srcdoc = sandboxHTML;
    container.innerHTML = '';
    container.appendChild(iframe);
    
  } catch (e) {
    container.textContent = '⚠ Plugin error';
    console.warn('Plugin script error:', e);
  }
}

function _clipboardCopy(text) {
  if (navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(text).catch(() => _clipboardFallback(text));
  } else {
    _clipboardFallback(text);
  }
}

function _clipboardFallback(text) {
  const ta = document.createElement('textarea');
  ta.value = text;
  ta.style.cssText = 'position:fixed; top:-9999px; left:-9999px;';
  document.body.appendChild(ta);
  ta.select();
  document.execCommand('copy');
  document.body.removeChild(ta);
}

function _showKeyRevealToast(key, showToastFn) {
  const container = document.getElementById('toast-container');
  if (!container) return;
  const toast = document.createElement('div');
  toast.style.cssText = `
    background:var(--bg-card); border:1px solid var(--border-focus);
    border-radius:var(--radius); padding:12px 16px; font-size:12px;
    color:var(--text-muted); display:flex; flex-direction:column; gap:8px;
    pointer-events:all; min-width:260px; max-width:420px;
    box-shadow:0 4px 24px rgba(0,0,0,.4);
  `;
  toast.innerHTML = `
    <span style="color:var(--text-main); font-size:12px;">New API key generated. Copy it now — it won't be shown again.</span>
    <div style="display:flex; gap:6px; align-items:center;">
      <input readonly value="${key}" style="flex:1; background:var(--bg-input); border:1px solid var(--border); color:var(--text-main); padding:5px 8px; font-size:11px; font-family:var(--font-mono); border-radius:var(--radius-sm);" onclick="this.select();">
      <button class="btn" style="font-size:11px; padding:4px 10px; flex-shrink:0;">Copy</button>
    </div>
  `;
  const btn = toast.querySelector('button');
  const input = toast.querySelector('input');
  btn.addEventListener('click', () => {
    _clipboardCopy(key);
    btn.textContent = 'Copied!';
    setTimeout(() => toast.remove(), 1500);
  });
  input.addEventListener('click', () => input.select());
  container.appendChild(toast);
}

// API Key Actions
function initKeyActions() {
  const copyBtn = $('btn-copy-key');
  if (copyBtn) {
    copyBtn.addEventListener('click', async () => {
      const keyEl = $('dash-apikey');
      const fullKey = keyEl?.dataset.fullKey;
      if (fullKey) {
        _clipboardCopy(fullKey);
        copyBtn.textContent = 'Copied!';
        setTimeout(() => { copyBtn.textContent = 'Copy'; }, 2000);
      } else {
        // La clave completa no está disponible — el servidor nunca la devuelve en GET /keys
        const { showToast } = await import('./ui.js');
        showToast('Full key not available. Create a new key with "New Key" to get a copyable value.', [], 5000, 'danger');
      }
    });
  }

  const newKeyBtn = $('btn-new-key');
  if (newKeyBtn) {
    newKeyBtn.addEventListener('click', async () => {
      try {
        const result = await api('/keys', {
          method: 'POST',
          body: JSON.stringify({}),
        });
        if (result && result.key) {
          const newKey = result.key.key;
          await loadApiKey();
          // Guardar fullKey en el row recién creado para que Copy funcione
          const listEl = $('dash-apikey-list');
          if (listEl) {
            const row = listEl.querySelector(`[data-key-id="${result.key.id}"]`);
            if (row) row.dataset.fullKey = newKey;
          }
          const { showToast } = await import('./ui.js');
          _showKeyRevealToast(newKey, showToast);
        }
      } catch (e) {
        const { showToast } = await import('./ui.js');
        showToast('Failed to create key: ' + e.message, [], 4000, 'danger');
      }
    });
  }
}

export async function loadAdminPanelIfNeeded() {
  const user = getCurrentUser();
  if (!user || !user.is_admin) return;
  
  const panel = $('admin-panel');
  if (panel) panel.style.display = 'block';
  
  await Promise.allSettled([loadAdminPlugins(), loadAdminReports(), loadAdminThemes()]);
}

export function adminShowTab(tab, btn) {
  const tabs = ['plugins', 'reports', 'themes'];
  tabs.forEach(t => {
    const el = $(`admin-tab-${t}`);
    if (el) el.style.display = t === tab ? 'block' : 'none';
  });
  
  // Update button active states
  const parent = btn.parentNode;
  parent.querySelectorAll('.code-tab').forEach(b => b.classList.remove('active'));
  btn.classList.add('active');
}
window.adminShowTab = adminShowTab;

// Save profile settings
export async function saveProfileSettings() {
  const { showToast } = await import('./ui.js');
  const bioEl = $('profile-bio');
  const websiteEl = $('profile-website');

  const payload = {
    bio: bioEl?.value ?? undefined,
    website: websiteEl?.value ?? undefined,
    is_public: $('ptog-public')?.checked ?? undefined,
    profile_show_activity: $('ptog-activity')?.checked ?? undefined,
    profile_show_streak: $('ptog-streak')?.checked ?? undefined,
    profile_show_languages: $('ptog-languages')?.checked ?? undefined,
    profile_show_projects: $('ptog-projects')?.checked ?? undefined,
    profile_show_plugins: $('ptog-plugins')?.checked ?? undefined,
    available_for_hire: $('ptog-hire')?.checked ?? undefined,
    show_in_leaderboard: $('ptog-leaderboard')?.checked ?? undefined,
  };

  try {
    await api('/user/profile/update', { method: 'POST', body: JSON.stringify(payload) });
    const statusEl = $('profile-save-status');
    if (statusEl) {
      statusEl.style.display = 'inline';
      setTimeout(() => { statusEl.style.display = 'none'; }, 2000);
    }
    const { setCurrentUser, getCurrentUser } = await import('./auth.js');
    const user = getCurrentUser();
    if (user) setCurrentUser({ ...user, ...payload });
  } catch (e) {
    showToast('Failed to save profile: ' + e.message, [], 4000, 'danger');
  }
}
window.saveProfileSettings = saveProfileSettings;

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
    'ptog-leaderboard': 'show_in_leaderboard',
  };
  for (const [elId, field] of Object.entries(togMap)) {
    const el = $(elId);
    if (el) el.checked = !!user[field];
  }
}

// Placeholder for functions that need to be implemented
async function loadAdminPlugins() {
  try {
    const { plugins } = await api('/store/admin/plugins');
    const container = $('admin-plugins-list');
    if (!container || !plugins) return;
    
    container.innerHTML = plugins.map(p => `
      <div style="display:flex; justify-content:space-between; align-items:center; padding:8px; border-bottom:1px solid var(--border);">
        <div style="font-size:12px;">
          <strong>${p.display_name}</strong> by @${p.author_username || 'unknown'} · v${p.version} · ${p.install_count} installs
          ${p.is_banned ? ' <span style="color:#e53;">(Banned)</span>' : ''}
        </div>
        <div style="display:flex; gap:6px;">
          <button class="btn" style="padding:2px 8px; font-size:10px;" onclick="adminBanPlugin('${p.id}', ${!p.is_banned})">${p.is_banned ? 'Unban' : 'Ban'}</button>
          <button class="btn" style="padding:2px 8px; font-size:10px; color:#e53;" onclick="adminDeletePlugin('${p.id}')">Delete</button>
        </div>
      </div>
    `).join('');
  } catch (e) {
    console.warn('Admin plugins error:', e);
  }
}

async function loadAdminReports() {
  try {
    const { reports } = await api('/store/admin/reports');
    const container = $('admin-reports-list');
    const badge = $('admin-reports-badge');
    if (!container || !reports) return;
    
    if (badge) {
      const unresolved = reports.filter(r => !r.resolved).length;
      badge.textContent = unresolved;
      badge.style.display = unresolved > 0 ? 'inline-block' : 'none';
    }

    container.innerHTML = reports.map(r => `
      <div style="display:flex; flex-direction:column; gap:4px; padding:8px; border:1px solid var(--border); border-radius:4px; position:relative; ${r.resolved ? 'opacity:0.6;' : ''}">
        <div style="display:flex; justify-content:space-between; align-items:flex-start;">
          <strong style="font-size:12px;">${r.reason.toUpperCase()} on ${r.plugin_name}</strong>
          <span style="font-size:10px; color:var(--text-dark);">${new Date(r.created_at).toLocaleDateString()}</span>
        </div>
        <p style="font-size:11px; margin:2px 0;">${r.description || 'No description'}</p>
        <div style="font-size:10px; color:var(--text-muted);">Reported by @${r.reporter_username}</div>
        ${!r.resolved ? `<button class="btn" style="position:absolute; bottom:8px; right:8px; padding:2px 8px; font-size:10px;" onclick="adminResolveReport('${r.id}')">Resolve</button>` : ''}
      </div>
    `).join('');
  } catch (e) {
    console.warn('Admin reports error:', e);
  }
}

async function loadAdminThemes() {
  try {
    const { themes } = await api('/themes');
    const container = $('admin-themes-list');
    if (!container || !themes) return;

    container.innerHTML = themes.map(t => `
      <div style="display:flex; justify-content:space-between; align-items:center; padding:8px; border-bottom:1px solid var(--border);">
        <div style="font-size:12px;">
          <strong>${t.icon || '🎨'} ${t.display_name}</strong> by @${t.author_username || 'unknown'} · v${t.version} · ${t.install_count} installs
          ${t.is_banned ? ' <span style="color:#e53;">(Banned)</span>' : ''}
        </div>
        <div style="display:flex; gap:6px;">
          <button class="btn" style="padding:2px 8px; font-size:10px; color:#e53;" onclick="adminDeleteTheme('${t.id}')">Delete</button>
        </div>
      </div>
    `).join('');
  } catch (e) {
    console.warn('Admin themes error:', e);
  }
}

export async function adminDeleteTheme(themeId) {
  try {
    await api(`/themes/admin/${themeId}`, { method: 'DELETE' });
    await loadAdminThemes();
  } catch (e) {
    console.warn('Delete theme error:', e);
  }
}
window.adminDeleteTheme = adminDeleteTheme;

let _diffModalPluginId = null;

function openPluginDiffModal(displayName, prevVersion, newVersion, oldScript, newScript, pluginId) {
  const modal = document.getElementById('modal-plugin-diff');
  if (!modal) return;

  _diffModalPluginId = pluginId;

  const titleEl = document.getElementById('diff-modal-title');
  const versionEl = document.getElementById('diff-modal-version');
  const contentEl = document.getElementById('diff-modal-content');

  if (titleEl) titleEl.textContent = prevVersion ? `${displayName} — update available` : `${displayName} — review script`;
  if (versionEl) versionEl.textContent = prevVersion ? `${prevVersion} → ${newVersion}` : `v${newVersion}`;

  if (contentEl) {
    if (!prevVersion) {
      // Primera instalación — mostrar script completo como nuevo
      contentEl.innerHTML = newScript
        ? newScript.split('\n').map(l => `<span style="color:#4ade80;">+ ${escapeHtml(l)}</span>`).join('\n')
        : '<span style="color:var(--text-muted);">No script.</span>';
    } else {
      // Diff entre versiones
      const oldLines = (oldScript || '').split('\n');
      const newLines = (newScript || '').split('\n');
      const maxLen = Math.max(oldLines.length, newLines.length);
      let html = '';
      for (let i = 0; i < maxLen; i++) {
        const o = oldLines[i];
        const n = newLines[i];
        if (o === n) {
          html += `<span>  ${escapeHtml(o ?? '')}</span>\n`;
        } else {
          if (o !== undefined) html += `<span style="color:#f87171;">- ${escapeHtml(o)}</span>\n`;
          if (n !== undefined) html += `<span style="color:#4ade80;">+ ${escapeHtml(n)}</span>\n`;
        }
      }
      contentEl.innerHTML = html;
    }
  }

  modal.style.display = 'flex';
}

function closePluginDiffModal() {
  const modal = document.getElementById('modal-plugin-diff');
  if (modal) modal.style.display = 'none';
  _diffModalPluginId = null;
}

async function acceptPluginUpdateFromModal() {
  if (!_diffModalPluginId) return;
  try {
    await api(`/store/plugin/${_diffModalPluginId}/accept`, { method: 'POST' });
    closePluginDiffModal();
    loadPluginPanels();
  } catch (e) {
    console.warn('Accept plugin error:', e);
  }
}

function escapeHtml(str) {
  return String(str)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}

window.openPluginDiffModal = openPluginDiffModal;
window.closePluginDiffModal = closePluginDiffModal;
window.acceptPluginUpdateFromModal = acceptPluginUpdateFromModal;
window._openPanelDiff = function(pluginId) {
  const d = _panelDataMap.get(pluginId);
  if (d) openPluginDiffModal(d.title, null, d.latestVersion, '', d.latestScript, pluginId);
};

// Export functions that are used elsewhere
export { loadApiKey, initKeyActions, formatPanelValue, loadPluginPanels, runPluginScript };
