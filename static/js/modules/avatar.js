/* ═══════════════════════════════════════════════════════
   CodeTrackr — avatar.js
   Handles: avatar fallbacks (identicons)
   ═══════════════════════════════════════════════════════ */

import { minidenticon } from '../vendor/minidenticons.min.js';

const cache = new Map();

function identiconDataUrl(seed, saturation = 90, lightness = 50) {
  if (!seed) return '';
  const key = `${seed}|${saturation}|${lightness}`;
  if (cache.has(key)) return cache.get(key);
  const svg = minidenticon(seed, saturation, lightness);
  const url = `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
  cache.set(key, url);
  return url;
}

export function avatarUrlForUser(user, opts = {}) {
  if (user && user.avatar_url) return user.avatar_url;
  const seed = (user && (user.username || user.display_name || user.id)) || '';
  return identiconDataUrl(seed, opts.saturation ?? 90, opts.lightness ?? 50);
}
