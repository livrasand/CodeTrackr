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
        if (data.token) localStorage.setItem('ct_token', data.token);
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
  currentToken = null;
  currentUser = null;
  window.location.reload();
}

export function getCurrentToken() {
  return currentToken;
}

export function getCurrentUser() {
  return currentUser;
}

export function setCurrentUser(user) {
  currentUser = user;
}

// Anonymous authentication (Mullvad-style)
export async function createAnonymousAccount() {
  try {
    const response = await fetch('/auth/anonymous/create', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
    });
    
    if (!response.ok) {
      throw new Error('Failed to create anonymous account');
    }
    
    const data = await response.json();
    if (data.token) {
      localStorage.setItem('ct_token', data.token);
      currentToken = data.token;
      currentUser = data.user;
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
    if (data.token) {
      localStorage.setItem('ct_token', data.token);
      currentToken = data.token;
      currentUser = data.user;
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
