# CodeTrackr Frontend - Modular Architecture

## Overview

The CodeTrackr frontend has been refactored from a 2500+ line monolith to a modular architecture using native ES modules. This improves maintainability, facilitates testing, and allows better code organization.

## Module Structure

### Core Modules

- **`auth.js`** - Authentication management, tokens, user state
- **`api.js`** - API communication, formatting utilities
- **`ui.js`** - UI utilities, toast notifications, interface updates
- **`router.js`** - Client-side routing, view management
- **`init.js`** - Main orchestration, application initialization

### Feature Modules

- **`dashboard.js`** - Dashboard, statistics, charts, plugin panels
- **`profile.js`** - Public profiles, follow system
- **`websocket.js`** - WebSocket connections, real-time updates
- **`leaderboard.js`** - Leaderboards, dynamic tabs
- **`plugin-store.js`** - Plugin store, installation/uninstallation
- **`theme.js`** - Theme system, CSS variables, theme editor

### Utility Modules

- **`animations.js`** - Scroll animations, animated counters
- **`navigation.js`** - Navigation, hamburger menu, route handling
- **`stats.js`** - Public statistics
- **`plugins.js`** - Plugin tab system

## Problem Solved

### Before: `initAuth()` with Multiple Responsibilities

The original `initAuth()` function had 4 distinct responsibilities:

```javascript
// BEFORE - Everything in one function (40-79 lines)
async function initAuth() {
  // 1. OAuth handling
  if (oauthCode && !token) { /* redirect */ }
  
  // 2. Token management  
  if (token) { localStorage.setItem(...) }
  else { currentToken = localStorage.getItem(...) }
  
  // 3. Route detection
  const profileMatch = window.location.pathname.match(/^\/u\/([^/]+)$/);
  if (profileMatch) { /* handle profile */ }
  
  // 4. UI initialization
  if (isLoggedIn()) {
    await updateUserUI();
    if ($('dashboard')) await loadDashboard();
  }
}
```

### After: Separation of Responsibilities

```javascript
// NOW - Each module with its responsibility
// auth.js - Only handles OAuth and tokens
export function handleOAuthCallback() { /* OAuth */ }
export function setToken(token) { /* token management */ }

// router.js - Only handles routes
export function detectRoute() { /* route detection */ }

// init.js - Orchestrates initialization
export async function initAuth() {
  handleOAuthCallback();
  setToken(getTokenFromHash());
  const route = detectRoute();
  // ... coordinate modules
}
```

## Benefits of Modular Architecture

### 1. **Single Responsibility Principle**
Each module has a clear and defined responsibility.

### 2. **Better Maintainability**
- Easy to find where each functionality is located
- Changes in one area don't affect other areas
- Simpler debugging

### 3. **Facilitated Testing**
- Each module can be tested independently
- Simpler dependency mocking
- More granular code coverage

### 4. **Potential Code Splitting**
- On-demand module loading
- Better initial performance
- Modules can be lazy-loaded

### 5. **Improved Collaboration**
- Different developers can work on different modules
- Fewer merge conflicts
- More focused code reviews

## Migration and Compatibility

### Global Exports
To maintain compatibility with existing HTML handlers:

```javascript
// app.js - Exports functions globally during transition
window.logout = logout;
window.openPublicProfile = (username) => {
  import('./modules/profile.js').then(module => 
    module.openPublicProfile(username)
  );
};
```

### Lazy Loading
Less critical functions are loaded dynamically:

```javascript
window.openPluginDetailModal = (pluginId, installedIds) => {
  import('./modules/plugin-detail.js').then(module => 
    module.openPluginDetailModal(pluginId, installedIds)
  );
};
```

## Usage

### Development
```html
<script type="module" src="js/app.js"></script>
```

### Individual Testing
```javascript
import { isLoggedIn, setToken } from './modules/auth.js';
import { api, fmt } from './modules/api.js';
```

## Next Steps

1. **Complete missing modules** - Some functions still need to be moved to specific modules
2. **Unit testing** - Add tests for each module
3. **TypeScript migration** - Optional: migrate to TypeScript for better type safety
4. **Bundle optimization** - Configure bundler for production

## Reference Files

- `static/index_modular.html` - Test page to verify modules
- `static/js/app.js` - Main entry point (now ~45 lines vs 2500+)
- `static/js/modules/` - All modules organized by responsibility
