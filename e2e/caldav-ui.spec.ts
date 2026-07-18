import { test, expect } from '@playwright/test';
import * as http from 'http';

let mockServer: http.Server;

test.beforeAll(() => {
  mockServer = http.createServer((req, res) => {
    let body = '';
    req.on('data', chunk => {
      body += chunk;
    });
    req.on('end', () => {
      res.writeHead(207, { 'Content-Type': 'application/xml; charset=utf-8' });
      res.end(`<?xml version="1.0" encoding="utf-8" ?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <D:response>
    <D:href>/cal/4cb38c7656ae483d8ee5650d9fb02108/1.ics</D:href>
    <D:propstat>
      <D:prop>
        <calendar-data>BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
UID:1
SUMMARY:Đám cưới Khang
DTSTART:20260807T070000Z
DTEND:20260807T110000Z
END:VEVENT
END:VCALENDAR</calendar-data>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/cal/4cb38c7656ae483d8ee5650d9fb02108/2.ics</D:href>
    <D:propstat>
      <D:prop>
        <calendar-data>BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
UID:2
SUMMARY:Company Trip Đà Nẵng - Hội An
DTSTART:20260725T000000Z
DTEND:20260727T000000Z
END:VEVENT
END:VCALENDAR</calendar-data>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/cal/4cb38c7656ae483d8ee5650d9fb02108/3.ics</D:href>
    <D:propstat>
      <D:prop>
        <calendar-data>BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
UID:3
SUMMARY:Test Event - has date
DTSTART:20260720T000000Z
DTEND:20260721T000000Z
END:VEVENT
END:VCALENDAR</calendar-data>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>`);
    });
  });
  mockServer.listen(8090, '127.0.0.1');
});

test.afterAll(() => {
  mockServer.close();
});

test('should load /caldav-ui, fill out the form, submit, and verify events are rendered', async ({ page }) => {
  // 1. Visit /caldav-ui
  await page.goto('/caldav-ui');

  // Verify the page heading is correct
  await expect(page.locator('h1')).toHaveText('CalDAV Server Test Client');

  // 2. Fill server URL/username/password/path pointing to our mock server
  await page.fill('#server_url', 'http://127.0.0.1:8090');
  await page.fill('#username', '');
  await page.fill('#password', '');
  await page.fill('#calendar_path', '/cal/4cb38c7656ae483d8ee5650d9fb02108');

  // 3. Submit
  await page.click('button[type="submit"]');

  // 4. Verify events list is rendered with expected titles from the public feed
  await expect(page.locator('.results-header')).toBeVisible({ timeout: 15000 });

  // Verify that the expected titles are shown
  await expect(page.getByText('Đám cưới Khang')).toBeVisible();
  await expect(page.getByText('Company Trip Đà Nẵng - Hội An')).toBeVisible();
  await expect(page.getByText('Test Event - has date')).toBeVisible();
});
