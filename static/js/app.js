/* ═══════════════════════════════════════════════════════
   CodeTrackr — app.js
   Handles: auth flow, dashboard, leaderboard, plugins,
            animations, WebSocket real-time updates
   ═══════════════════════════════════════════════════════ */

const API = '/api/v1';
let WS = null;
let currentToken = null;
let currentUser = null;

// ── Utilities ─────────────────────────────────────────────────────────────────

const $ = (id) => document.getElementById(id);
const fmt = {
  seconds: (s) => {
    if (!s || s === 0) return '0m';
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    if (h === 0) return `${m}m`;
    if (m === 0) return `${h}h`;
    return `${h}h ${m}m`;
  },
  num: (n) => n >= 1_000_000 ? (n / 1_000_000).toFixed(1) + 'M'
    : n >= 1_000 ? (n / 1_000).toFixed(1) + 'k'
      : String(n),
  date: (d) => new Date(d).toLocaleDateString('en-US', { weekday: 'short', month: 'short', day: 'numeric' }),
};

async function api(path, options = {}) {
  const headers = { 'Content-Type': 'application/json', ...(options.headers || {}) };
  if (currentToken) headers['Authorization'] = `Bearer ${currentToken}`;
  const res = await fetch(`${API}${path}`, { ...options, headers });
  if (!res.ok) throw new Error(`API error ${res.status}`);
  return res.json();
}

// ── Auth ──────────────────────────────────────────────────────────────────────

async function initAuth() {
  const params = new URLSearchParams(window.location.search);
  const oauthCode = params.get('code');

  // Lee el token del fragment (#token=) — el backend lo pasa así para evitar leakage en logs
  const hashParams = new URLSearchParams(window.location.hash.slice(1));
  const token = hashParams.get('token');

  // GitHub redirected here instead of /auth/github/callback — misconfigured callback URL
  if (oauthCode && !token) {
    window.location.href = `/auth/github/callback?${params.toString()}`;
    return;
  }

  if (token) {
    localStorage.setItem('ct_token', token);
    currentToken = token;
    window.history.replaceState({}, '', window.location.pathname);
  } else {
    currentToken = localStorage.getItem('ct_token');
  }

  // Detect /u/:username route
  const profileMatch = window.location.pathname.match(/^\/u\/([^/]+)$/);
  if (profileMatch) {
    const username = profileMatch[1];
    if (isLoggedIn()) {
      await updateUserUI();
    }
    await openPublicProfile(username);
    return;
  }

  if (isLoggedIn()) {
    await updateUserUI();
    if ($('dashboard')) {
      await loadDashboard();
    }
  }
}

async function updateUserUI() {
  const authItem = $('nav-auth-item');
  const userItem = $('nav-user-item');
  const avatar = $('nav-avatar');
  const username = $('nav-username');

  if (isLoggedIn()) {
    try {
      if (!currentUser) currentUser = await api('/user/me');
      if (authItem) authItem.style.display = 'none';
      if (userItem) userItem.style.display = 'block';
      if (avatar && currentUser.avatar_url) avatar.src = currentUser.avatar_url;
      if (username) username.textContent = currentUser.username;

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

function isLoggedIn() {
  return !!currentToken;
}

function logout() {
  localStorage.removeItem('ct_token');
  currentToken = null;
  currentUser = null;
  window.location.reload();
}

// ── View toggle ───────────────────────────────────────────────────────────────

function showLanding() {
  document.body.querySelectorAll('section, footer, .hero').forEach(el => {
    el.style.display = '';
  });
  const dash = $('dashboard');
  if (dash) dash.classList.add('hidden');
  const pp = $('public-profile');
  if (pp) pp.classList.add('hidden');
  const nav = $('nav');
  if (nav) nav.style.display = '';
}

function showDashboard() {
  // Hide landing sections
  ['hero', 'stats', 'leaderboard', 'features', 'build', 'plugins', 'pricing', 'about', 'footer', 'nav', 'public-profile'].forEach(id => {
    const el = $(id);
    if (el) el.classList.add('hidden');
  });
  const dash = $('dashboard');
  if (dash) dash.classList.remove('hidden');
}

async function uninstallPlugin(pluginId, btn) {
  if (!confirm('Are you sure you want to uninstall this plugin?')) return;

  btn.textContent = 'Removing...';
  btn.disabled = true;

  try {
    await api(`/store/uninstall/${pluginId}`, { method: 'DELETE' });
    loadPluginPanels();
    loadPluginStore();
  } catch (e) {
    btn.textContent = 'Error';
    btn.disabled = false;
  }
}

// ── Publish Modal ─────────────────────────────────────────────────────────────

function openPublishModal() {
  if (!isLoggedIn()) { showToast('Please login to publish plugins.', [], 4000, 'warning'); return; }
  const modal = $('modal-publish');
  if (modal) { modal.style.display = 'flex'; }
}

function closePublishModal() {
  const modal = $('modal-publish');
  if (modal) modal.style.display = 'none';
  const errEl = $('pub-error');
  if (errEl) errEl.style.display = 'none';
}

function onPubPluginTypeChange(type) {
  const isLifecycle = type === 'lifecycle';
  const widgetRow = $('pub-widget-type-row');
  const scriptRow = $('pub-script-row');
  const scriptHint = $('pub-script-hint');
  if (widgetRow) widgetRow.style.display = isLifecycle ? 'none' : '';
  if (scriptRow) scriptRow.style.display = isLifecycle ? 'none' : '';
  if (scriptHint) scriptHint.textContent = isLifecycle
    ? '— runs server-side on lifecycle events (on_heartbeat, on_tick, on_install…)'
    : "— runs in the user's browser. Use container (DOM element) and token (Bearer token) as variables.";
}

async function submitPublishPlugin() {
  const name = $('pub-name')?.value.trim();
  const displayName = $('pub-display-name')?.value.trim();
  const errEl = $('pub-error');

  if (!name || !displayName) {
    if (errEl) { errEl.textContent = 'Name and Display Name are required.'; errEl.style.display = 'block'; }
    return;
  }
  if (!/^[a-z0-9-]+$/.test(name)) {
    if (errEl) { errEl.textContent = 'Name must be lowercase kebab-case (letters, numbers, hyphens).'; errEl.style.display = 'block'; }
    return;
  }

  const btn = $('btn-submit-publish');
  if (btn) { btn.textContent = 'Publishing...'; btn.disabled = true; }
  if (errEl) errEl.style.display = 'none';

  const pluginType = $('pub-plugin-type')?.value || 'widget';
  const isLifecycle = pluginType === 'lifecycle';

  try {
    await api('/store/publish', {
      method: 'POST',
      body: JSON.stringify({
        name,
        display_name: displayName,
        description: $('pub-description')?.value.trim() || null,
        version: $('pub-version')?.value.trim() || '0.1.0',
        icon: $('pub-icon')?.value.trim() || '🔌',
        repository: $('pub-repo')?.value.trim() || null,
        plugin_type: pluginType,
        widget_type: isLifecycle ? null : ($('pub-widget-type')?.value || 'counter'),
        script: $('pub-script')?.value.trim() || null,
      }),
    });
    closePublishModal();
    loadPluginStore();
    const pluginTypeEl2 = $('pub-plugin-type');
    if (pluginTypeEl2) { pluginTypeEl2.value = 'widget'; onPubPluginTypeChange('widget'); }
    ['pub-name','pub-display-name','pub-description','pub-version','pub-icon','pub-repo','pub-script'].forEach(id => {
      const el = $(id); if (el) el.value = '';
    });
  } catch (e) {
    if (errEl) { errEl.textContent = 'Failed to publish: ' + e.message; errEl.style.display = 'block'; }
  } finally {
    if (btn) { btn.textContent = 'Publish'; btn.disabled = false; }
  }
}

// ── Report Modal ──────────────────────────────────────────────────────────────

function openReportModal(pluginId) {
  if (!isLoggedIn()) { showToast('Please login to report plugins.', [], 4000, 'warning'); return; }
  const idInput = $('report-plugin-id');
  if (idInput) idInput.value = pluginId;
  const modal = $('modal-report');
  if (modal) modal.style.display = 'flex';
}

function closeReportModal() {
  const modal = $('modal-report');
  if (modal) modal.style.display = 'none';
  const errEl = $('report-error');
  if (errEl) errEl.style.display = 'none';
}

async function submitReport() {
  const pluginId = $('report-plugin-id')?.value;
  const reason = $('report-reason')?.value;
  const description = $('report-description')?.value.trim() || null;
  const errEl = $('report-error');

  if (!pluginId || !reason) return;

  try {
    await api(`/store/report/${pluginId}`, {
      method: 'POST',
      body: JSON.stringify({ reason, description }),
    });
    closeReportModal();
    showToast('Report submitted. Thank you!', [], 4000, 'success');
    const descEl = $('report-description');
    if (descEl) descEl.value = '';
  } catch (e) {
    if (errEl) { errEl.textContent = 'Failed to submit report: ' + e.message; errEl.style.display = 'block'; }
  }
}

// ── Pro Features ──────────────────────────────────────────────────────────────

async function startCheckout(btn) {
  const orig = btn.textContent;
  btn.disabled = true;
  btn.textContent = 'Loading...';
  try {
    const config = await api('/billing/config');
    const { price_id } = config;
    if (!price_id) throw new Error('Billing not configured');
    const session = await api('/billing/checkout', {
      method: 'POST',
      body: JSON.stringify({ price_id }),
    });
    if (session.url) {
      window.location.href = session.url;
    } else {
      throw new Error('No checkout URL returned');
    }
  } catch (e) {
    showToast('Could not start checkout: ' + e.message, [], 4000);
    btn.disabled = false;
    btn.textContent = orig;
  }
}

function applyProFeatures(user) {
  const isPro = user && user.plan === 'pro';
  const upgradeBtn = $('btn-upgrade-pro');
  const proBadge = $('dash-pro-badge');

  if (upgradeBtn) upgradeBtn.style.display = isPro ? 'none' : 'inline-flex';
  if (proBadge) proBadge.style.display = isPro ? 'inline' : 'none';
}

// ── Dashboard ──────────────────────────────────────────────────────────────────

async function loadDashboard() {
  showDashboard();

  // Set greeting and date
  const hour = new Date().getHours();
  const greeting = hour < 12 ? 'Good morning' : hour < 18 ? 'Good afternoon' : 'Good evening';

  try {
    currentUser = await api('/user/me');
    const nameEl = $('dash-username');
    if (nameEl) nameEl.textContent = currentUser.username;
    const avatarEl = $('dash-avatar');
    if (avatarEl && currentUser.avatar_url) avatarEl.src = currentUser.avatar_url;
    const greetEl = $('dash-greeting');
    if (greetEl) greetEl.textContent = `${greeting}, ${currentUser.display_name || currentUser.username}!`;
    applyProFeatures(currentUser);
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
          if (acceptedScript && panelEl) runPluginScript(acceptedScript, panelEl, currentToken);
        } else {
          // Update available — run OLD accepted script, show toast
          if (acceptedScript && panelEl) runPluginScript(acceptedScript, panelEl, currentToken);
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

function setEl(id, val) {
  const el = $(id);
  if (el) el.textContent = val;
}

// ── API Key Actions ───────────────────────────────────────────────────────────

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
          showToast(`New API key created! Copy it now: ${result.key.key}`, [], 12000, 'success');
        }
      } catch (e) {
        showToast('Failed to create key: ' + e.message, [], 4000, 'danger');
      }
    });
  }
}

// ── WebSocket ─────────────────────────────────────────────────────────────────

let _wsRetryDelay = 5000;
let _wsRetryCount = 0;
const _wsMaxRetries = 5;

async function connectWebSocket() {
  if (!currentToken) return;
  if (_wsRetryCount >= _wsMaxRetries) return;
  const dash = $('dashboard');
  if (!dash || dash.classList.contains('hidden')) return;

  let ticket;
  try {
    const data = await api('/ws-ticket', { method: 'POST' });
    ticket = data.ticket;
  } catch (e) {
    _wsRetryCount++;
    _wsRetryDelay = Math.min(_wsRetryDelay * 2, 60000);
    setTimeout(connectWebSocket, _wsRetryDelay);
    return;
  }

  const proto = location.protocol === 'https:' ? 'wss' : 'ws';
  WS = new WebSocket(`${proto}://${location.host}/ws?ticket=${encodeURIComponent(ticket)}`);

  WS.onopen = () => { _wsRetryDelay = 5000; _wsRetryCount = 0; };
  WS.onclose = (e) => {
    if (e.code === 1000) return;
    // Closed due to tab suspension / device sleep — reconnect when page becomes visible again
    if (document.hidden) return;
    _wsRetryCount++;
    _wsRetryDelay = Math.min(_wsRetryDelay * 2, 60000);
    if (_wsRetryCount < _wsMaxRetries) setTimeout(connectWebSocket, _wsRetryDelay);
  };
  WS.onerror = () => {};

  WS.onmessage = (event) => {
    try {
      const data = JSON.parse(event.data);
      if (data.type === 'heartbeat') {
        // Pulse the today card
        const todayCard = $('dcard-today');
        if (todayCard) {
          todayCard.style.borderColor = 'var(--purple-500)';
          setTimeout(() => { todayCard.style.borderColor = ''; }, 800);
        }
        // Refresh stats debounced
        clearTimeout(connectWebSocket._refreshTimer);
        connectWebSocket._refreshTimer = setTimeout(() => {
          loadDashSummary();
          loadDashDaily();
        }, 3000);
      }
      if (data.type === 'leaderboard_update') {
        loadLeaderboard(currentLbTab);
      }
    } catch { }
  };
}

// ── Leaderboard ───────────────────────────────────────────────────────────────

let currentLbTab = 'global';
let lbInterval = null;

function initLeaderboard() {
  bindLbTabs();
  loadLeaderboard('global').then(injectLanguageTabs);

  // Auto-refresh every 30s
  if (lbInterval) clearInterval(lbInterval);
  lbInterval = setInterval(() => {
    loadLeaderboard(currentLbTab);
  }, 30_000);
}

function bindLbTabs() {
  const container = $('lb-tabs');
  if (!container) return;
  container.querySelectorAll('.code-tab').forEach(tab => {
    tab.addEventListener('click', () => {
      container.querySelectorAll('.code-tab').forEach(t => t.classList.remove('active'));
      tab.classList.add('active');
      currentLbTab = tab.dataset.tab;
      loadLeaderboard(currentLbTab);
    });
  });
}

async function injectLanguageTabs() {
  const container = $('lb-tabs');
  if (!container) return;

  try {
    const data = await api('/leaderboards/global?limit=50');
    const entries = data.leaderboard || [];

    // Count language occurrences
    const langCount = {};
    entries.forEach(e => {
      if (e.top_language) {
        langCount[e.top_language] = (langCount[e.top_language] || 0) + 1;
      }
    });

    // Top 3 languages sorted by frequency
    const top3 = Object.entries(langCount)
      .sort((a, b) => b[1] - a[1])
      .slice(0, 3)
      .map(([lang]) => lang);

    if (top3.length === 0) return;

    // Remove any previously injected language tabs
    container.querySelectorAll('.code-tab[data-dynamic]').forEach(el => el.remove());

    // Find the live indicator span to insert before it
    const liveSpan = container.querySelector('span');

    top3.forEach(lang => {
      const btn = document.createElement('button');
      btn.className = 'code-tab';
      btn.dataset.tab = lang.toLowerCase();
      btn.dataset.dynamic = '1';
      btn.textContent = lang;
      btn.addEventListener('click', () => {
        container.querySelectorAll('.code-tab').forEach(t => t.classList.remove('active'));
        btn.classList.add('active');
        currentLbTab = lang.toLowerCase();
        loadLeaderboard(currentLbTab);
      });
      container.insertBefore(btn, liveSpan);
    });
  } catch (e) {
    // No data yet, no tabs injected
  }
}

// ── Plugin Code Tabs ──────────────────────────────────────────────────────────

function initPluginTabs() {
  const tabs = document.querySelectorAll('.code-tab');
  tabs.forEach(tab => {
    tab.addEventListener('click', () => {
      // Avoid conflict with leaderboard or store tabs if they use the same class but are handled elsewhere
      if (tab.closest('#leaderboard') || tab.closest('#plugin-store')) return;

      tabs.forEach(t => t.classList.remove('active'));
      tab.classList.add('active');
      document.querySelectorAll('.code-content').forEach(c => c.classList.add('hidden'));
      const target = $(`code-${tab.dataset.code}`);
      if (target) target.classList.remove('hidden');
    });
  });
}

// ── Plugin Store ──────────────────────────────────────────────────────────────

async function uninstallPluginFromStore(pluginId, btn) {
  btn.textContent = 'Removing...';
  btn.disabled = true;
  try {
    await api(`/store/uninstall/${pluginId}`, { method: 'DELETE' });
    loadPluginPanels();
    loadPluginStore();
  } catch (e) {
    btn.textContent = 'Uninstall';
    btn.disabled = false;
  }
}

async function loadPluginStore(filter = 'all') {
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
      const reportBtn = isLoggedIn() && !isInstalled && filter !== 'installed' && p.author_username !== currentUser?.username
        ? `<button class="btn" style="font-size:11px; height:32px; padding:0 10px; color:var(--text-dark);" onclick="openReportModal('${pid}')">⚑</button>`
        : '';
      const deleteBtn = isLoggedIn() && p.author_username === currentUser?.username
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

async function installPlugin(pluginId, btn) {
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
    setTimeout(() => { loadPluginStore(); loadPluginPanels(); }, 1500);
  } catch (e) {
    console.error('Install failed:', e);
    btn.textContent = 'Error';
    btn.disabled = false;
    setTimeout(() => { btn.textContent = originalText; }, 2000);
  }
}

// ── Scroll Animations ─────────────────────────────────────────────────────────

function initScrollAnimations() {
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

// ── Counter Animation ─────────────────────────────────────────────────────────

function animateCounters() {
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

// ── Navigation ────────────────────────────────────────────────────────────────

function initNav() {
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

// ── Init ──────────────────────────────────────────────────────────────────────

document.addEventListener('DOMContentLoaded', async () => {
  await initAuth();
  initNav();
  if (window.location.pathname === '/' || window.location.pathname === '') {
    initLandingPage();
  }
});

window.addEventListener('popstate', async () => {
  const profileMatch = window.location.pathname.match(/^\/u\/([^/]+)$/);
  if (profileMatch) {
    await openPublicProfile(profileMatch[1]);
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
});

function copyApiKey() {
  const keyEl = $('profile-apikey');
  if (keyEl) {
    const text = keyEl.textContent;
    if (text.includes('••••')) {
      showToast('Key is hidden. Use "New" to create a visible one if needed.', [], 4000, 'info');
    } else {
      navigator.clipboard.writeText(text.replace('...', ''));
      showToast('API Key prefix copied!', [], 3000, 'success');
    }
  }
}

async function regenerateApiKey() {
  if (!confirm('This will create a new API Key for your account. You can have up to 5 keys.')) return;
  try {
    const data = await api('/keys', {
      method: 'POST',
      body: JSON.stringify({ name: `Key ${new Date().toLocaleDateString()}` })
    });
    const key = data.key.key;
    showToast(`NEW API KEY: ${key} — Copy this now, it won't be shown again.`, [], 15000, 'success');
    loadUserKeys();
  } catch (e) {
    showToast('Failed to generate key: ' + e.message, [], 4000, 'danger');
  }
}

async function loadPublicStats() {
  try {
    const data = await api('/stats/public');
    const devsEl = document.getElementById('stat-devs');
    if (devsEl) devsEl.textContent = `${fmt.num(data.users)} active developers`;
    const hoursEl = document.getElementById('stat-hours');
    if (hoursEl) {
      const hours = Math.floor(data.total_seconds / 3600);
      hoursEl.textContent = `Over ${fmt.num(hours)} hours logged`;
    }
  } catch (e) {
    console.warn('Could not load public stats', e);
  }
}

// ── Admin Panel ───────────────────────────────────────────────────────────────

async function loadAdminPanelIfNeeded() {
  if (!currentUser) return;
  if (!currentUser.is_admin) return;
  const panel = $('admin-panel');
  if (panel) panel.style.display = 'block';
  await Promise.allSettled([loadAdminPlugins(), loadAdminReports()]);
}

function adminShowTab(tab, btn) {
  const pluginsTab = $('admin-tab-plugins');
  const reportsTab = $('admin-tab-reports');
  if (pluginsTab) pluginsTab.style.display = tab === 'plugins' ? 'block' : 'none';
  if (reportsTab) reportsTab.style.display = tab === 'reports' ? 'block' : 'none';
  const panel = $('admin-panel');
  if (panel) panel.querySelectorAll('.code-tab').forEach(b => b.classList.remove('active'));
  if (btn) btn.classList.add('active');
}

async function loadAdminPlugins() {
  try {
    const { plugins } = await api('/store/admin/plugins');
    const container = $('admin-plugins-list');
    if (!container) return;
    if (!plugins || plugins.length === 0) {
      container.innerHTML = '<div style="color:var(--text-muted); font-size:13px;">No plugins yet.</div>';
      return;
    }
    container.innerHTML = plugins.map(p => `
      <div style="display:flex; justify-content:space-between; align-items:center; padding:10px 12px; background:var(--bg-card); border:1px solid var(--border); border-radius:4px;">
        <div>
          <span style="font-size:13px; color:var(--text-main);">${p.icon || '🔌'} ${p.display_name}</span>
          <span style="font-size:11px; color:var(--text-muted); margin-left:8px;">by author · v${p.version} · ${p.install_count} installs</span>
          ${p.is_banned ? '<span style="margin-left:8px; font-size:10px; color:#e53; background:rgba(238,85,51,.12); padding:1px 6px; border-radius:3px;">BANNED</span>' : ''}
        </div>
        <div style="display:flex; gap:6px;">
          ${p.is_banned
            ? `<button class="btn" style="font-size:11px; padding:2px 8px;" onclick="adminUnbanPlugin('${p.id}', this)">Unban</button>`
            : `<button class="btn" style="font-size:11px; padding:2px 8px; color:#e53;" onclick="adminBanPlugin('${p.id}', this)">Ban</button>`
          }
          <button class="btn" style="font-size:11px; padding:2px 8px; color:#e53;" onclick="adminDeletePlugin('${p.id}', this)">Delete</button>
        </div>
      </div>
    `).join('');
  } catch (e) {
    console.warn('Admin plugins error:', e);
  }
}

async function loadAdminReports() {
  try {
    const { reports, count } = await api('/store/admin/reports');
    const badge = $('admin-reports-badge');
    const unresolved = (reports || []).filter(r => !r.resolved).length;
    if (badge) {
      if (unresolved > 0) { badge.textContent = unresolved; badge.style.display = 'inline'; }
      else badge.style.display = 'none';
    }
    const container = $('admin-reports-list');
    if (!container) return;
    if (!reports || reports.length === 0) {
      container.innerHTML = '<div style="color:var(--text-muted); font-size:13px;">No reports.</div>';
      return;
    }
    container.innerHTML = reports.map(r => `
      <div style="display:flex; justify-content:space-between; align-items:center; padding:10px 12px; background:var(--bg-card); border:1px solid ${r.resolved ? 'var(--border)' : 'rgba(238,85,51,.3)'}; border-radius:4px; opacity:${r.resolved ? '0.5' : '1'};">
        <div>
          <span style="font-size:13px; color:var(--text-main);">${r.plugin_name}</span>
          <span style="font-size:11px; color:#e53; margin-left:8px;">${r.reason}</span>
          <span style="font-size:11px; color:var(--text-muted); margin-left:8px;">by @${r.reporter_username}</span>
          ${r.description ? `<div style="font-size:12px; color:var(--text-muted); margin-top:2px;">${r.description}</div>` : ''}
        </div>
        <div style="display:flex; gap:6px; flex-shrink:0; margin-left:12px;">
          ${!r.resolved ? `<button class="btn" style="font-size:11px; padding:2px 8px;" onclick="adminResolveReport('${r.id}', this)">Resolve</button>` : '<span style="font-size:11px; color:var(--text-muted);">Resolved</span>'}
          <button class="btn" style="font-size:11px; padding:2px 8px; color:#e53;" onclick="adminBanPlugin('${r.plugin_id}', this)">Ban plugin</button>
        </div>
      </div>
    `).join('');
  } catch (e) {
    console.warn('Admin reports error:', e);
  }
}

async function adminBanPlugin(pluginId, btn) {
  const reason = prompt('Ban reason (optional):') || '';
  if (reason === null) return;
  const orig = btn.textContent;
  btn.disabled = true; btn.textContent = '...';
  try {
    await api(`/store/admin/ban/${pluginId}`, { method: 'POST', body: JSON.stringify({ reason: reason || null }) });
    await loadAdminPlugins();
    loadPluginStore();
  } catch (e) {
    showToast('Error: ' + e.message, [], 4000, 'danger');
  } finally {
    btn.disabled = false; btn.textContent = orig;
  }
}

async function adminUnbanPlugin(pluginId, btn) {
  const orig = btn.textContent;
  btn.disabled = true; btn.textContent = '...';
  try {
    await api(`/store/admin/unban/${pluginId}`, { method: 'POST' });
    await loadAdminPlugins();
    loadPluginStore();
  } catch (e) {
    showToast('Error: ' + e.message, [], 4000, 'danger');
  } finally {
    btn.disabled = false; btn.textContent = orig;
  }
}

async function adminDeletePlugin(pluginId, btn) {
  if (!confirm('Permanently delete this plugin? This cannot be undone.')) return;
  const orig = btn.textContent;
  btn.disabled = true; btn.textContent = '...';
  try {
    await api(`/store/admin/delete/${pluginId}`, { method: 'DELETE' });
    await loadAdminPlugins();
    loadPluginStore();
  } catch (e) {
    showToast('Error: ' + e.message, [], 4000, 'danger');
  } finally {
    btn.disabled = false; btn.textContent = orig;
  }
}

async function adminResolveReport(reportId, btn) {
  const orig = btn.textContent;
  btn.disabled = true; btn.textContent = '...';
  try {
    await api(`/store/admin/reports/${reportId}/resolve`, { method: 'POST' });
    await loadAdminReports();
  } catch (e) {
    showToast('Error: ' + e.message, [], 4000, 'danger');
  } finally {
    btn.disabled = false; btn.textContent = orig;
  }
}

// ── Plugin Update Toast & Diff ────────────────────────────────────────────────

function showToast(message, actions = [], durationMs = 8000, type = '') {
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

function computeDiff(oldScript, newScript) {
  const oldLines = (oldScript || '').split('\n');
  const newLines = (newScript || '').split('\n');
  const oldSet = new Set(oldLines);
  const newSet = new Set(newLines);
  const lines = [];

  oldLines.forEach(line => {
    if (!newSet.has(line)) lines.push({ type: 'removed', text: line });
  });
  newLines.forEach(line => {
    if (!oldSet.has(line)) lines.push({ type: 'added', text: line });
  });

  return lines;
}

function openPluginDiffModal(displayName, prevVersion, newVersion, oldScript, newScript, pluginId) {
  const modal = $('modal-plugin-diff');
  if (!modal) return;
  modal.dataset.pluginId = pluginId || '';

  const title = $('diff-modal-title');
  const version = $('diff-modal-version');
  const content = $('diff-modal-content');

  if (title) title.textContent = `${displayName} — updated`;
  if (version) version.textContent = `${prevVersion} → ${newVersion}`;

  const diff = computeDiff(oldScript, newScript);

  if (content) {
    if (diff.length === 0) {
      content.innerHTML = '<span style="color:var(--text-dark);">No script changes detected.</span>';
    } else {
      content.innerHTML = diff.map(({ type, text }) => {
        const escaped = text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
        if (type === 'added') {
          return `<div style="background:#1a2a1f; color:#4ade80;">+ ${escaped}</div>`;
        }
        return `<div style="background:#2a1f1f; color:#f87171;">− ${escaped}</div>`;
      }).join('');
    }
  }

  modal.style.display = 'flex';
}

// ── Plugin Detail Modal ───────────────────────────────────────────────────────

let _detailPluginId = null;
let _detailRating = 0;
let _detailInstalledIds = new Set();

function starsHtml(avg, total) {
  const filled = Math.round(avg);
  let s = '';
  for (let i = 1; i <= 5; i++) s += i <= filled ? '★' : '☆';
  return s;
}

function setDetailRating(n) {
  _detailRating = n;
  const btns = $('detail-star-input')?.querySelectorAll('button');
  if (!btns) return;
  btns.forEach((b, i) => { b.textContent = i < n ? '★' : '☆'; });
}

async function openPluginDetailModal(pluginId, installedIds) {
  _detailPluginId = pluginId;
  _detailRating = 0;
  _detailInstalledIds = installedIds || new Set();

  const modal = $('modal-plugin-detail');
  if (!modal) return;
  modal.style.display = 'flex';

  // Reset star input
  const starBtns = $('detail-star-input')?.querySelectorAll('button');
  if (starBtns) starBtns.forEach(b => { b.textContent = '☆'; });
  const reviewBody = $('detail-review-body');
  if (reviewBody) reviewBody.value = '';

  try {
    const data = await api(`/store/plugin/${pluginId}/detail`);
    const { plugin, reviews, screenshots } = data;
    const authorUsername = data.author_username;

    // Header
    const titleEl = $('detail-title');
    if (titleEl) titleEl.textContent = `${plugin.icon || '🔌'} ${plugin.display_name}`;
    const versionEl = $('detail-version');
    if (versionEl) versionEl.textContent = `v${plugin.version}`;
    const nameEl = $('detail-name');
    if (nameEl) {
      nameEl.innerHTML = `<span style="font-family:var(--font-mono);">${plugin.name}</span>${authorUsername
        ? ` <span style="color:var(--text-muted);">by</span> <a href="javascript:void(0)" onclick="closePluginDetailModal(); openPublicProfile('${authorUsername}')" style="color:var(--text-dark); text-decoration:underline;">@${authorUsername}</a>`
        : ''}`;
    }
    const starsEl = $('detail-stars');
    if (starsEl) starsEl.textContent = starsHtml(plugin.avg_rating || 0, plugin.rating_count || 0);
    const ratingCountEl = $('detail-rating-count');
    if (ratingCountEl) ratingCountEl.textContent = plugin.rating_count > 0
      ? `${Number(plugin.avg_rating).toFixed(1)} (${plugin.rating_count})`
      : 'No ratings yet';

    // Description
    const descEl = $('detail-description');
    if (descEl) descEl.textContent = plugin.description || 'No description provided.';

    // Meta
    const installsEl = $('detail-installs');
    if (installsEl) installsEl.textContent = plugin.install_count || 0;
    const repoEl = $('detail-repo');
    if (repoEl) {
      if (plugin.repository) {
        repoEl.href = plugin.repository;
        repoEl.style.display = 'inline';
      } else {
        repoEl.style.display = 'none';
      }
    }

    // Action buttons
    const actionsEl = $('detail-actions');
    if (actionsEl) {
      const isInstalled = _detailInstalledIds.has(plugin.id);
      const installBtn = isInstalled
        ? `<button class="btn" style="font-size:12px; color:var(--text-dark);" onclick="uninstallPluginFromStore('${plugin.id}', this); closePluginDetailModal();">Uninstall</button>`
        : `<button class="btn" style="font-size:12px;" onclick="installPlugin('${plugin.id}', this)">Install</button>`;
      const codeBtn = plugin.script
        ? `<button class="btn" style="font-size:12px; color:var(--text-dark);" onclick="openPluginCodeModal('${plugin.display_name.replace(/'/g,"&#39;")}','${plugin.name}','${plugin.version}',decodeURIComponent('${encodeURIComponent(plugin.script || '')}'))">{ } View code</button>`
        : '';
      actionsEl.innerHTML = installBtn + codeBtn;
    }

    // Screenshots
    const ssSection = $('detail-screenshots-section');
    const ssContainer = $('detail-screenshots');
    const ssForm = $('detail-screenshot-form');
    if (ssSection && ssContainer) {
      ssSection.style.display = 'block';
      if (screenshots.length > 0) {
        ssContainer.innerHTML = screenshots.map(s => `
          <div style="flex-shrink:0;">
            <a href="${s.url}" target="_blank">
              <img src="${s.url}" alt="${s.caption || ''}" style="height:120px; border-radius:var(--radius-sm); border:1px solid var(--border); object-fit:cover; max-width:200px;">
            </a>
            ${s.caption ? `<div style="font-size:10px; color:var(--text-dark); margin-top:4px; max-width:200px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;">${s.caption}</div>` : ''}
          </div>
        `).join('');
      } else {
        ssContainer.innerHTML = `<span style="font-size:12px; color:var(--text-dark);">No screenshots yet.</span>`;
      }
      if (ssForm) ssForm.style.display = isLoggedIn() ? 'flex' : 'none';
    }

    // Rate & Review form (only logged in)
    const reviewSection = $('detail-review-section');
    if (reviewSection) reviewSection.style.display = isLoggedIn() ? 'block' : 'none';

    // Reviews list
    const reviewsList = $('detail-reviews-list');
    if (reviewsList) {
      if (reviews.length === 0) {
        reviewsList.innerHTML = `<div style="font-size:12px; color:var(--text-dark);">No reviews yet. Be the first!</div>`;
      } else {
        reviewsList.innerHTML = reviews.map(r => {
          const stars = starsHtml(r.rating, 1);
          const date = new Date(r.created_at).toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' });
          return `
            <div style="border-top:1px solid var(--border); padding:10px 0;">
              <div style="display:flex; justify-content:space-between; align-items:center; margin-bottom:4px;">
                <span style="font-size:12px; color:var(--text-main);">@${r.username}</span>
                <div style="display:flex; align-items:center; gap:8px;">
                  ${r.rating > 0 ? `<span style="font-size:12px; color:var(--accent);">${stars}</span>` : ''}
                  <span style="font-size:10px; color:var(--text-dark);">${date}</span>
                </div>
              </div>
              <p style="margin:0; font-size:12px; color:var(--text-muted); line-height:1.5;">${r.body}</p>
            </div>
          `;
        }).join('');
      }
    }
  } catch (e) {
    console.warn('Plugin detail error:', e);
  }
}

function closePluginDetailModal() {
  const modal = $('modal-plugin-detail');
  if (modal) modal.style.display = 'none';
}

async function submitDetailReview() {
  if (!_detailPluginId) return;
  const body = $('detail-review-body')?.value.trim();
  if (!body) return;
  try {
    if (_detailRating > 0) {
      await api(`/store/plugin/${_detailPluginId}/rate`, { method: 'POST', body: JSON.stringify({ rating: _detailRating }) });
    }
    await api(`/store/plugin/${_detailPluginId}/review`, { method: 'POST', body: JSON.stringify({ body }) });
    await openPluginDetailModal(_detailPluginId, _detailInstalledIds);
  } catch (e) {
    showToast('Failed to submit review: ' + e.message, [], 4000);
  }
}

async function submitScreenshot() {
  if (!_detailPluginId) return;
  const url = $('detail-screenshot-url')?.value.trim();
  const caption = $('detail-screenshot-caption')?.value.trim() || null;
  if (!url) return;
  try {
    await api(`/store/plugin/${_detailPluginId}/screenshot`, { method: 'POST', body: JSON.stringify({ url, caption }) });
    const urlEl = $('detail-screenshot-url');
    const capEl = $('detail-screenshot-caption');
    if (urlEl) urlEl.value = '';
    if (capEl) capEl.value = '';
    await openPluginDetailModal(_detailPluginId, _detailInstalledIds);
  } catch (e) {
    showToast('Failed to add screenshot: ' + e.message, [], 4000);
  }
}

function openPluginCodeModal(displayName, pluginName, version, script) {
  const modal = $('modal-plugin-code');
  if (!modal) return;
  const title = $('code-modal-title');
  const meta = $('code-modal-meta');
  const content = $('code-modal-content');
  if (title) title.textContent = `${displayName} — source`;
  if (meta) meta.textContent = `${pluginName}  v${version}`;
  if (content) {
    if (script) {
      content.textContent = script;
    } else {
      content.textContent = '// No script available for this plugin.';
    }
  }
  modal.style.display = 'flex';
}

function closePluginCodeModal() {
  const modal = $('modal-plugin-code');
  if (modal) modal.style.display = 'none';
}

function closePluginDiffModal() {
  const modal = $('modal-plugin-diff');
  if (modal) modal.style.display = 'none';
}

async function acceptPluginUpdateFromModal() {
  const modal = $('modal-plugin-diff');
  if (!modal) return;
  const pluginId = modal.dataset.pluginId;
  if (!pluginId) return;
  await api(`/store/plugin/${pluginId}/accept`, { method: 'POST' });
  closePluginDiffModal();
  loadPluginPanels();
}

// ── Global exports ───────────────────────────────────────────────────────────

window.logout = logout;
window.uninstallPlugin = uninstallPlugin;
window.uninstallPluginFromStore = uninstallPluginFromStore;
window.loadPluginStore = loadPluginStore;
window.installPlugin = installPlugin;
window.copyApiKey = copyApiKey;
window.regenerateApiKey = regenerateApiKey;
window.openPublishModal = openPublishModal;
window.closePublishModal = closePublishModal;
window.submitPublishPlugin = submitPublishPlugin;
window.onPubPluginTypeChange = onPubPluginTypeChange;
window.openReportModal = openReportModal;
window.closeReportModal = closeReportModal;
window.submitReport = submitReport;
window.adminShowTab = adminShowTab;
window.adminBanPlugin = adminBanPlugin;
window.adminUnbanPlugin = adminUnbanPlugin;
window.adminDeletePlugin = adminDeletePlugin;

async function authorDeletePlugin(pluginId, btn) {
  if (!confirm('Delete your plugin? This cannot be undone.')) return;
  const orig = btn.textContent;
  btn.disabled = true; btn.textContent = '...';
  try {
    await api(`/store/my/${pluginId}`, { method: 'DELETE' });
    loadPluginStore();
  } catch (e) {
    showToast('Could not delete plugin: ' + e.message, [], 3000);
    btn.disabled = false; btn.textContent = orig;
  }
}
window.authorDeletePlugin = authorDeletePlugin;
window.adminResolveReport = adminResolveReport;
// ── Profile Settings (dashboard) ─────────────────────────────────────────────

async function loadProfileSettings() {
  if (!currentUser) return;
  const bioEl = $('profile-bio');
  const websiteEl = $('profile-website');
  const linkEl = $('dash-profile-link');

  if (bioEl) bioEl.value = currentUser.bio || '';
  if (websiteEl) websiteEl.value = currentUser.website || '';
  if (linkEl) {
    linkEl.href = `javascript:void(0)`;
    linkEl.onclick = () => openPublicProfile(currentUser.username);
    linkEl.textContent = `↗ @${currentUser.username}`;
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
    if (el) el.checked = !!currentUser[field];
  }
}

async function saveProfileSettings() {
  const statusEl = $('profile-save-status');
  try {
    const body = {
      bio: $('profile-bio')?.value.trim() || null,
      website: $('profile-website')?.value.trim() || null,
      is_public: $('ptog-public')?.checked,
      profile_show_activity: $('ptog-activity')?.checked,
      profile_show_streak: $('ptog-streak')?.checked,
      profile_show_languages: $('ptog-languages')?.checked,
      profile_show_projects: $('ptog-projects')?.checked,
      profile_show_plugins: $('ptog-plugins')?.checked,
      available_for_hire: $('ptog-hire')?.checked,
    };
    await api('/user/profile/update', { method: 'POST', body: JSON.stringify(body) });
    currentUser = { ...currentUser, ...body };
    if (statusEl) {
      statusEl.style.display = 'inline';
      setTimeout(() => { statusEl.style.display = 'none'; }, 2500);
    }
  } catch (e) {
    showToast('Failed to save profile: ' + e.message, [], 4000);
  }
}

// ── Public Profile View ───────────────────────────────────────────────────────

let _ppPrevSection = null;

async function openPublicProfile(username) {
  const ppSection = $('public-profile');
  if (!ppSection) return;

  // Remember which section was visible to restore on back
  const dash = $('dashboard');
  if (dash && !dash.classList.contains('hidden')) {
    _ppPrevSection = 'dashboard';
  } else {
    _ppPrevSection = 'landing';
  }

  // Update URL to /u/:username without full reload
  const currentPath = window.location.pathname;
  if (!currentPath.startsWith('/u/')) {
    window.history.pushState({ profile: username }, '', `/u/${username}`);
  }

  // Hide everything except pp section
  ['hero', 'stats', 'leaderboard', 'features', 'build', 'plugins', 'pricing', 'about', 'footer', 'dashboard'].forEach(id => {
    const el = $(id);
    if (el) el.classList.add('hidden');
  });
  ppSection.classList.remove('hidden');

  // Reset
  $('pp-avatar') && ($('pp-avatar').src = '');
  $('pp-display-name') && ($('pp-display-name').textContent = '');
  $('pp-username') && ($('pp-username').textContent = '');
  $('pp-bio') && ($('pp-bio').textContent = '');
  $('pp-followers') && ($('pp-followers').textContent = '…');
  $('pp-following') && ($('pp-following').textContent = '…');
  $('pp-weekly') && ($('pp-weekly').textContent = '—');
  $('pp-streak') && ($('pp-streak').textContent = '—');
  $('pp-actions') && ($('pp-actions').innerHTML = '');

  try {
    const p = await api(`/user/profile/${username}`);

    const avatarEl = $('pp-avatar');
    if (avatarEl) avatarEl.src = p.avatar_url || '';

    const nameEl = $('pp-display-name');
    if (nameEl) nameEl.textContent = p.display_name || p.username;

    const unameEl = $('pp-username');
    if (unameEl) unameEl.textContent = `@${p.username}`;

    const planEl = $('pp-plan');
    if (planEl) {
      if (p.plan === 'pro') { planEl.textContent = '★ Pro'; planEl.style.display = 'inline'; }
      else planEl.style.display = 'none';
    }

    const bioEl = $('pp-bio');
    if (bioEl) bioEl.textContent = p.bio || '';

    const followersEl = $('pp-followers');
    if (followersEl) followersEl.textContent = p.follower_count ?? 0;

    const followingEl = $('pp-following');
    if (followingEl) followingEl.textContent = p.following_count ?? 0;

    const countryEl = $('pp-country');
    if (countryEl) countryEl.textContent = p.country || '';

    const websiteEl = $('pp-website');
    if (websiteEl) {
      if (p.website) { websiteEl.href = p.website; websiteEl.style.display = 'inline'; }
      else websiteEl.style.display = 'none';
    }

    const sinceEl = $('pp-since');
    if (sinceEl) sinceEl.textContent = new Date(p.member_since).toLocaleDateString('en-US', { month: 'short', year: 'numeric' });

    // Follow button
    const actionsEl = $('pp-actions');
    if (actionsEl && isLoggedIn() && currentUser?.username !== username) {
      let isFollowing = false;
      try {
        const res = await api(`/user/following/${username}`);
        isFollowing = res.following;
      } catch (_) {}
      actionsEl.innerHTML = isFollowing
        ? `<button class="btn" style="font-size:12px; color:var(--text-dark);" onclick="toggleFollow('${username}', false, this)">✓ Following</button>`
        : `<button class="btn" style="font-size:12px;" onclick="toggleFollow('${username}', true, this)">+ Follow</button>`;
    }

    // Stats
    const streakCard = $('pp-streak-card');
    const actCard = $('pp-activity-card');
    if (streakCard) streakCard.style.display = p.show_streak ? '' : 'none';
    if (actCard) actCard.style.display = p.show_activity ? '' : 'none';
    $('pp-streak') && ($('pp-streak').textContent = p.streak_days ?? 0);
    $('pp-weekly') && ($('pp-weekly').textContent = fmt.seconds(p.weekly_seconds));

    // Languages
    const langCard = $('pp-languages-card');
    const langBars = $('pp-lang-bars');
    if (langCard && langBars) {
      if (p.show_languages && p.languages.length > 0) {
        langCard.style.display = '';
        const maxSec = Math.max(...p.languages.map(l => l.seconds), 1);
        langBars.innerHTML = p.languages.map(l => {
          const pct = Math.round((l.seconds / maxSec) * 100);
          return `
            <div style="margin-bottom:8px;">
              <div style="display:flex; justify-content:space-between; font-size:11px; color:var(--text-dark); margin-bottom:3px;">
                <span>${l.language}</span><span>${fmt.seconds(l.seconds)}</span>
              </div>
              <div style="background:var(--border); border-radius:2px; height:4px;">
                <div style="background:var(--accent); width:${pct}%; height:100%; border-radius:2px;"></div>
              </div>
            </div>`;
        }).join('');
      } else {
        langCard.style.display = 'none';
      }
    }

    // Projects
    const projCard = $('pp-projects-card');
    const projList = $('pp-projects-list');
    if (projCard && projList) {
      if (p.show_projects && p.projects.length > 0) {
        projCard.style.display = '';
        const maxSec = Math.max(...p.projects.map(pr => pr.seconds), 1);
        projList.innerHTML = p.projects.map(pr => {
          const pct = Math.round((pr.seconds / maxSec) * 100);
          return `
            <div style="margin-bottom:8px;">
              <div style="display:flex; justify-content:space-between; font-size:11px; color:var(--text-dark); margin-bottom:3px;">
                <span style="font-family:var(--font-mono);">${pr.project}</span><span>${fmt.seconds(pr.seconds)}</span>
              </div>
              <div style="background:var(--border); border-radius:2px; height:4px;">
                <div style="background:var(--text-dark); width:${pct}%; height:100%; border-radius:2px;"></div>
              </div>
            </div>`;
        }).join('');
      } else {
        projCard.style.display = 'none';
      }
    }

    // Plugins
    const plugCard = $('pp-plugins-card');
    const plugGrid = $('pp-plugins-grid');
    if (plugCard && plugGrid) {
      if (p.show_plugins && p.plugins.length > 0) {
        plugCard.style.display = '';
        plugGrid.innerHTML = p.plugins.map(pl => `
          <div class="card" style="cursor:pointer;" onclick="openPluginDetailModal('${pl.id}', new Set())">
            <div style="display:flex; justify-content:space-between; align-items:flex-start; margin-bottom:4px;">
              <h3 style="margin:0; font-size:13px;">${pl.icon || '🔌'} ${pl.display_name}</h3>
              <span class="key-hint" style="font-size:10px;">v${pl.version}</span>
            </div>
            <p style="font-size:11px; color:var(--text-muted); margin:4px 0 8px; line-height:1.4;">${pl.description || ''}</p>
            <div style="font-size:11px; color:var(--text-dark); display:flex; gap:10px;">
              <span>↓ ${pl.install_count}</span>
              ${pl.rating_count > 0 ? `<span style="color:var(--accent);">★ ${Number(pl.avg_rating).toFixed(1)}</span>` : ''}
            </div>
          </div>
        `).join('');
      } else {
        plugCard.style.display = 'none';
      }
    }

    // Available for hire
    const hireSection = $('pp-hire-section');
    if (hireSection) hireSection.style.display = p.available_for_hire ? '' : 'none';

  } catch (e) {
    console.warn('Public profile error:', e);
    const ppSection2 = $('public-profile');
    if (ppSection2) ppSection2.innerHTML = `<div class="container"><p style="color:var(--text-muted); padding:40px;">Profile not found or not public.</p><button class="btn" onclick="closePublicProfile()">← Back</button></div>`;
  }
}

function closePublicProfile() {
  const ppSection = $('public-profile');
  if (ppSection) ppSection.classList.add('hidden');

  if (_ppPrevSection === 'dashboard') {
    window.history.pushState({}, '', '/');
    const dash = $('dashboard');
    if (dash) dash.classList.remove('hidden');
  } else {
    window.history.pushState({}, '', '/');
    showLanding();
  }
}

async function toggleFollow(username, doFollow, btn) {
  if (!btn) return;
  try {
    if (doFollow) {
      await api(`/user/follow/${username}`, { method: 'POST' });
      btn.textContent = '✓ Following';
      btn.style.color = 'var(--text-dark)';
      btn.onclick = () => toggleFollow(username, false, btn);
    } else {
      await api(`/user/unfollow/${username}`, { method: 'DELETE' });
      btn.textContent = '+ Follow';
      btn.style.color = '';
      btn.onclick = () => toggleFollow(username, true, btn);
    }
    // Update counter
    const followersEl = $('pp-followers');
    if (followersEl) followersEl.textContent = parseInt(followersEl.textContent || '0') + (doFollow ? 1 : -1);
  } catch (e) {
    showToast('Action failed: ' + e.message, [], 3000);
  }
}

// ── Available for Hire — Contact Modal ────────────────────────────────────────

let _contactTargetUsername = null;

function openContactModal() {
  const ppUnameEl = $('pp-username');
  if (ppUnameEl) {
    _contactTargetUsername = ppUnameEl.textContent.replace('@', '').trim();
  }
  const modal = $('hire-contact-modal');
  if (!modal) return;
  const nameEl = $('hire-contact-name');
  const emailEl = $('hire-contact-email');
  const msgEl = $('hire-contact-message');
  const statusEl = $('hire-contact-status');
  const submitBtn = $('hire-contact-submit');
  if (nameEl) nameEl.value = '';
  if (emailEl) emailEl.value = '';
  if (msgEl) msgEl.value = '';
  if (statusEl) { statusEl.style.display = 'none'; statusEl.textContent = ''; }
  if (submitBtn) { submitBtn.disabled = false; submitBtn.textContent = 'Send'; }
  modal.style.display = 'flex';
}

function closeContactModal() {
  const modal = $('hire-contact-modal');
  if (modal) modal.style.display = 'none';
}

async function submitContactDev() {
  const submitBtn = $('hire-contact-submit');
  const statusEl = $('hire-contact-status');
  const name = $('hire-contact-name')?.value.trim();
  const email = $('hire-contact-email')?.value.trim();
  const message = $('hire-contact-message')?.value.trim();

  if (!name || !email || !message) {
    if (statusEl) { statusEl.style.display = 'block'; statusEl.style.color = 'var(--text-muted)'; statusEl.textContent = 'Please fill in all fields.'; }
    return;
  }

  if (!_contactTargetUsername) return;

  if (submitBtn) { submitBtn.disabled = true; submitBtn.textContent = 'Sending…'; }

  try {
    await api(`/user/contact/${_contactTargetUsername}`, {
      method: 'POST',
      body: JSON.stringify({ name, email, message }),
    });
    if (statusEl) { statusEl.style.display = 'block'; statusEl.style.color = 'var(--accent)'; statusEl.textContent = 'Message sent successfully!'; }
    if (submitBtn) submitBtn.textContent = 'Sent';
    setTimeout(() => closeContactModal(), 2000);
  } catch (e) {
    if (statusEl) { statusEl.style.display = 'block'; statusEl.style.color = 'var(--text-muted)'; statusEl.textContent = 'Failed to send: ' + e.message; }
    if (submitBtn) { submitBtn.disabled = false; submitBtn.textContent = 'Send'; }
  }
}

function applyLeaderboardHireFilter(checkbox) {
  loadLeaderboard(currentLbTab);
}

// ── Leaderboard (with hire filter support) ───────────────────────────────────

async function loadLeaderboard(tab) {
  const loadingEl = $('lb-loading');
  const rowsEl = $('lb-rows');
  if (!rowsEl) return;

  rowsEl.innerHTML = Array.from({ length: 5 }, () => `
    <tr>
      <td colspan="5" style="padding: 16px; opacity: 0.5;">Loading fast stats...</td>
    </tr>
  `).join('');

  if (loadingEl) loadingEl.style.display = 'none';

  const hireOnly = $('lb-filter-hire')?.checked;

  try {
    let endpoint = tab === 'global'
      ? '/leaderboards/global?limit=10'
      : `/leaderboards/language/${tab}?limit=10`;

    if (hireOnly) endpoint += '&available_for_hire=true';

    const data = await api(endpoint);
    const entries = data.leaderboard || [];

    if (entries.length === 0) {
      rowsEl.innerHTML = `<tr><td colspan="5" style="text-align:center; padding: 24px; color: var(--text-muted);">No data yet this week. Start coding!</td></tr>`;
      return;
    }

    rowsEl.innerHTML = entries.map((e) => {
      const username = e.username || '';
      let displayName = username || 'Unknown';
      if (displayName.length > 12) displayName = displayName.substring(0, 10) + '...';
      const nameCell = username
        ? `<a href="javascript:void(0)" onclick="openPublicProfile('${username}')" style="color:var(--text-main); text-decoration:underline; cursor:pointer;">${displayName}</a>`
        : displayName;
      return `
        <tr>
          <td class="td-main">${nameCell}</td>
          <td class="td-main">${fmt.seconds(e.seconds)}</td>
          <td>${e.top_language || e.language || '—'}</td>
          <td>${e.top_editor || '—'}</td>
          <td>${e.top_os || '—'}</td>
        </tr>
      `;
    }).join('');
  } catch (e) {
    rowsEl.innerHTML = `<tr><td colspan="5" style="text-align:center; padding: 24px; color: var(--text-muted);">Leaderboard loading failed.</td></tr>`;
  }
}

window.closePluginDiffModal = closePluginDiffModal;
window.openPluginCodeModal = openPluginCodeModal;
window.closePluginCodeModal = closePluginCodeModal;
window.openPluginDetailModal = openPluginDetailModal;
window.closePluginDetailModal = closePluginDetailModal;
window.setDetailRating = setDetailRating;
window.submitDetailReview = submitDetailReview;
window.submitScreenshot = submitScreenshot;
window.acceptPluginUpdateFromModal = acceptPluginUpdateFromModal;
window.saveProfileSettings = saveProfileSettings;
window.openPublicProfile = openPublicProfile;
window.closePublicProfile = closePublicProfile;
window.toggleFollow = toggleFollow;
window.openContactModal = openContactModal;
window.closeContactModal = closeContactModal;
window.submitContactDev = submitContactDev;
window.applyLeaderboardHireFilter = applyLeaderboardHireFilter;

function initLandingPage() {
  initScrollAnimations();
  initLeaderboard();
  initPluginTabs();
  loadPublicStats();

  // Plugin Store initialization
  if (isLoggedIn()) {
    const publishBtn = $('btn-publish-plugin');
    if (publishBtn) publishBtn.style.display = 'inline-flex';
    const editorBtn = $('btn-open-editor');
    if (editorBtn) editorBtn.style.display = 'inline-flex';
  }
  loadPluginStore();


  // Animate counters when hero stats come into view
  const statsSection = document.querySelector('.hero-stats');
  if (statsSection) {
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

document.addEventListener('DOMContentLoaded', async () => {
  await initAuth();
  applyActiveTheme();
  if (window.location.pathname === '/' || window.location.pathname === '') {
    initLandingPage();
  }
});

document.addEventListener('visibilitychange', () => {
  const _vDash = $('dashboard');
  if (document.visibilityState === 'visible' && currentToken && _vDash && !_vDash.classList.contains('hidden')) {
    if (!WS || WS.readyState === WebSocket.CLOSED) {
      _wsRetryCount = 0;
      _wsRetryDelay = 5000;
      connectWebSocket();
    }
  }
});

// ══════════════════════════════════════════════════════════════════════════════
// THEME SYSTEM
// ══════════════════════════════════════════════════════════════════════════════

// CSS variables exposed in the editor (label: varName)
const THEME_VARS = [
  { key: '--bg',           label: 'Background',        type: 'color' },
  { key: '--bg-card',      label: 'Card background',   type: 'color' },
  { key: '--bg-input',     label: 'Input background',  type: 'color' },
  { key: '--bg-hover',     label: 'Hover background',  type: 'color' },
  { key: '--text-main',    label: 'Primary text',      type: 'color' },
  { key: '--text-muted',   label: 'Muted text',        type: 'color' },
  { key: '--text-dark',    label: 'Subtle text',       type: 'color' },
  { key: '--border',       label: 'Border',            type: 'color' },
  { key: '--border-focus', label: 'Border focus',      type: 'color' },
  { key: '--accent',       label: 'Accent',            type: 'color' },
];

// Read computed CSS variable value from :root
function getCssVar(name) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

// Apply a flat object of CSS variables to :root (live preview)
function applyCssVars(vars) {
  if (!vars || typeof vars !== 'object') return;
  for (const [k, v] of Object.entries(vars)) {
    if (typeof v === 'string' && v) {
      document.documentElement.style.setProperty(k, v);
    }
  }
}

// Remove all custom CSS variable overrides from :root inline style
function clearCssVars(vars) {
  for (const v of THEME_VARS) {
    document.documentElement.style.removeProperty(v.key);
  }
}

// ── Load & apply saved theme on page load ─────────────────────────────────────

async function applyActiveTheme() {
  if (!currentToken) {
    // Try localStorage fallback for guests
    try {
      const saved = localStorage.getItem('ct_theme_vars');
      if (saved) applyCssVars(JSON.parse(saved));
    } catch (_) {}
    return;
  }
  try {
    const data = await api('/themes/active');
    // Merge: base theme variables first, then user custom_vars on top
    const merged = Object.assign({}, data.variables || {}, data.custom_vars || {});
    applyCssVars(merged);
    localStorage.setItem('ct_theme_vars', JSON.stringify(merged));
  } catch (_) {
    // Silently fall back to localStorage
    try {
      const saved = localStorage.getItem('ct_theme_vars');
      if (saved) applyCssVars(JSON.parse(saved));
    } catch (_2) {}
  }
}

// ── Store tab switching (Plugins ↔ Themes) ────────────────────────────────────

function switchStoreTab(tab, btn) {
  const pluginSection = document.getElementById('plugin-store');
  const themeSection = document.getElementById('theme-store-section');
  const pluginBtn = document.getElementById('store-tab-plugins');
  const themeBtn = document.getElementById('store-tab-themes');

  if (tab === 'plugins') {
    if (pluginSection) pluginSection.style.display = '';
    if (themeSection) themeSection.style.display = 'none';
    if (pluginBtn) pluginBtn.classList.add('active');
    if (themeBtn) themeBtn.classList.remove('active');
  } else {
    if (pluginSection) pluginSection.style.display = 'none';
    if (themeSection) themeSection.style.display = '';
    if (pluginBtn) pluginBtn.classList.remove('active');
    if (themeBtn) themeBtn.classList.add('active');
  }
}

// ── Theme Store: list community themes ────────────────────────────────────────

let _installedThemeIds = new Set();
let _activeThemeId = null;

async function loadThemeStore() {
  const grid = document.getElementById('theme-store-grid');
  if (!grid) return;
  try {
    const { themes } = await api('/themes');
    if (!themes || themes.length === 0) {
      grid.innerHTML = `<div style="color:var(--text-muted); padding:16px; grid-column:1/-1;">No themes published yet. Be the first!</div>`;
      return;
    }
    grid.innerHTML = themes.map(t => _renderThemeCard(t)).join('');
  } catch (e) {
    grid.innerHTML = `<div style="color:var(--text-muted); padding:16px;">Failed to load themes.</div>`;
  }
}

async function loadInstalledThemes() {
  const bar = document.getElementById('theme-installed-bar');
  const list = document.getElementById('theme-installed-list');
  if (!list || !currentToken) return;
  try {
    const [installedData, activeData] = await Promise.all([
      api('/themes/installed'),
      api('/themes/active'),
    ]);
    _activeThemeId = activeData.active_theme_id || null;
    _installedThemeIds = new Set((installedData.themes || []).map(t => t.id));

    if (installedData.themes && installedData.themes.length > 0) {
      if (bar) bar.style.display = '';
      list.innerHTML = installedData.themes.map(t => {
        const isActive = t.id === _activeThemeId;
        return `
          <div style="display:inline-flex; align-items:center; gap:6px; background:var(--bg-card); border:1px solid ${isActive ? 'var(--accent,var(--border-focus))' : 'var(--border)'}; border-radius:var(--radius-pill); padding:4px 10px; font-size:11px;">
            <span>${t.icon || '🎨'}</span>
            <span style="color:var(--text-main);">${t.display_name}</span>
            ${isActive
              ? `<span style="color:var(--text-dark);">✓ active</span>`
              : `<button onclick="activateTheme('${t.id}',${JSON.stringify(t.variables)},${JSON.stringify(t.custom_css||null)})" style="background:none;border:none;color:var(--text-dark);cursor:pointer;font-size:11px;padding:0;">Apply</button>`
            }
            <button onclick="uninstallTheme('${t.id}',this)" style="background:none;border:none;color:var(--text-dark);cursor:pointer;font-size:11px;padding:0 2px;">✕</button>
          </div>`;
      }).join('');

      // Refresh store grid to reflect install state
      loadThemeStore();
    }
  } catch (_) {}
}

function _renderThemeCard(t) {
  const installed = _installedThemeIds.has(t.id);
  const isActive = t.id === _activeThemeId;
  // Build a mini color swatch from variables
  const vars = t.variables || {};
  const swatchBg = vars['--bg'] || '#121212';
  const swatchCard = vars['--bg-card'] || '#18181b';
  const swatchText = vars['--text-main'] || '#ffffff';
  const swatchAccent = vars['--accent'] || vars['--border-focus'] || '#3f3f46';

  return `
    <div class="card" style="display:flex; flex-direction:column; gap:12px; position:relative;">
      <!-- Color preview swatch -->
      <div style="height:48px; border-radius:var(--radius-sm); overflow:hidden; display:grid; grid-template-columns:repeat(4,1fr); border:1px solid var(--border);">
        <div style="background:${swatchBg};"></div>
        <div style="background:${swatchCard};"></div>
        <div style="background:${swatchText};"></div>
        <div style="background:${swatchAccent};"></div>
      </div>
      <div>
        <div style="display:flex; align-items:center; gap:6px; margin-bottom:4px;">
          <span style="font-size:16px;">${t.icon || '🎨'}</span>
          <span style="font-size:13px; color:var(--text-main); font-weight:500;">${t.display_name}</span>
          ${isActive ? `<span style="font-size:10px; color:var(--text-dark);" class="key-hint">active</span>` : ''}
        </div>
        ${t.description ? `<p style="font-size:11px; color:var(--text-dark); margin:0; line-height:1.4;">${t.description}</p>` : ''}
        <div style="font-size:10px; color:var(--text-dark); margin-top:4px; font-family:var(--font-mono);">by @${t.author_username} · ↓${t.install_count}</div>
      </div>
      <div style="display:flex; gap:6px; flex-wrap:wrap; margin-top:auto;">
        <button class="btn" style="font-size:11px; padding:3px 10px;" onclick="previewTheme(${JSON.stringify(t.variables||{})})">Preview</button>
        ${installed
          ? `<button class="btn" style="font-size:11px; padding:3px 10px;" onclick="activateTheme('${t.id}',${JSON.stringify(t.variables)},${JSON.stringify(t.custom_css||null)})">${isActive ? '✓ Active' : 'Apply'}</button>
             <button class="btn" style="font-size:11px; padding:3px 10px; color:var(--text-dark);" onclick="uninstallTheme('${t.id}',this)">Uninstall</button>`
          : `<button class="btn" style="font-size:11px; padding:3px 10px;" onclick="installTheme('${t.id}',this)">Install</button>`
        }
      </div>
    </div>`;
}

async function installTheme(themeId, btn) {
  if (!currentToken) { showToast('Please log in to install themes.', [], 3000, 'warning'); return; }
  if (btn) { btn.disabled = true; btn.textContent = 'Installing…'; }
  try {
    await api(`/themes/install/${themeId}`, { method: 'POST' });
    _installedThemeIds.add(themeId);
    await loadInstalledThemes();
    showToast('Theme installed!', [], 2500);
  } catch (e) {
    showToast('Install failed: ' + e.message, [], 3000, 'warning');
    if (btn) { btn.disabled = false; btn.textContent = 'Install'; }
  }
}

async function uninstallTheme(themeId, btn) {
  if (!currentToken) return;
  if (btn) { btn.disabled = true; }
  try {
    await api(`/themes/uninstall/${themeId}`, { method: 'DELETE' });
    _installedThemeIds.delete(themeId);
    if (_activeThemeId === themeId) {
      _activeThemeId = null;
      clearCssVars();
      localStorage.removeItem('ct_theme_vars');
    }
    await loadInstalledThemes();
    showToast('Theme uninstalled.', [], 2500);
  } catch (e) {
    showToast('Failed: ' + e.message, [], 3000, 'warning');
  }
}

async function activateTheme(themeId, variables, customCss) {
  try {
    clearCssVars();
    applyCssVars(variables || {});
    _activeThemeId = themeId;
    localStorage.setItem('ct_theme_vars', JSON.stringify(variables || {}));

    await api('/themes/apply', {
      method: 'POST',
      body: JSON.stringify({ theme_id: themeId, custom_vars: {} }),
    });
    await loadInstalledThemes();
    showToast('Theme applied!', [], 2000);
    // Sync editor inputs with new theme values
    _syncEditorInputs(variables || {});
  } catch (e) {
    showToast('Failed to apply theme: ' + e.message, [], 3000, 'warning');
  }
}

// Live preview without saving
function previewTheme(variables) {
  clearCssVars();
  applyCssVars(variables || {});
  showToast('Previewing theme — click Apply to keep, Reset to revert.', [], 3000);
}

// ── CSS Variable Editor ────────────────────────────────────────────────────────

let _editorOrigVars = {};

async function initThemeEditor() {
  const grid = document.getElementById('theme-var-grid');
  if (!grid) return;

  // Capture current computed values as baseline
  for (const v of THEME_VARS) {
    _editorOrigVars[v.key] = getCssVar(v.key);
  }

  // Load saved custom_vars from server if logged in
  let savedVars = {};
  if (currentToken) {
    try {
      const data = await api('/themes/active');
      savedVars = Object.assign({}, data.variables || {}, data.custom_vars || {});
    } catch (_) {}
  } else {
    try {
      const s = localStorage.getItem('ct_theme_vars');
      if (s) savedVars = JSON.parse(s);
    } catch (_) {}
  }

  grid.innerHTML = THEME_VARS.map(v => {
    const current = savedVars[v.key] || _editorOrigVars[v.key] || '';
    // Determine input type: color picker if value looks like a hex color, else text
    const isHex = /^#[0-9a-fA-F]{3,8}$/.test(current);
    return `
      <div>
        <label style="font-size:10px; color:var(--text-dark); display:block; margin-bottom:4px; font-family:var(--font-mono);">${v.key}</label>
        <div style="font-size:11px; color:var(--text-muted); margin-bottom:4px;">${v.label}</div>
        <div style="display:flex; gap:6px; align-items:center;">
          ${isHex || v.type === 'color'
            ? `<input type="color" value="${isHex ? current : '#121212'}" data-var="${v.key}"
                 oninput="livePreviewVar('${v.key}', this.value); document.getElementById('text-${v.key.replace(/--/g,'').replace(/-/g,'')}').value=this.value;"
                 style="width:32px; height:28px; padding:2px; border:1px solid var(--border); background:var(--bg-card); border-radius:var(--radius-sm); cursor:pointer;">`
            : ''
          }
          <input type="text" id="text-${v.key.replace(/--/g,'').replace(/-/g,'')}" value="${current}" data-var="${v.key}"
            oninput="livePreviewVar('${v.key}', this.value)"
            style="flex:1; background:var(--bg); border:1px solid var(--border); color:var(--text-main); padding:5px 8px; font-size:11px; font-family:var(--font-mono); border-radius:var(--radius-sm);">
        </div>
      </div>`;
  }).join('');

  // Pre-populate publish theme vars textarea
  const varsEl = document.getElementById('theme-pub-vars');
  if (varsEl) varsEl.value = JSON.stringify(savedVars, null, 2);
}

function _syncEditorInputs(vars) {
  for (const v of THEME_VARS) {
    const val = vars[v.key];
    if (!val) continue;
    const textId = 'text-' + v.key.replace(/--/g,'').replace(/-/g,'');
    const textEl = document.getElementById(textId);
    if (textEl) textEl.value = val;
    const colorEl = document.querySelector(`input[type="color"][data-var="${v.key}"]`);
    if (colorEl && /^#[0-9a-fA-F]{3,8}$/.test(val)) colorEl.value = val;
  }
}

function livePreviewVar(varName, value) {
  document.documentElement.style.setProperty(varName, value);
  // Sync publish vars textarea
  const varsEl = document.getElementById('theme-pub-vars');
  if (varsEl) {
    const current = _collectEditorVars();
    varsEl.value = JSON.stringify(current, null, 2);
  }
}

function _collectEditorVars() {
  const vars = {};
  document.querySelectorAll('#theme-var-grid input[data-var]').forEach(input => {
    if (input.type !== 'color' && input.value.trim()) {
      vars[input.dataset.var] = input.value.trim();
    }
  });
  return vars;
}

async function saveCustomVars() {
  const vars = _collectEditorVars();
  const statusEl = document.getElementById('theme-save-status');
  try {
    await api('/themes/apply', {
      method: 'POST',
      body: JSON.stringify({ theme_id: _activeThemeId || null, custom_vars: vars }),
    });
    applyCssVars(vars);
    localStorage.setItem('ct_theme_vars', JSON.stringify(vars));
    if (statusEl) { statusEl.style.display = 'block'; setTimeout(() => { statusEl.style.display = 'none'; }, 2000); }
  } catch (e) {
    showToast('Failed to save: ' + e.message, [], 3000, 'warning');
  }
}

async function resetThemeEditor() {
  clearCssVars();
  _activeThemeId = null;
  localStorage.removeItem('ct_theme_vars');
  if (currentToken) {
    try {
      await api('/themes/apply', {
        method: 'POST',
        body: JSON.stringify({ theme_id: null, custom_vars: {} }),
      });
    } catch (_) {}
  }
  // Re-read computed defaults (now from stylesheet)
  const grid = document.getElementById('theme-var-grid');
  if (grid) initThemeEditor();
  showToast('Theme reset to default.', [], 2000);
}

// ── Publish Theme Modal ────────────────────────────────────────────────────────

function openPublishThemeModal() {
  const modal = document.getElementById('modal-publish-theme');
  if (!modal) return;
  // Pre-fill vars from editor
  const varsEl = document.getElementById('theme-pub-vars');
  if (varsEl) varsEl.value = JSON.stringify(_collectEditorVars(), null, 2);
  modal.style.display = 'flex';
}

function closePublishThemeModal() {
  const modal = document.getElementById('modal-publish-theme');
  if (modal) modal.style.display = 'none';
}

async function submitPublishTheme() {
  const name = document.getElementById('theme-pub-name')?.value.trim();
  const displayName = document.getElementById('theme-pub-display-name')?.value.trim();
  const errEl = document.getElementById('theme-pub-error');
  const btn = document.getElementById('btn-submit-theme');

  if (!name || !displayName) {
    if (errEl) { errEl.textContent = 'Name and Display Name are required.'; errEl.style.display = 'block'; }
    return;
  }

  let variables = {};
  try {
    const raw = document.getElementById('theme-pub-vars')?.value.trim();
    if (raw) variables = JSON.parse(raw);
  } catch (_) {
    if (errEl) { errEl.textContent = 'Invalid JSON in CSS Variables field.'; errEl.style.display = 'block'; }
    return;
  }

  if (btn) { btn.disabled = true; btn.textContent = 'Publishing…'; }
  if (errEl) errEl.style.display = 'none';

  try {
    await api('/themes/publish', {
      method: 'POST',
      body: JSON.stringify({
        name,
        display_name: displayName,
        description: document.getElementById('theme-pub-desc')?.value.trim() || null,
        version: document.getElementById('theme-pub-version')?.value.trim() || '1.0.0',
        icon: document.getElementById('theme-pub-icon')?.value.trim() || '🎨',
        variables,
      }),
    });
    closePublishThemeModal();
    loadThemeStore();
    showToast('Theme published!', [], 2500);
    ['theme-pub-name','theme-pub-display-name','theme-pub-desc','theme-pub-version','theme-pub-icon'].forEach(id => {
      const el = document.getElementById(id); if (el) el.value = '';
    });
  } catch (e) {
    if (errEl) { errEl.textContent = 'Failed: ' + e.message; errEl.style.display = 'block'; }
  } finally {
    if (btn) { btn.disabled = false; btn.textContent = 'Publish Theme'; }
  }
}

function exportThemeJSON() {
  const vars = _collectEditorVars();
  const blob = new Blob([JSON.stringify(vars, null, 2)], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = 'codetrackr-theme.json';
  a.click();
  URL.revokeObjectURL(url);
}

function importThemeJSON(input) {
  const file = input.files && input.files[0];
  if (!file) return;
  const reader = new FileReader();
  reader.onload = function(e) {
    try {
      const vars = JSON.parse(e.target.result);
      if (typeof vars !== 'object' || Array.isArray(vars)) {
        showToast('Invalid theme file.', [], 3000, 'warning');
        return;
      }
      applyCssVars(vars);
      _syncEditorInputs(vars);
      const varsEl = document.getElementById('theme-pub-vars');
      if (varsEl) varsEl.value = JSON.stringify(vars, null, 2);
      showToast('Theme imported — click Save to persist.', [], 3000);
    } catch (_) {
      showToast('Could not parse JSON file.', [], 3000, 'warning');
    }
    input.value = '';
  };
  reader.readAsText(file);
}

window.exportThemeJSON = exportThemeJSON;
window.importThemeJSON = importThemeJSON;
window.switchStoreTab = switchStoreTab;
window.loadThemeStore = loadThemeStore;
window.loadInstalledThemes = loadInstalledThemes;
window.initThemeEditor = initThemeEditor;
window.installTheme = installTheme;
window.uninstallTheme = uninstallTheme;
window.activateTheme = activateTheme;
window.previewTheme = previewTheme;
window.livePreviewVar = livePreviewVar;
window.saveCustomVars = saveCustomVars;
window.resetThemeEditor = resetThemeEditor;
window.openPublishThemeModal = openPublishThemeModal;
window.closePublishThemeModal = closePublishThemeModal;
window.submitPublishTheme = submitPublishTheme;
