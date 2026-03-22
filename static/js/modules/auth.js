/* ═══════════════════════════════════════════════════════
   CodeTrackr — auth.js
   Handles: OAuth flow, token management, authentication state
   ═══════════════════════════════════════════════════════ */

let currentToken = null;
let currentUser = null;

// OAuth handling
export function handleOAuthCallback() {
  // Server-issued exchange code in hash fragment (#exchange=<uuid>) after GitHub OAuth
  const hashParams = new URLSearchParams(window.location.hash.slice(1));
  const exchangeCode = hashParams.get('exchange');
  if (exchangeCode) {
    fetch('/auth/exchange', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ code: exchangeCode }),
    }).then(async res => {
      if (res.ok) {
        const data = await res.json();
        const accessToken = data.access_token || data.token;
        if (accessToken) {
          localStorage.setItem('ct_token', accessToken);
          currentToken = accessToken;
        }
        if (data.refresh_token) {
          localStorage.setItem('ct_refresh_token', data.refresh_token);
        }
        window.location.href = '/';
      } else {
        window.location.href = '/?error=auth_failed';
      }
    }).catch(() => {
      window.location.href = '/?error=auth_failed';
    });
    return true;
  }

  // GitHub redirected here instead of /auth/github/callback — misconfigured callback URL
  const params = new URLSearchParams(window.location.search);
  const oauthCode = params.get('code');
  if (oauthCode && !getTokenFromHash()) {
    window.location.href = `/auth/github/callback?${params.toString()}`;
    return true; // Indicates redirect is happening
  }
  return false;
}

// Token management
export function getTokenFromHash() {
  const hashParams = new URLSearchParams(window.location.hash.slice(1));
  return hashParams.get('token');
}

export function setToken(token) {
  if (token) {
    localStorage.setItem('ct_token', token);
    currentToken = token;
    window.history.replaceState({}, '', window.location.pathname);
  }
}

export function loadStoredToken() {
  currentToken = localStorage.getItem('ct_token');
  return currentToken;
}

export function isLoggedIn() {
  return !!currentToken;
}

export function logout() {
  localStorage.removeItem('ct_token');
  localStorage.removeItem('ct_refresh_token');
  currentToken = null;
  currentUser = null;
  window.location.reload();
}

export function getCurrentToken() {
  if (!currentToken) {
    currentToken = localStorage.getItem('ct_token');
  }
  return currentToken;
}

export function getCurrentUser() {
  return currentUser;
}

export function setCurrentUser(user) {
  currentUser = user;
}

// Refresh token management
export function getRefreshToken() {
  return localStorage.getItem('ct_refresh_token');
}

export async function refreshAccessToken() {
  const refreshToken = getRefreshToken();
  if (!refreshToken) {
    throw new Error('No refresh token available');
  }
  
  try {
    const response = await fetch('/auth/refresh', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ refresh_token: refreshToken }),
    });
    
    if (!response.ok) {
      throw new Error('Token refresh failed');
    }
    
    const data = await response.json();
    if (data.access_token) {
      localStorage.setItem('ct_token', data.access_token);
      currentToken = data.access_token;
      
      // Update refresh token if provided
      if (data.refresh_token) {
        localStorage.setItem('ct_refresh_token', data.refresh_token);
      }
    }
    
    return data;
  } catch (error) {
    console.error('Token refresh failed:', error);
    // Clear tokens on refresh failure
    logout();
    throw error;
  }
}

// Anonymous authentication (Mullvad-style)
export async function createAnonymousAccount() {
  try {
    const response = await fetch('/auth/anonymous/create', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
    });
    
    if (!response.ok) {
      const error = await response.json();
      throw new Error(error.error || 'Failed to create anonymous account');
    }
    
    const data = await response.json();
    if (data.access_token) {
      localStorage.setItem('ct_token', data.access_token);
      currentToken = data.access_token;
      currentUser = data.user;
      
      // Store refresh token if available
      if (data.refresh_token) {
        localStorage.setItem('ct_refresh_token', data.refresh_token);
      }
    }
    return data;
  } catch (error) {
    console.error('Anonymous account creation failed:', error);
    throw error;
  }
}

export async function loginWithAccountNumber(accountNumber) {
  try {
    const response = await fetch('/auth/anonymous/login', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ account_number: accountNumber }),
    });
    
    if (!response.ok) {
      const error = await response.json();
      throw new Error(error.error || 'Login failed');
    }
    
    const data = await response.json();
    if (data.access_token) {
      localStorage.setItem('ct_token', data.access_token);
      currentToken = data.access_token;
      currentUser = data.user;
      
      // Store refresh token if available
      if (data.refresh_token) {
        localStorage.setItem('ct_refresh_token', data.refresh_token);
      }
    }
    return data;
  } catch (error) {
    console.error('Anonymous login failed:', error);
    throw error;
  }
}

export async function verifyAccountNumber(accountNumber) {
  try {
    const response = await fetch('/auth/anonymous/verify', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ account_number: accountNumber }),
    });
    
    if (!response.ok) {
      throw new Error('Verification failed');
    }
    
    return await response.json();
  } catch (error) {
    console.error('Account number verification failed:', error);
    throw error;
  }
}

export function formatAccountNumber(number) {
  // Format as XXXX XXXX XXXX XXXX for better readability
  return number.replace(/(\d{4})(\d{4})(\d{4})(\d{4})/, '$1 $2 $3 $4');
}

export function validateAccountNumber(number) {
  // Remove spaces and check if exactly 16 digits
  const cleanNumber = number.replace(/\s/g, '');
  return cleanNumber.length === 16 && /^\d+$/.test(cleanNumber);
}
