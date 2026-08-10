import '@testing-library/jest-dom';

// Vitest's jsdom pool defers `localStorage`/`window.localStorage` to Node
// 22+'s own experimental implementation, which throws without a
// --localstorage-file flag (verified: raw jsdom outside vitest works fine —
// this is specific to how vitest wires globals). A minimal in-memory
// Storage polyfill sidesteps both, rather than threading a Node flag through
// the test runner for every contributor's environment.
class MemoryStorage implements Storage {
  private store = new Map<string, string>();
  get length() {
    return this.store.size;
  }
  clear() {
    this.store.clear();
  }
  getItem(key: string) {
    return this.store.has(key) ? this.store.get(key)! : null;
  }
  key(index: number) {
    return Array.from(this.store.keys())[index] ?? null;
  }
  removeItem(key: string) {
    this.store.delete(key);
  }
  setItem(key: string, value: string) {
    this.store.set(key, String(value));
  }
}

if (typeof window !== 'undefined') {
  const memoryStorage = new MemoryStorage();
  Object.defineProperty(window, 'localStorage', { value: memoryStorage, configurable: true });
  Object.defineProperty(globalThis, 'localStorage', { value: memoryStorage, configurable: true });
}
