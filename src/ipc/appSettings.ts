import { invoke } from '@tauri-apps/api/core';

/**
 * Read an app setting by key.
 *
 * Rust command source: src-tauri/src/lib.rs (get_app_setting).
 * Rejected promises carry an AppError-shaped payload ({ message: string }).
 */
export async function getAppSetting(key: string): Promise<string | null> {
  return invoke<string | null>('get_app_setting', { key });
}

/**
 * Write an app setting by key. Last-write-wins.
 *
 * Rust command source: src-tauri/src/lib.rs (set_app_setting).
 * Rejected promises carry an AppError-shaped payload ({ message: string }).
 */
export async function setAppSetting(key: string, value: string): Promise<void> {
  return invoke<void>('set_app_setting', { key, value });
}
