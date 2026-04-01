/* ═══════════════════════════════════════════════════════
   CodeTrackr — billing.js
   Handles: Stripe checkout, billing operations
   ═══════════════════════════════════════════════════════ */

import { api } from './api.js';

// Global export for inline onclick handlers
window.startCheckout = startCheckout;

export async function startCheckout(button, priceIdOverride) {
  if (!button) return;
  
  try {
    button.disabled = true;
    button.textContent = 'Loading...';

    // Get billing config to fetch price ID
    const config = await api('/billing/config');
    const price_id = priceIdOverride || config.price_id;
    if (!price_id) {
      throw new Error('Billing not configured');
    }

    // Create checkout session
    const session = await api('/billing/checkout', {
      method: 'POST',
      body: JSON.stringify({ price_id })
    });

    if (session.url) {
      window.location.href = session.url;
    } else {
      throw new Error('No checkout URL returned');
    }
  } catch (error) {
    console.error('Checkout error:', error);
    button.disabled = false;
    button.textContent = '★ Upgrade to Pro';
    alert('Failed to start checkout: ' + error.message);
  }
}

export async function getBillingStatus() {
  try {
    return await api('/billing/status');
  } catch (error) {
    console.error('Failed to get billing status:', error);
    return null;
  }
}

export async function openBillingPortal() {
  try {
    const session = await api('/billing/portal', {
      method: 'POST'
    });
    
    if (session.url) {
      window.location.href = session.url;
    }
  } catch (error) {
    console.error('Failed to open billing portal:', error);
    alert('Failed to open billing portal: ' + error.message);
  }
}
