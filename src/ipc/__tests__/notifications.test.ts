import { beforeEach, describe, expect, it, vi, type Mock } from 'vitest';
import {
  isNotificationPermissionGranted,
  notify,
  requestNotificationPermission,
} from '../notifications';

vi.mock('@tauri-apps/plugin-notification', () => ({
  isPermissionGranted: vi.fn(),
  requestPermission: vi.fn(),
  sendNotification: vi.fn(),
}));

import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from '@tauri-apps/plugin-notification';

const mockedIsPermissionGranted = isPermissionGranted as Mock;
const mockedRequestPermission = requestPermission as Mock;
const mockedSendNotification = sendNotification as Mock;

beforeEach(() => {
  vi.clearAllMocks();
});

describe('isNotificationPermissionGranted', () => {
  it('delegates to isPermissionGranted and returns boolean', async () => {
    mockedIsPermissionGranted.mockResolvedValue(true);
    await expect(isNotificationPermissionGranted()).resolves.toBe(true);
  });
});

describe('requestNotificationPermission', () => {
  it('delegates to requestPermission and returns the permission string', async () => {
    mockedRequestPermission.mockResolvedValue('granted');
    await expect(requestNotificationPermission()).resolves.toBe('granted');
  });
});

describe('notify', () => {
  it('calls sendNotification with title and body when permission is granted', async () => {
    mockedIsPermissionGranted.mockResolvedValue(true);

    await notify('T', 'B');

    expect(mockedSendNotification).toHaveBeenCalledOnce();
    expect(mockedSendNotification).toHaveBeenCalledWith({ title: 'T', body: 'B' });
  });

  it('does not call sendNotification when permission is not granted', async () => {
    mockedIsPermissionGranted.mockResolvedValue(false);

    await expect(notify('T', 'B')).resolves.toBeUndefined();
    expect(mockedSendNotification).not.toHaveBeenCalled();
  });
});
