import { describe, expect, it } from 'vitest';

import {
  setCargoLockPackageVersion,
  setCargoTomlVersion,
} from '../../scripts/set-cargo-version.mjs';

describe('set-cargo-version', () => {
  it('updates the package version in Cargo.toml', () => {
    const input = '[package]\nname = "agent-dashboard"\nversion = "0.1.0"\n';
    expect(setCargoTomlVersion(input, '1.2.3')).toContain('version = "1.2.3"');
  });

  it('updates the package version in Cargo.lock', () => {
    const input = '[[package]]\nname = "agent-dashboard"\nversion = "0.1.0"\n';
    expect(setCargoLockPackageVersion(input, '1.2.3')).toContain('version = "1.2.3"');
  });
});
