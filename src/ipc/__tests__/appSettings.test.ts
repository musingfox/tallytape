import { beforeEach, describe, expect, it, vi, type Mock } from 'vitest';
import { getAppSetting, setAppSetting } from '../appSettings';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

import { invoke } from '@tauri-apps/api/core';

const mockedInvoke = invoke as Mock;

beforeEach(() => {
  vi.clearAllMocks();
});

describe('getAppSetting', () => {
  it('calls get_app_setting with the key and resolves null for an absent key', async () => {
    mockedInvoke.mockResolvedValue(null);

    await expect(getAppSetting('absent')).resolves.toBeNull();
    expect(mockedInvoke).toHaveBeenCalledWith('get_app_setting', { key: 'absent' });
  });

  it('resolves the stored string when a value is present', async () => {
    mockedInvoke.mockResolvedValue('bar');

    await expect(getAppSetting('foo')).resolves.toBe('bar');
    expect(mockedInvoke).toHaveBeenCalledWith('get_app_setting', { key: 'foo' });
  });
});

describe('setAppSetting', () => {
  it('calls invoke set_app_setting with correct key and value', async () => {
    mockedInvoke.mockResolvedValue(undefined);

    await setAppSetting('flag', 'true');

    expect(mockedInvoke).toHaveBeenCalledWith('set_app_setting', { key: 'flag', value: 'true' });
  });

  it('resolves to void', async () => {
    mockedInvoke.mockResolvedValue(undefined);

    await expect(setAppSetting('x', 'y')).resolves.toBeUndefined();
  });
});
