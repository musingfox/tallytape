import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from '@tauri-apps/plugin-notification';

/**
 * Check whether the user has already granted notification permission.
 */
export async function isNotificationPermissionGranted(): Promise<boolean> {
  return isPermissionGranted();
}

/**
 * Request notification permission from the user.
 *
 * Returns the permission decision as a standard NotificationPermission string
 * ('granted', 'denied', or 'default').
 */
export async function requestNotificationPermission(): Promise<NotificationPermission> {
  return requestPermission();
}

/**
 * Send a system notification if permission is currently granted.
 *
 * Silently no-ops when permission is not granted. Rethrows genuine plugin
 * errors but never rejects on a simple denial.
 */
export async function notify(title: string, body: string): Promise<void> {
  const granted = await isPermissionGranted();
  if (granted) {
    sendNotification({ title, body });
  }
}
