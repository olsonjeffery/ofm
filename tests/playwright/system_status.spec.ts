/**
 * Playwright E2E tests for System Status & Health.
 *
 * Setup:
 *   1. Start ofm on port 3167:
 *        OFM_PORT=3167 OFM_FOOTPRINT="$PWD/.ofm" OFM_RAUTHY_ENABLED=true cargo run
 *   2. Note the admin password from the startup logs (OFM_ADMIN_PASSWORD env).
 *   3. Run: npx playwright test tests/playwright/system_status.spec.ts
 *
 * Requires: rauthy enabled server (so the live health report is populated with
 * the rauthy + hiqlite + opencode-pool entries).
 */

import { test, expect } from '@playwright/test';

const BASE = 'http://localhost:3167';

async function navigateThroughLogin(page, adminPassword: string) {
  await page.goto(`${BASE}/webapp`);
  await page.waitForURL('**/auth/v1/**');
  await page.fill('input[name="username"]', 'admin@localhost');
  await page.fill('input[name="password"]', adminPassword);
  await page.click('button[type="submit"]');
  const consentBtn = page.locator('button:has-text("Allow")');
  if (await consentBtn.isVisible({ timeout: 3000 }).catch(() => false)) {
    await consentBtn.click();
  }
  await page.waitForURL('**/webapp');
}

// ---------------------------------------------------------------------------
// Scenario 1: navbar badge + dropdown links (admin)
// ---------------------------------------------------------------------------
test('scenario-1: admin navbar shows health badge, settings + agents dropdown entries', async ({
  page,
}) => {
  const adminPassword = process.env.OFM_ADMIN_PASSWORD || '';
  await navigateThroughLogin(page, adminPassword);

  await page.waitForSelector('#system-health-badge');

  // Badge shows a numeric running-services count (or – before the first fetch).
  const count = page.locator('#system-health-count');
  const text = (await count.textContent()) || '';
  expect(text === '–' || /^\d+$/.test(text)).toBe(true);

  // Settings dropdown has the admin-only "System" item.
  await page.click('#settings-dropdown-trigger');
  const settingsSystem = page.locator('#settings-system-item');
  await expect(settingsSystem).toBeVisible();
  await expect(settingsSystem).toHaveAttribute('href', '/webapp/system');

  // Agents dropdown has the "System Status" link for all users.
  await page.click('#agent-dropdown-trigger');
  const agentsSystem = page.locator('.dropdown-item[href="/webapp/system"]');
  await expect(agentsSystem).toBeVisible();
  await expect(agentsSystem).toContainText('System Status');
});

// ---------------------------------------------------------------------------
// Scenario 2: System Status page renders dependency + live sections
// ---------------------------------------------------------------------------
test('scenario-2: system status page renders report sections and live cards', async ({
  page,
}) => {
  const adminPassword = process.env.OFM_ADMIN_PASSWORD || '';
  await navigateThroughLogin(page, adminPassword);

  await page.goto(`${BASE}/webapp/system`);
  await page.waitForSelector('#system-status-page');

  await expect(page.locator('h1', { hasText: 'System Status' })).toBeVisible();
  await expect(page.locator('#system-status-markdown h2', { hasText: 'Dependency Check' })).toBeVisible();
  await expect(page.locator('#system-status-markdown h2', { hasText: 'Live System Health' })).toBeVisible();

  // Live cards include the opencode-pool + hiqlite resources with a status icon.
  const cards = page.locator('#system-status-cards .card');
  await expect(cards.first()).toBeVisible();
  await expect(cards.first()).toContainText('live:');

  // The badge updates to a numeric value once the page fetches.
  await expect
    .poll(async () => (await page.locator('#system-health-count').textContent()) || '')
    .toMatch(/^\d+$/);

  // No console errors during initial render.
  const errors: string[] = [];
  page.on('console', (msg) => {
    if (msg.type() === 'error') errors.push(msg.text());
  });
  await page.reload();
  await page.waitForSelector('#system-status-page');
  await page.waitForTimeout(1000);
  expect(errors).toEqual([]);
});

// ---------------------------------------------------------------------------
// Scenario 3: timestamps are localized via data-utc
// ---------------------------------------------------------------------------
test('scenario-3: page timestamps carry data-utc attributes', async ({ page }) => {
  const adminPassword = process.env.OFM_ADMIN_PASSWORD || '';
  await navigateThroughLogin(page, adminPassword);

  await page.goto(`${BASE}/webapp/system`);
  await page.waitForSelector('#system-status-page');

  // At least one element carries a machine-readable UTC timestamp.
  const utcCount = await page.locator('[data-utc]').count();
  expect(utcCount).toBeGreaterThan(0);
});
