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
    if (filter === 'all') _allPlugins = plugins;

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
      
      // Indicador visual para plugins con acceso a red
      const networkBadge = p.has_external_access 
        ? `<span style="background:#ff6b6b; color:white; font-size:9px; padding:1px 4px; border-radius:2px; margin-left:4px;" title="Este plugin accede a servicios externos">🌐 RED</span>`
        : '';
      
      return (
        `<div class="card plugin-card" style="display:flex; flex-direction:column; cursor:pointer;" data-plugin-id="${pid}" data-installed="${isInstalled ? '1' : '0'}">` +
          `<div style="display:flex; justify-content:space-between; align-items:flex-start; margin-bottom:4px;">` +
            `<h3 style="margin:0; font-size:15px;">${icon} ${p.display_name}${networkBadge}</h3>` +
            `<span class="key-hint" style="font-size:10px; padding:2px 6px;">v${p.version}</span>` +
          `</div>` +
          `<div style="font-size:11px; color:var(--text-dark); font-family:var(--font-mono); margin-bottom:4px;">${p.name}</div>` +
          (p.author_username ? `<div style="font-size:11px; color:var(--text-muted); margin-bottom:6px;" onclick="event.stopPropagation(); openPublicProfile('${p.author_username}')">by <span style="cursor:pointer; text-decoration:underline; color:var(--text-dark);">@${p.author_username}</span></div>` : '') +
          `<p style="font-size:12px; margin:4px 0 8px; color:var(--text-muted); flex-grow:1; line-height:1.5;">${linkifyDescription(p.description, plugins)}</p>` +
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

  // Primero obtener detalles del plugin para verificar si tiene acceso a red
  try {
    const pluginData = await api(`/store/plugin/${pluginId}/detail`);
    
    // Si el plugin tiene acceso externo, mostrar modal de consentimiento
    if (pluginData.has_external_access) {
      showNetworkConsentModal(pluginId, btn, pluginData);
      return;
    }
    
    // Si no tiene acceso externo, instalar normalmente
    proceedWithInstallation(pluginId, btn);
    
  } catch (error) {
    console.error('Error checking plugin details:', error);
    // Si hay error getting details, proceed with installation anyway
    proceedWithInstallation(pluginId, btn);
  }
}

function showNetworkConsentModal(pluginId, btn, pluginData) {
  // Crear modal si no existe
  let modal = document.getElementById('modal-network-consent');
  if (!modal) {
    modal = document.createElement('div');
    modal.id = 'modal-network-consent';
    modal.style.cssText = 'display:none; position:fixed; inset:0; background:rgba(0,0,0,.7); z-index:1000; align-items:center; justify-content:center;';
    modal.innerHTML = `
      <div class="card" style="width:100%; max-width:480px; margin:0 16px; max-height:90vh; overflow-y:auto;">
        <div style="display:flex; justify-content:space-between; align-items:center; margin-bottom:20px;">
          <h3 style="margin:0; font-size:14px; color:var(--text-main);">⚠️ Advertencia de Acceso a Red</h3>
          <button onclick="closeNetworkConsentModal()" style="background:none; border:none; color:var(--text-muted); cursor:pointer; font-size:18px;">✕</button>
        </div>
        
        <div style="display:flex; flex-direction:column; gap:16px;">
          <div>
            <strong>Plugin: ${pluginData.display_name}</strong>
            <div style="font-size:11px; color:var(--text-dark); font-family:var(--font-mono); margin-top:2px;">${pluginData.name}</div>
          </div>
          
          <div style="background:#fff3cd; border:1px solid #ffeaa7; border-radius:4px; padding:12px; font-size:12px; color:#856404;">
            <strong>⚠️ Este plugin accede a servicios externos</strong><br><br>
            <ul style="margin:8px 0; padding-left:20px;">
              <li>El plugin contactará servidores externos desde tu navegador</li>
              <li>Tu <strong>dirección IP real</strong> será visible para esos servidores</li>
              <li>Tu <strong>User-Agent</strong> (navegador y sistema) será compartido</li>
              <li>Si el plugin usa tu token de forma incorrecta, podría exponer tus datos</li>
            </ul>
            <strong>¿Confías en el autor de este plugin?</strong>
          </div>
          
          <div style="font-size:11px; color:var(--text-muted);">
            <strong>Autor:</strong> @${pluginData.author_username}<br>
            <strong>Repositorio:</strong> ${pluginData.repository_url || 'No especificado'}
          </div>
          
          <div style="display:flex; gap:8px;">
            <button class="btn" onclick="closeNetworkConsentModal()" style="flex:1; color:var(--text-dark);">Cancelar</button>
            <button class="btn" id="btn-consent-install" style="flex:1; background:#e53; border-color:#e53;">Entiendo los riesgos - Instalar</button>
          </div>
        </div>
      </div>
    `;
    document.body.appendChild(modal);
  }
  
  // Guardar referencia al plugin y botón
  modal.dataset.pluginId = pluginId;
  modal.dataset.btnId = btn.id || Date.now().toString();
  
  // Configurar botón de instalación
  const installBtn = modal.querySelector('#btn-consent-install');
  installBtn.onclick = () => {
    closeNetworkConsentModal();
    proceedWithInstallation(pluginId, btn);
  };
  
  // Mostrar modal
  modal.style.display = 'flex';
}

function closeNetworkConsentModal() {
  const modal = document.getElementById('modal-network-consent');
  if (modal) {
    modal.style.display = 'none';
  }
}

async function proceedWithInstallation(pluginId, btn) {
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

function linkifyDescription(text, allPlugins) {
  if (!text) return 'No description provided.';
  return text.replace(/@([a-zA-Z0-9_-]+)/g, (match, name) => {
    const found = allPlugins.find(p => p.name === name);
    if (!found) return match;
    return `<span style="color:var(--accent); cursor:pointer; text-decoration:underline;" onclick="event.stopPropagation(); openPluginDetailModal('${found.id}', new Set())">${match}</span>`;
  });
}

function starsHtml(avg, total) {
  const filled = Math.round(avg);
  let s = '';
  for (let i = 1; i <= 5; i++) s += i <= filled ? '★' : '☆';
  return s;
}

let _detailPluginId = null;
let _detailRating = 0;
let _allPlugins = [];

export async function openPluginDetailModal(pluginId, installedIds) {
  const modal = document.getElementById('modal-plugin-detail');
  if (!modal) return;

  _detailPluginId = pluginId;
  _detailRating = 0;

  // Limpiar mientras carga
  const ids = ['detail-title','detail-version','detail-name','detail-description',
                'detail-stars','detail-rating-count','detail-installs','detail-actions',
                'detail-reviews-list'];
  ids.forEach(id => { const el = document.getElementById(id); if (el) el.textContent = ''; });
  modal.style.display = 'flex';

  try {
    const resp = await api(`/store/plugin/${pluginId}/detail`);
    const p = resp.plugin || resp;
    const reviews = resp.reviews || [];

    // Header
    const titleEl = document.getElementById('detail-title');
    if (titleEl) titleEl.textContent = `${p.icon || '🔌'} ${p.display_name}`;
    const versionEl = document.getElementById('detail-version');
    if (versionEl) versionEl.textContent = `v${p.version}`;
    const nameEl = document.getElementById('detail-name');
    if (nameEl) nameEl.textContent = p.name;

    // Rating
    const avg = p.avg_rating ? Number(p.avg_rating) : 0;
    const starsEl = document.getElementById('detail-stars');
    if (starsEl) starsEl.textContent = starsHtml(Math.round(avg), 5);
    const ratingCountEl = document.getElementById('detail-rating-count');
    if (ratingCountEl) ratingCountEl.textContent = p.rating_count ? `${avg.toFixed(1)} (${p.rating_count})` : 'No ratings yet';

    // Description
    const descEl = document.getElementById('detail-description');
    if (descEl) descEl.innerHTML = linkifyDescription(p.description, _allPlugins);

    // Meta
    const installsEl = document.getElementById('detail-installs');
    if (installsEl) installsEl.textContent = p.install_count || 0;
    const repoEl = document.getElementById('detail-repo');
    if (repoEl) {
      if (p.repository) {
        repoEl.href = p.repository;
        repoEl.style.display = 'inline';
      } else {
        repoEl.style.display = 'none';
      }
    }

    // Author (usar author_username del plugin o del nivel raíz de la respuesta)
    const authorUsername = p.author_username || resp.author_username;
    if (authorUsername) {
      const nameEl = document.getElementById('detail-name');
      if (nameEl) nameEl.innerHTML = `${p.name} &nbsp;·&nbsp; <span style="cursor:pointer; text-decoration:underline;" onclick="openPublicProfile('${authorUsername}')">@${authorUsername}</span>`;
    }

    // Actions
    const actionsEl = document.getElementById('detail-actions');
    if (actionsEl) {
      const isInstalled = installedIds instanceof Set ? installedIds.has(String(pluginId)) : false;
      const isOwn = isLoggedIn() && getCurrentUser()?.username === authorUsername;
      let btns = '';
      if (isInstalled) {
        btns += `<button class="btn" style="font-size:12px; color:var(--text-dark);" onclick="uninstallPluginFromStore('${pluginId}', this); closePluginDetailModal();">Uninstall</button>`;
      } else {
        btns += `<button class="btn" style="font-size:12px;" onclick="installPlugin('${pluginId}', this)">Install</button>`;
      }
      if (p.script) {
        btns += `<button class="btn" style="font-size:12px; color:var(--text-dark);" onclick="openPluginCodeModal('${pluginId}', '${(p.display_name||'').replace(/'/g,"\\'")}', '${p.version}')">View Code</button>`;
      }
      if (!isOwn && isLoggedIn() && !isInstalled) {
        btns += `<button class="btn" style="font-size:12px; color:var(--text-dark);" onclick="openReportModal('${pluginId}')">⚑ Report</button>`;
      }
      if (isOwn) {
        btns += `<button class="btn" style="font-size:12px; color:#e53;" onclick="authorDeletePlugin('${pluginId}', this); closePluginDetailModal();">Delete</button>`;
      }
      actionsEl.innerHTML = btns;
    }

    // Review section
    const reviewSection = document.getElementById('detail-review-section');
    if (reviewSection) reviewSection.style.display = isLoggedIn() ? 'block' : 'none';

    // Reviews list
    const reviewsEl = document.getElementById('detail-reviews-list');
    if (reviewsEl) {
      if (reviews.length === 0) {
        reviewsEl.innerHTML = '<div style="font-size:12px; color:var(--text-muted);">No reviews yet.</div>';
      } else {
        reviewsEl.innerHTML = reviews.map(r => `
          <div style="border-top:1px solid var(--border); padding-top:10px; margin-top:10px;">
            <div style="display:flex; justify-content:space-between; margin-bottom:4px;">
              <span style="font-size:12px; font-weight:600; color:var(--text-main);">@${r.username || 'anonymous'}</span>
              <span style="font-size:12px; color:var(--accent);">${starsHtml(r.rating || 0, 5)}</span>
            </div>
            ${r.body ? `<p style="margin:0; font-size:12px; color:var(--text-muted); line-height:1.5;">${r.body}</p>` : ''}
          </div>`).join('');
      }
    }

  } catch (e) {
    console.warn('Plugin detail error:', e);
    const descEl = document.getElementById('detail-description');
    if (descEl) descEl.textContent = 'Error loading plugin details.';
  }
}

export function closePluginDetailModal() {
  const modal = document.getElementById('modal-plugin-detail');
  if (modal) modal.style.display = 'none';
  _detailPluginId = null;
}

export function setDetailRating(value) {
  _detailRating = value;
  const btns = document.querySelectorAll('#detail-star-input button');
  btns.forEach((btn, i) => { btn.textContent = i < value ? '★' : '☆'; });
}

export async function submitDetailReview() {
  if (!_detailPluginId || !isLoggedIn()) return;
  const body = document.getElementById('detail-review-body')?.value.trim();
  try {
    await api(`/store/plugin/${_detailPluginId}/review`, {
      method: 'POST',
      body: JSON.stringify({ rating: _detailRating || null, body: body || null }),
    });
    showToast('Review submitted!', [], 3000, 'success');
    openPluginDetailModal(_detailPluginId, new Set());
  } catch (e) {
    showToast('Error submitting review: ' + e.message, [], 4000, 'danger');
  }
}

export async function submitScreenshot() {
  if (!_detailPluginId) return;
  const url = document.getElementById('detail-screenshot-url')?.value.trim();
  const caption = document.getElementById('detail-screenshot-caption')?.value.trim();
  if (!url) return;
  try {
    await api(`/store/plugin/${_detailPluginId}/screenshot`, {
      method: 'POST',
      body: JSON.stringify({ url, caption }),
    });
    showToast('Screenshot added!', [], 3000, 'success');
    openPluginDetailModal(_detailPluginId, new Set());
  } catch (e) {
    showToast('Error adding screenshot: ' + e.message, [], 4000, 'danger');
  }
}

export async function openPluginCodeModal(pluginId, displayName, version) {
  const modal = document.getElementById('modal-plugin-code');
  if (!modal) return;
  const titleEl = document.getElementById('code-modal-title');
  const metaEl = document.getElementById('code-modal-meta');
  const contentEl = document.getElementById('code-modal-content');
  if (titleEl) titleEl.textContent = displayName;
  if (metaEl) metaEl.textContent = `v${version}`;
  if (contentEl) contentEl.textContent = 'Loading…';
  modal.style.display = 'flex';
  try {
    const data = await api(`/store/plugin/${pluginId}/script`);
    if (contentEl) contentEl.textContent = data.script || '(no script)';
  } catch (e) {
    if (contentEl) contentEl.textContent = 'Error loading script.';
  }
}

export function closePluginCodeModal() {
  const modal = document.getElementById('modal-plugin-code');
  if (modal) modal.style.display = 'none';
}

// Función para detectar si un plugin script intenta hacer peticiones externas
function detectExternalNetworkAccess(script) {
  // Patrones para detectar intentos de acceso a red externa
  const patterns = [
    // fetch con URLs externas
    /fetch\s*\(\s*['"`]https?:\/\/(?!\/)/gi,
    /fetch\s*\(\s*['"`]\/\/(?!localhost|127\.0\.0\.1)/gi,
    // XMLHttpRequest con URLs externas
    /\.open\s*\(\s*['"`](GET|POST|PUT|DELETE|PATCH)['"`]\s*,\s*['"`]https?:\/\/(?!\/)/gi,
    // Creación de URLs externas
    /new\s+URL\s*\(\s*['"`]https?:\/\/(?!\/)/gi,
    // APIs comunes externas
    /api\.openai\.com/gi,
    /api\.github\.com/gi,
    /api\.twitter\.com/gi,
    /graph\.facebook\.com/gi,
    /googleapis\.com/gi,
  ];
  
  for (const pattern of patterns) {
    if (pattern.test(script)) {
      return true;
    }
  }
  
  // Detectar URLs con dominios externos
  const urlPattern = /https?:\/\/([a-zA-Z0-9.-]+)(?![\/])|\/\/([a-zA-Z0-9.-]+)(?![\/])/g;
  const matches = script.match(urlPattern);
  if (matches) {
    for (const match of matches) {
      const domain = match.replace(/^(https?:\/\/|\/\/)/, '');
      // Excluir dominios locales y del mismo sitio
      if (!domain.includes('localhost') && 
          !domain.includes('127.0.0.1') &&
          !domain.includes(window.location.hostname)) {
        return true;
      }
    }
  }
  
  return false;
}

// Función para mostrar advertencia de red en modal de publicación
function showNetworkWarningIfNeeded(script) {
  const hasExternalAccess = detectExternalNetworkAccess(script);
  const warningEl = document.getElementById('pub-network-warning');
  
  if (hasExternalAccess) {
    if (!warningEl) {
      const scriptRow = document.getElementById('pub-script-row');
      const warningDiv = document.createElement('div');
      warningDiv.id = 'pub-network-warning';
      warningDiv.style.cssText = 'background:#fff3cd; border:1px solid #ffeaa7; border-radius:4px; padding:8px; margin-bottom:8px; font-size:11px; color:#856404;';
      warningDiv.innerHTML = `
        <strong>⚠️ Acceso a red detectado</strong><br>
        Este plugin intenta contactar servidores externos. Los usuarios verán una advertencia antes de instalarlo y su IP será visible para los servidores externos.
      `;
      scriptRow.parentNode.insertBefore(warningDiv, scriptRow);
    }
  } else if (warningEl) {
    warningEl.remove();
  }
  
  return hasExternalAccess;
}

// Funciones para manejar el modal de publicación
function openPublishModal() {
  const modal = document.getElementById('modal-publish');
  if (modal) {
    modal.style.display = 'flex';
    // Limpiar formulario
    document.getElementById('pub-name').value = '';
    document.getElementById('pub-display-name').value = '';
    document.getElementById('pub-description').value = '';
    document.getElementById('pub-version').value = '0.1.0';
    document.getElementById('pub-icon').value = '🔌';
    document.getElementById('pub-repo').value = '';
    document.getElementById('pub-plugin-type').value = 'widget';
    document.getElementById('pub-widget-type').value = 'counter';
    document.getElementById('pub-script').value = '';
    document.getElementById('pub-error').style.display = 'none';
    
    // Agregar listener para detectar acceso a red
    const scriptTextarea = document.getElementById('pub-script');
    if (scriptTextarea) {
      scriptTextarea.addEventListener('input', function() {
        showNetworkWarningIfNeeded(this.value);
      });
    }
  }
}

function closePublishModal() {
  const modal = document.getElementById('modal-publish');
  if (modal) {
    modal.style.display = 'none';
  }
}

async function submitPublishPlugin() {
  const errorEl = document.getElementById('pub-error');
  errorEl.style.display = 'none';

  const name = document.getElementById('pub-name').value.trim().toLowerCase();
  const displayName = document.getElementById('pub-display-name').value.trim();
  const description = document.getElementById('pub-description').value.trim() || null;
  const version = document.getElementById('pub-version').value.trim() || null;
  const icon = document.getElementById('pub-icon').value.trim() || null;
  const repo = document.getElementById('pub-repo').value.trim() || null;
  const widgetType = document.getElementById('pub-widget-type')?.value || null;
  const script = document.getElementById('pub-script').value.trim() || null;

  if (!name || !displayName) {
    errorEl.textContent = 'Nombre y display name son obligatorios.';
    errorEl.style.display = 'block';
    return;
  }
  if (!/^[a-z0-9-]+$/.test(name)) {
    errorEl.textContent = 'El nombre debe ser kebab-case (solo letras minúsculas, números y guiones).';
    errorEl.style.display = 'block';
    return;
  }
  if (!script) {
    errorEl.textContent = 'El script es obligatorio.';
    errorEl.style.display = 'block';
    return;
  }

  const hasExternalAccess = detectExternalNetworkAccess(script);

  const btn = document.getElementById('btn-submit-publish');
  const originalText = btn.textContent;
  btn.textContent = 'Publicando...';
  btn.disabled = true;

  try {
    const pluginType = document.getElementById('pub-plugin-type')?.value || 'widget';
    await api('/store/publish', {
      method: 'POST',
      body: JSON.stringify({
        name,
        display_name: displayName,
        description,
        version,
        icon,
        repository: repo,
        plugin_type: pluginType,
        widget_type: widgetType,
        script,
        has_external_access: hasExternalAccess
      })
    });

    btn.textContent = '¡Publicado!';
    setTimeout(() => {
      closePublishModal();
      loadPluginStore();
    }, 1500);

  } catch (error) {
    console.error('Error publicando plugin:', error);
    errorEl.textContent = error.message || 'Error al publicar plugin';
    errorEl.style.display = 'block';
    btn.textContent = originalText;
    btn.disabled = false;
  }
}

let _deletePluginId = null;
let _deletePluginBtn = null;

export function authorDeletePlugin(pluginId, btn) {
  _deletePluginId = pluginId;
  _deletePluginBtn = btn || null;

  const nameEl = document.getElementById('delete-plugin-name');
  if (nameEl) {
    const card = btn ? btn.closest('[data-plugin-id]') : null;
    const nameInCard = card ? card.querySelector('h3') : null;
    nameEl.textContent = nameInCard ? nameInCard.textContent.trim() : pluginId;
  }

  const modal = document.getElementById('modal-delete-plugin');
  if (modal) modal.style.display = 'flex';
}

export function closeDeletePluginModal() {
  const modal = document.getElementById('modal-delete-plugin');
  if (modal) modal.style.display = 'none';
  _deletePluginId = null;
  _deletePluginBtn = null;
}

export async function confirmDeletePlugin() {
  if (!_deletePluginId) return;
  const btn = document.getElementById('btn-confirm-delete-plugin');
  if (btn) { btn.disabled = true; btn.textContent = 'Deleting…'; }

  try {
    await api(`/store/my/${_deletePluginId}`, { method: 'DELETE' });
    closeDeletePluginModal();
    const { showToast } = await import('./ui.js');
    showToast('Plugin deleted.', [], 3000);
    await loadPluginStore();
  } catch (e) {
    const { showToast } = await import('./ui.js');
    showToast('Failed to delete: ' + e.message, [], 4000, 'danger');
    if (btn) { btn.disabled = false; btn.textContent = 'Delete'; }
  }
}

// Export functions that are used globally
window.installPlugin = installPlugin;
window.uninstallPluginFromStore = uninstallPluginFromStore;
window.loadPluginStore = loadPluginStore;
window.openPublishModal = openPublishModal;
window.closePublishModal = closePublishModal;
window.submitPublishPlugin = submitPublishPlugin;
window.openPluginDetailModal = openPluginDetailModal;
window.closePluginDetailModal = closePluginDetailModal;
window.setDetailRating = setDetailRating;
window.submitDetailReview = submitDetailReview;
window.submitScreenshot = submitScreenshot;
window.openPluginCodeModal = openPluginCodeModal;
window.closePluginCodeModal = closePluginCodeModal;
window.authorDeletePlugin = authorDeletePlugin;
window.closeDeletePluginModal = closeDeletePluginModal;
window.confirmDeletePlugin = confirmDeletePlugin;
