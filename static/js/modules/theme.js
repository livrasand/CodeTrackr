/* ═══════════════════════════════════════════════════════
   CodeTrackr — theme.js
   Handles: theme system, CSS variables, theme store
   ═══════════════════════════════════════════════════════ */

import { $, showToast } from './ui.js';
import { api } from './api.js';
import { getCurrentToken } from './auth.js';

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
export function applyCssVars(vars) {
  if (!vars || typeof vars !== 'object') return;
  for (const [k, v] of Object.entries(vars)) {
    if (typeof v === 'string' && v) {
      document.documentElement.style.setProperty(k, v);
    }
  }
}

// Remove all custom CSS variable overrides from :root inline style
export function clearCssVars() {
  for (const v of THEME_VARS) {
    document.documentElement.style.removeProperty(v.key);
  }
}

// Load & apply saved theme on page load
export async function applyActiveTheme() {
  const token = getCurrentToken();
  if (!token) {
    // Try localStorage fallback for guests
    try {
      const saved = localStorage.getItem('ct_theme_vars');
      if (saved) applyCssVars(JSON.parse(saved));
    } catch (_) {}
    return;
  }
  try {
    const data = await fetchActiveTheme();
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

// Store tab switching (Plugins ↔ Themes)
export function switchStoreTab(tab, btn) {
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

// Theme store functionality
let _installedThemeIds = new Set();
let _activeThemeId = null;
let _themesCache = null;
let _themesFetchedAt = 0;
let _loadThemesInFlight = null;
let _loadInstalledInFlight = null;
const THEMES_CACHE_MS = 15000;
let _activeThemeCache = null;
let _activeThemeFetchedAt = 0;
let _activeThemeInFlight = null;
let _installedThemesCache = null;
let _installedThemesFetchedAt = 0;
let _installedThemesInFlight = null;
const ACTIVE_THEME_CACHE_MS = 15000;
const INSTALLED_THEMES_CACHE_MS = 15000;

function fetchActiveTheme(force = false) {
  const now = Date.now();
  if (!force && _activeThemeCache && (now - _activeThemeFetchedAt) < ACTIVE_THEME_CACHE_MS) {
    return Promise.resolve(_activeThemeCache);
  }
  if (_activeThemeInFlight) return _activeThemeInFlight;
  _activeThemeInFlight = api('/themes/active')
    .then((data) => {
      _activeThemeCache = data;
      _activeThemeFetchedAt = Date.now();
      return data;
    })
    .finally(() => {
      _activeThemeInFlight = null;
    });
  return _activeThemeInFlight;
}

function fetchInstalledThemes(force = false) {
  const now = Date.now();
  if (!force && _installedThemesCache && (now - _installedThemesFetchedAt) < INSTALLED_THEMES_CACHE_MS) {
    return Promise.resolve(_installedThemesCache);
  }
  if (_installedThemesInFlight) return _installedThemesInFlight;
  _installedThemesInFlight = api('/themes/installed')
    .then((data) => {
      _installedThemesCache = data;
      _installedThemesFetchedAt = Date.now();
      return data;
    })
    .finally(() => {
      _installedThemesInFlight = null;
    });
  return _installedThemesInFlight;
}

function _renderThemeStore(themes) {
  const grid = document.getElementById('theme-store-grid');
  if (!grid) return;
  if (!themes || themes.length === 0) {
    grid.innerHTML = `<div style="color:var(--text-muted); padding:16px; grid-column:1/-1;">No themes published yet. Be the first!</div>`;
    return;
  }
  grid.innerHTML = themes.map(t => _renderThemeCard(t)).join('');
}

export async function loadThemeStore(force = false) {
  const grid = document.getElementById('theme-store-grid');
  if (!grid) return;

  const now = Date.now();
  if (!force && _themesCache && (now - _themesFetchedAt) < THEMES_CACHE_MS) {
    _renderThemeStore(_themesCache);
    return;
  }

  if (_loadThemesInFlight) return _loadThemesInFlight;

  _loadThemesInFlight = (async () => {
    try {
      const { themes } = await api('/themes');
      _themesCache = themes || [];
      _themesFetchedAt = Date.now();
      _renderThemeStore(_themesCache);
    } catch (e) {
      if (_themesCache) {
        _renderThemeStore(_themesCache);
      } else if (grid) {
        grid.innerHTML = `<div style="color:var(--text-muted); padding:16px;">Failed to load themes.</div>`;
      }
    } finally {
      _loadThemesInFlight = null;
    }
  })();

  return _loadThemesInFlight;
}

export async function loadInstalledThemes(force = false) {
  const bar = document.getElementById('theme-installed-bar');
  const list = document.getElementById('theme-installed-list');
  if (!list || !getCurrentToken()) return;
  if (_loadInstalledInFlight) return _loadInstalledInFlight;

  _loadInstalledInFlight = (async () => {
    try {
      const [installedData, activeData] = await Promise.all([
        fetchInstalledThemes(force),
        fetchActiveTheme(force),
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
      } else {
        if (bar) bar.style.display = 'none';
        list.innerHTML = '';
      }

      // Refresh store grid to reflect install state without spamming /themes
      if (_themesCache) {
        _renderThemeStore(_themesCache);
      } else {
        await loadThemeStore();
      }
    } catch (_) {
      // ignore
    } finally {
      _loadInstalledInFlight = null;
    }
  })();

  return _loadInstalledInFlight;
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

export async function installTheme(themeId, btn) {
  if (!getCurrentToken()) { showToast('Please log in to install themes.', [], 3000, 'warning'); return; }
  if (btn) { btn.disabled = true; btn.textContent = 'Installing…'; }
  try {
    await api(`/themes/install/${themeId}`, { method: 'POST' });
    _installedThemeIds.add(themeId);
    await loadInstalledThemes(true);
    showToast('Theme installed!', [], 2500);
  } catch (e) {
    showToast('Install failed: ' + e.message, [], 3000, 'warning');
    if (btn) { btn.disabled = false; btn.textContent = 'Install'; }
  }
}

export async function uninstallTheme(themeId, btn) {
  if (!getCurrentToken()) return;
  if (btn) { btn.disabled = true; }
  try {
    await api(`/themes/uninstall/${themeId}`, { method: 'DELETE' });
    _installedThemeIds.delete(themeId);
    if (_activeThemeId === themeId) {
      _activeThemeId = null;
      clearCssVars();
      localStorage.removeItem('ct_theme_vars');
    }
    await loadInstalledThemes(true);
    showToast('Theme uninstalled.', [], 2500);
  } catch (e) {
    showToast('Failed: ' + e.message, [], 3000, 'warning');
  }
}

export async function activateTheme(themeId, variables, customCss) {
  try {
    clearCssVars();
    applyCssVars(variables || {});
    _activeThemeId = themeId;
    localStorage.setItem('ct_theme_vars', JSON.stringify(variables || {}));

    await api('/themes/apply', {
      method: 'POST',
      body: JSON.stringify({ theme_id: themeId, custom_vars: {} }),
    });
    await loadInstalledThemes(true);
    showToast('Theme applied!', [], 2000);
    // Sync editor inputs with new theme values
    _syncEditorInputs(variables || {});
  } catch (e) {
    showToast('Failed to apply theme: ' + e.message, [], 3000, 'warning');
  }
}

// Live preview without saving
export function previewTheme(variables) {
  clearCssVars();
  applyCssVars(variables || {});
  showToast('Previewing theme — click Apply to keep, Reset to revert.', [], 3000);
}

// Theme editor functionality
let _editorOrigVars = {};

export async function initThemeEditor() {
  const grid = document.getElementById('theme-var-grid');
  if (!grid) return;

  // Capture current computed values as baseline
  for (const v of THEME_VARS) {
    _editorOrigVars[v.key] = getCssVar(v.key);
  }

  // Load saved custom_vars from server if logged in
  let savedVars = {};
  const token = getCurrentToken();
  if (token) {
    try {
      const data = await fetchActiveTheme();
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

export function livePreviewVar(varName, value) {
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

export async function saveCustomVars() {
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

export async function resetThemeEditor() {
  clearCssVars();
  _activeThemeId = null;
  localStorage.removeItem('ct_theme_vars');
  const token = getCurrentToken();
  if (token) {
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

// Export theme functionality
export function exportThemeJSON() {
  const vars = _collectEditorVars();
  const blob = new Blob([JSON.stringify(vars, null, 2)], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = 'codetrackr-theme.json';
  a.click();
  URL.revokeObjectURL(url);
}

export function importThemeJSON(input) {
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

export function openPublishThemeModal() {
  const modal = document.getElementById('modal-publish-theme');
  if (!modal) return;
  const vars = _collectEditorVars();
  const varsEl = document.getElementById('theme-pub-vars');
  if (varsEl) varsEl.value = JSON.stringify(vars, null, 2);
  const errEl = document.getElementById('theme-pub-error');
  if (errEl) errEl.style.display = 'none';
  modal.style.display = 'flex';
}

export function closePublishThemeModal() {
  const modal = document.getElementById('modal-publish-theme');
  if (modal) modal.style.display = 'none';
}

export async function submitPublishTheme() {
  const token = getCurrentToken();
  if (!token) return;

  const name = document.getElementById('theme-pub-name')?.value.trim();
  const displayName = document.getElementById('theme-pub-display-name')?.value.trim();
  const description = document.getElementById('theme-pub-desc')?.value.trim();
  const version = document.getElementById('theme-pub-version')?.value.trim() || '1.0.0';
  const icon = document.getElementById('theme-pub-icon')?.value.trim() || '🎨';
  const varsRaw = document.getElementById('theme-pub-vars')?.value.trim();
  const errEl = document.getElementById('theme-pub-error');

  if (!name || !displayName) {
    if (errEl) { errEl.textContent = 'Name and display name are required.'; errEl.style.display = 'block'; }
    return;
  }

  let variables = {};
  try {
    variables = varsRaw ? JSON.parse(varsRaw) : {};
  } catch (_) {
    if (errEl) { errEl.textContent = 'Invalid JSON in CSS Variables field.'; errEl.style.display = 'block'; }
    return;
  }

  const btn = document.getElementById('btn-submit-theme');
  const originalText = btn ? btn.textContent : 'Publish Theme';
  if (btn) { btn.textContent = 'Publishing...'; btn.disabled = true; }

  try {
    await api('/themes/publish', {
      method: 'POST',
      body: JSON.stringify({ name, display_name: displayName, description, version, icon, variables }),
    });
    showToast('Theme published!', [], 3000);
    closePublishThemeModal();
    loadThemeStore(true);
  } catch (e) {
    if (errEl) { errEl.textContent = e.message || 'Error publishing theme.'; errEl.style.display = 'block'; }
    if (btn) { btn.textContent = originalText; btn.disabled = false; }
  }
}

// Make functions globally available
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
window.exportThemeJSON = exportThemeJSON;
window.importThemeJSON = importThemeJSON;
window.openPublishThemeModal = openPublishThemeModal;
window.closePublishThemeModal = closePublishThemeModal;
window.submitPublishTheme = submitPublishTheme;
