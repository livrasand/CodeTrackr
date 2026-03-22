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
  const token = localStorage.getItem('ct_token');
  if (!token) {
    const errorEl = document.getElementById('pub-error');
    errorEl.textContent = 'Debes iniciar sesión para publicar plugins.';
    errorEl.style.display = 'block';
    return;
  }
  
  const name = document.getElementById('pub-name').value.trim();
  const displayName = document.getElementById('pub-display-name').value.trim();
  const description = document.getElementById('pub-description').value.trim();
  const version = document.getElementById('pub-version').value.trim();
  const icon = document.getElementById('pub-icon').value.trim();
  const repo = document.getElementById('pub-repo').value.trim();
  const pluginType = document.getElementById('pub-plugin-type').value;
  const widgetType = document.getElementById('pub-widget-type').value;
  const script = document.getElementById('pub-script').value.trim();
  
  // Validaciones básicas
  if (!name || !displayName || !script) {
    const errorEl = document.getElementById('pub-error');
    errorEl.textContent = 'Nombre, display name y script son obligatorios.';
    errorEl.style.display = 'block';
    return;
  }
  
  // Detectar acceso a red
  const hasExternalAccess = detectExternalNetworkAccess(script);
  
  const btn = document.getElementById('btn-submit-publish');
  const originalText = btn.textContent;
  btn.textContent = 'Publicando...';
  btn.disabled = true;
  
  try {
    const response = await fetch('/api/v1/store/publish', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${token}`
      },
      body: JSON.stringify({
        name,
        display_name: displayName,
        description,
        version,
        icon,
        repository_url: repo,
        plugin_type: pluginType,
        widget_type: widgetType,
        script,
        has_external_access: hasExternalAccess
      })
    });
    
    if (!response.ok) {
      const error = await response.json();
      throw new Error(error.error || 'Error al publicar plugin');
    }
    
    btn.textContent = '¡Publicado!';
    setTimeout(() => {
      closePublishModal();
      loadPluginStore(); // Recargar la tienda
    }, 1500);
    
  } catch (error) {
    console.error('Error publicando plugin:', error);
    const errorEl = document.getElementById('pub-error');
    errorEl.textContent = error.message || 'Error al publicar plugin';
    errorEl.style.display = 'block';
    btn.textContent = originalText;
    btn.disabled = false;
  }
}

// Export functions that are used globally
window.installPlugin = installPlugin;
window.uninstallPluginFromStore = uninstallPluginFromStore;
window.loadPluginStore = loadPluginStore;
window.openPublishModal = openPublishModal;
window.closePublishModal = closePublishModal;
window.submitPublishPlugin = submitPublishPlugin;
