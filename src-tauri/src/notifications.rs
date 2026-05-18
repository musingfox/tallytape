//! Notification helper for the Tauri backend.
//!
//! Wraps `tauri-plugin-notification` with a `NotificationSink` trait so that
//! unit tests can inject a mock without a live Tauri runtime.

use anyhow::Context;
use tauri::AppHandle;
use tauri_plugin_notification::{NotificationExt, PermissionState};

// ---------------------------------------------------------------------------
// Private trait — enables test injection
// ---------------------------------------------------------------------------

trait NotificationSink {
    fn permission_state(&self) -> anyhow::Result<PermissionState>;
    fn show(&self, title: &str, body: &str) -> anyhow::Result<()>;
}

// ---------------------------------------------------------------------------
// Production implementation backed by a real AppHandle
// ---------------------------------------------------------------------------

struct AppHandleSink<'a>(&'a AppHandle);

impl NotificationSink for AppHandleSink<'_> {
    fn permission_state(&self) -> anyhow::Result<PermissionState> {
        self.0
            .notification()
            .permission_state()
            .context("notify: permission_state failed")
    }

    fn show(&self, title: &str, body: &str) -> anyhow::Result<()> {
        self.0
            .notification()
            .builder()
            .title(title)
            .body(body)
            .show()
            .context("notify: show failed")
    }
}

// ---------------------------------------------------------------------------
// Shared logic
// ---------------------------------------------------------------------------

fn notify_with_sink(sink: &dyn NotificationSink, title: &str, body: &str) -> anyhow::Result<()> {
    let state = sink.permission_state()?;
    if state == PermissionState::Granted {
        sink.show(title, body)?;
    }
    // Silently skip when permission is Denied / Prompt / PromptWithRationale
    Ok(())
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Send a system notification.
///
/// On `PermissionState::Granted` the notification is shown. On any other state
/// the call is silently skipped (no error). Only a genuine plugin failure
/// (e.g., underlying OS error) bubbles as `Err`.
pub fn notify(app: &AppHandle, title: &str, body: &str) -> anyhow::Result<()> {
    notify_with_sink(&AppHandleSink(app), title, body)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct MockSink {
        permission: PermissionState,
        show_called: Cell<u32>,
        show_title: std::cell::RefCell<Option<String>>,
        show_body: std::cell::RefCell<Option<String>>,
        show_error: Option<&'static str>,
    }

    impl MockSink {
        fn granted() -> Self {
            Self {
                permission: PermissionState::Granted,
                show_called: Cell::new(0),
                show_title: std::cell::RefCell::new(None),
                show_body: std::cell::RefCell::new(None),
                show_error: None,
            }
        }

        fn denied() -> Self {
            Self {
                permission: PermissionState::Denied,
                ..Self::granted()
            }
        }

        fn granted_with_show_error(msg: &'static str) -> Self {
            Self {
                show_error: Some(msg),
                ..Self::granted()
            }
        }
    }

    impl NotificationSink for MockSink {
        fn permission_state(&self) -> anyhow::Result<PermissionState> {
            Ok(self.permission)
        }

        fn show(&self, title: &str, body: &str) -> anyhow::Result<()> {
            if let Some(msg) = self.show_error {
                return Err(anyhow::anyhow!("{msg}"));
            }
            self.show_called.set(self.show_called.get() + 1);
            *self.show_title.borrow_mut() = Some(title.to_string());
            *self.show_body.borrow_mut() = Some(body.to_string());
            Ok(())
        }
    }

    // C4: permission Granted → builder invoked exactly once with correct title/body
    #[test]
    fn granted_invokes_sink_once_with_title_and_body() {
        let sink = MockSink::granted();
        notify_with_sink(&sink, "Title", "Body").unwrap();
        assert_eq!(sink.show_called.get(), 1);
        assert_eq!(sink.show_title.borrow().as_deref(), Some("Title"));
        assert_eq!(sink.show_body.borrow().as_deref(), Some("Body"));
    }

    // C4: permission Denied → sink.show never called, returns Ok(())
    #[test]
    fn denied_does_not_invoke_sink() {
        let sink = MockSink::denied();
        let result = notify_with_sink(&sink, "Title", "Body");
        assert!(result.is_ok());
        assert_eq!(sink.show_called.get(), 0);
    }

    // C4: plugin builder error → bubbles as Err
    #[test]
    fn show_error_bubbles_as_err() {
        let sink = MockSink::granted_with_show_error("OS error");
        let result = notify_with_sink(&sink, "T", "B");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("OS error"));
    }
}
