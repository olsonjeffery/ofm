/**
 * Playwright E2E tests for the AgentDropdown component.
 *
 * These tests verify the replacement of the old Live WebSocket indicator with
 * the new Bulma dropdown that combines WebSocket connection status + active
 * agent session entries.
 *
 * Setup:
 *   1. Start ofm on port 3246:  OFM_PORT=3246 OFM_RAUTHY_ENABLED=true cargo run
 *   2. Note the admin password from the startup logs
 *   3. Run: npx playwright test tests/playwright/agent_dropdown.spec.ts
 *
 * Requires: rauthy enabled server, a project + task + conversation seeded via API.
 */

import { test, expect } from '@playwright/test';

const BASE = 'http://localhost:3246';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async function navigateThroughLogin(page, adminPassword: string) {
  // 1. Go to webapp -> redirected to rauthy login
  await page.goto(`${BASE}/webapp`);
  await page.waitForURL('**/auth/v1/**');

  // 2. Fill rauthy login form
  await page.fill('input[name="username"]', 'admin@localhost');
  await page.fill('input[name="password"]', adminPassword);
  await page.click('button[type="submit"]');

  // 3. Consent screen (if rauthy shows one)
  const consentBtn = page.locator('button:has-text("Allow")');
  if (await consentBtn.isVisible({ timeout: 3000 }).catch(() => false)) {
    await consentBtn.click();
  }

  // 4. Should land on /webapp
  await page.waitForURL('**/webapp');
}

async function seedAgentRun(apiBase: string, adminPassword: string) {
  // Get an access token from rauthy first
  const tokenResp = await fetch(`${apiBase}/auth/v1/token`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({
      grant_type: 'password',
      client_id: 'ofm',
      username: 'admin@localhost',
      password: adminPassword,
      scope: 'openid profile email',
    }),
  });
  const tokenData = await tokenResp.json();
  const accessToken = tokenData.access_token;

  // Create project via API
  const projResp = await fetch(`${apiBase}/api/projects`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      Authorization: `Bearer ${accessToken}`,
    },
    body: JSON.stringify({ name: 'E2E Test Project', repo_folder_path: '/tmp/e2e-test' }),
  });
  const project = await projResp.json();
  const projectId = project.id;

  // Create task via API
  const taskResp = await fetch(`${apiBase}/api/projects/${projectId}/tasks`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      Authorization: `Bearer ${accessToken}`,
    },
    body: JSON.stringify({ title: 'E2E Agent Test Task' }),
  });
  const task = await taskResp.json();
  const taskId = task.id;

  return { projectId, taskId, accessToken };
}

// ---------------------------------------------------------------------------
// Scenario 1: Connection status + empty agents
// ---------------------------------------------------------------------------
test('scenario-1: connection status and empty agents', async ({ page }) => {
  // Assumes server is running on port 3246 with rauthy enabled
  // and we seed no agent runs.

  // Log in through rauthy
  const adminPassword = process.env.OFM_ADMIN_PASSWORD || '';
  await navigateThroughLogin(page, adminPassword);

  // The webapp dashboard should be loaded at this point
  await page.waitForSelector('#agent-dropdown');

  // Assert the dropdown button shows "0 Agents"
  const agentCount = page.locator('#agent-count');
  await expect(agentCount).toHaveText('0 Agents');

  // Assert the button is NOT disabled
  const trigger = page.locator('#agent-dropdown-trigger');
  await expect(trigger).not.toBeDisabled();

  // Click the button to open the dropdown
  await trigger.click();

  // Assert the first entry shows connection status
  const wsStatus = page.locator('#ws-status-entry');
  await expect(wsStatus).toBeVisible();

  // Assert the "last payload" text shows "No payloads yet"
  const lastPayload = page.locator('#ws-last-payload');
  await expect(lastPayload).toHaveText('No payloads yet');
});

// ---------------------------------------------------------------------------
// Scenario 2: Agent appears after starting, status persists across navigation
// ---------------------------------------------------------------------------
test('scenario-2: agent appears and persists across navigation', async ({ page, request }) => {
  const adminPassword = process.env.OFM_ADMIN_PASSWORD || '';

  // Log in through rauthy
  await navigateThroughLogin(page, adminPassword);

  // Seed an agent run via API (before navigating to the task detail page)
  const { projectId, taskId, accessToken } = await seedAgentRun(BASE, adminPassword);

  // Navigate to the task detail page (where agent-run buttons exist)
  await page.goto(`${BASE}/webapp/projects/${projectId}/tasks/${taskId}`);

  // Wait for the page to load
  await page.waitForSelector('#agent-dropdown');

  // Click the "Impl" agent-run button
  const implBtn = page.locator('button:has-text("Impl")');
  await implBtn.click();

  // On success, JS triggers window.location.reload()
  // Wait for it to complete
  await page.waitForTimeout(2000);

  // Assert the dropdown shows "1 Agents"
  const agentCount = page.locator('#agent-count');
  await expect(agentCount).toHaveText('1 Agents');

  // Open the dropdown
  const trigger = page.locator('#agent-dropdown-trigger');
  await trigger.click();

  // Assert agent entry with correct icon + task title is present
  const agentDropdownItem = page.locator('.dropdown-item[href*="/chat/"]');
  await expect(agentDropdownItem).toBeVisible();
  await expect(agentDropdownItem).toContainText('E2E Agent');

  // Assert it has the correct agent type icon (mdi-code-tags for Implementation)
  const agentIcon = agentDropdownItem.locator('.mdi-code-tags');
  await expect(agentIcon).toBeVisible();

  // Navigate to a different page (dashboard)
  await page.goto(`${BASE}/webapp`);
  await page.waitForSelector('#agent-dropdown');

  // Assert the dropdown still shows "1 Agents"
  await expect(agentCount).toHaveText('1 Agents');
});

// ---------------------------------------------------------------------------
// Scenario 3: Connection status updates in real-time
// ---------------------------------------------------------------------------
test('scenario-3: connection status updates in real-time', async ({ page }) => {
  const adminPassword = process.env.OFM_ADMIN_PASSWORD || '';
  await navigateThroughLogin(page, adminPassword);

  await page.waitForSelector('#agent-dropdown');

  // Open the dropdown
  const trigger = page.locator('#agent-dropdown-trigger');
  await trigger.click();

  // Wait a moment for WebSocket to connect
  await page.waitForTimeout(3000);

  // Check if the status entry updated
  const wsIcon = page.locator('#ws-icon');
  const wsLabel = page.locator('#ws-label');

  // At this point WS should be connected (or at least trying)
  // The status text should be one of: 'Connected', 'Connecting...', or 'Disconnected'
  const labelText = await wsLabel.textContent();
  expect(['Connected', 'Connecting...', 'Disconnected']).toContain(labelText);

  // If connected, the icon should be mdi-wifi
  if (labelText === 'Connected') {
    await expect(wsIcon).toHaveClass(/mdi-wifi/);
  } else {
    // If disconnected or connecting, icon should be mdi-wifi-off
    await expect(wsIcon).toHaveClass(/mdi-wifi-off/);
  }

  // Check that the "Last:" text exists (either "No payloads yet" or a time ago string)
  const lastPayload = page.locator('#ws-last-payload');
  const payloadText = await lastPayload.textContent();
  expect(payloadText).toMatch(/^(No payloads yet|Last: (Just now|\d+s ago|\d+m ago|\d+h ago))$/);
});
