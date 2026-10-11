import { describe, expect, it } from 'vitest';

import { chipClass, kindChipClass } from '../../src/chipVariants';

describe('chipVariants', () => {
  it('maps semantic variants to css classes', () => {
    expect(chipClass('needs-input')).toBe('chip chip-needs-input');
    expect(chipClass(undefined)).toBe('chip chip-neutral');
  });

  it('styles issue and pr kind chips', () => {
    expect(kindChipClass('issue', false)).toBe('chip chip-issue');
    expect(kindChipClass('pr', false)).toBe('chip chip-pr');
    expect(kindChipClass('pr', true)).toBe('chip chip-draft');
  });
});
