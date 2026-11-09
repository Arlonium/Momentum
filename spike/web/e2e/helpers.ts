import type { Browser, Page } from "@playwright/test";

/** Ouvre une page du spike pour un utilisateur et attend la fin de la synchro initiale. */
export async function openPage(browser: Browser, page: string, user: string, extra = ""): Promise<Page> {
  const context = await browser.newContext();
  const p = await context.newPage();
  await p.goto(`/?page=${encodeURIComponent(page)}&user=${encodeURIComponent(user)}${extra}`);
  await p.waitForFunction(() => (window as any).__mementum?.provider?.synced === true, null, { timeout: 30_000 });
  return p;
}

/** Contenu XML du fragment partagé (vue Yjs, indépendante du rendu). */
export async function fragmentXml(p: Page): Promise<string> {
  return p.evaluate(() => (window as any).__mementum.doc.getXmlFragment("document-store").toString());
}

export function uniquePage(prefix: string): string {
  return `${prefix}-${Date.now()}-${Math.floor(Math.random() * 1e6)}`;
}

export function percentile(values: number[], p: number): number {
  const s = [...values].sort((a, b) => a - b);
  return s[Math.round((s.length - 1) * p)];
}
