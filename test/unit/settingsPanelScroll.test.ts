import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

describe('settings panel scroll layout', () => {
  it('uses a flex panel with scrollable body and sticky footer regions', () => {
    const css = readFileSync(resolve(import.meta.dirname, '../../src/styles.css'), 'utf8');
    expect(css).toContain('.settings-panel-body');
    expect(css).toContain('.settings-panel-footer');
    expect(css).toMatch(/\.settings-panel-body\s*\{[^}]*overflow-y:\s*auto/);
    expect(css).toMatch(/\.settings-panel\s*\{[^}]*flex-direction:\s*column/);
    expect(css).toMatch(/-webkit-overflow-scrolling:\s*touch/);
  });
});
