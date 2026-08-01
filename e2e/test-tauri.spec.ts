import { test } from './fixtures/tauriMock';
test('tauri event test', async ({ page }) => {
  await page.goto('/');
  await page.waitForSelector('aside');
  
  page.on('console', msg => console.log('BROWSER CONSOLE:', msg.text()));

  const ok = await page.evaluate(() => {
    return new Promise(resolve => {
      setTimeout(() => {
        const listeners = (window as unknown as { __TAURI_LISTENERS__: Record<string, string[]> }).__TAURI_LISTENERS__;
        console.log("Listeners:", Object.keys(listeners));
        const corruptId = listeners['db.corrupted']?.[0];
        console.log("Corrupt ID:", corruptId);
        const win = window as unknown as Record<string, unknown>;
        const func = win['_' + corruptId] || win[corruptId];
        console.log("Func:", !!func);
        
        window.dispatchEvent(new CustomEvent('test-tauri-event', { detail: { event: 'db.corrupted', payload: {} } }));
        resolve(!!func);
      }, 1000);
    });
  });
  console.log("Is func valid?", ok);
  await page.waitForTimeout(1000);
  const text = await page.content();
  console.log("Has Database Corrupted?", text.includes('Database Corrupted'));
});
