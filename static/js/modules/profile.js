/* ═══════════════════════════════════════════════════════
   CodeTrackr — profile.js
   Handles: public profiles, user profiles, follow system
   ═══════════════════════════════════════════════════════ */

import { $, showToast } from './ui.js';
import { api, fmt } from './api.js';
import { isLoggedIn, getCurrentUser } from './auth.js';
import { hideAllViews } from './router.js';

let _ppPrevSection = null;

export async function openPublicProfile(username) {
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
  hideAllViews();
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
    if (actionsEl && isLoggedIn() && getCurrentUser()?.username !== username) {
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

export function closePublicProfile() {
  const ppSection = $('public-profile');
  if (ppSection) ppSection.classList.add('hidden');

  if (_ppPrevSection === 'dashboard') {
    window.history.pushState({}, '', '/');
    const dash = $('dashboard');
    if (dash) dash.classList.remove('hidden');
  } else {
    window.history.pushState({}, '', '/');
    const { showLanding } = require('./router.js');
    showLanding();
  }
}

export async function toggleFollow(username, doFollow, btn) {
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
