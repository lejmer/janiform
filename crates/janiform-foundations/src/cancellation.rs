//! Cooperative cancellation primitives shared by native, CLI, and PyO3 code.

use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// Polling policy for interruptible native work that cannot block on a Rust
/// completion handle.
///
/// The interval is explicitly set to fifty milliseconds so terminal and Python
/// interrupt handling observes cancellation within one interactive response
/// slice while still sleeping between polls. It is not used by numerical
/// algorithms or model evolution.
pub const NATIVE_INTERRUPT_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Result of recording a user cancellation request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancellationRequest {
    /// The request transitioned the operation into the cancelled state.
    Cancelled,
    /// Cancellation was already requested, so the caller may apply its
    /// frontend-specific force policy.
    ForceRequested,
}

/// Error returned when an operation observes cooperative cancellation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CancellationError;

impl fmt::Display for CancellationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("operation cancelled")
    }
}

impl Error for CancellationError {}

/// Writable endpoint for requesting cancellation of a running operation.
///
/// Cloning or copying this source with [`CancellationSource::shared_handle`]
/// shares the same atomic cancellation state. It does not create an
/// independent cancellation source.
#[derive(Clone, Debug)]
pub struct CancellationSource {
    state: Arc<CancellationState>,
}

impl CancellationSource {
    /// Create a source with no cancellation request recorded.
    pub fn new() -> Self {
        Self {
            state: Arc::new(CancellationState::new()),
        }
    }

    /// Return another writable handle to the same atomic cancellation state.
    pub fn shared_handle(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
        }
    }

    /// Return a cloneable read token that shares this source's atomic state.
    pub fn token(&self) -> CancellationToken {
        CancellationToken {
            state: Arc::clone(&self.state),
        }
    }

    /// Record a cancellation request.
    pub fn cancel(&self) {
        self.state.cancelled.store(true, Ordering::SeqCst);
    }

    /// Record a cancellation request and identify repeated requests as force
    /// requests for terminal frontends.
    pub fn request_cancellation(&self) -> CancellationRequest {
        if self.state.cancelled.swap(true, Ordering::SeqCst) {
            self.state.force_requested.store(true, Ordering::SeqCst);
            return CancellationRequest::ForceRequested;
        }

        CancellationRequest::Cancelled
    }

    /// Return whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::SeqCst)
    }

    /// Return whether a repeated cancellation request has asked for force.
    pub fn is_force_requested(&self) -> bool {
        self.state.force_requested.load(Ordering::SeqCst)
    }
}

impl Default for CancellationSource {
    fn default() -> Self {
        Self::new()
    }
}

/// Cloneable read handle for observing cooperative cancellation.
///
/// Cloning or copying this handle with [`CancellationToken::shared_handle`]
/// shares the same atomic cancellation state. It does not create an
/// independent cancellation request.
#[derive(Clone, Debug)]
pub struct CancellationToken {
    state: Arc<CancellationState>,
}

impl CancellationToken {
    /// Return another handle to the same atomic cancellation state.
    pub fn shared_handle(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
        }
    }

    /// Return a token that has no external source and therefore never cancels.
    pub fn uncancelled() -> Self {
        CancellationSource::new().token()
    }

    /// Return whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::SeqCst)
    }

    /// Return whether a repeated cancellation request has asked for force.
    pub fn is_force_requested(&self) -> bool {
        self.state.force_requested.load(Ordering::SeqCst)
    }

    /// Return `CancellationError` if cancellation has been requested.
    pub fn check_cancelled(&self) -> Result<(), CancellationError> {
        if self.is_cancelled() {
            return Err(CancellationError);
        }

        Ok(())
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::uncancelled()
    }
}

#[derive(Debug)]
struct CancellationState {
    cancelled: AtomicBool,
    force_requested: AtomicBool,
}

impl CancellationState {
    fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            force_requested: AtomicBool::new(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CancellationError;
    use super::CancellationRequest;
    use super::CancellationSource;
    use super::CancellationToken;

    #[test]
    fn token_starts_uncancelled() {
        let token = CancellationToken::uncancelled();

        assert_eq!(Ok(()), token.check_cancelled());
        assert_eq!(false, token.is_cancelled());
        assert_eq!(false, token.is_force_requested());
    }

    #[test]
    fn cancellation_is_observed_by_existing_token() {
        let source = CancellationSource::new();
        let token = source.token();

        source.cancel();

        assert_eq!(Err(CancellationError), token.check_cancelled());
        assert_eq!(true, source.is_cancelled());
        assert_eq!(true, token.is_cancelled());
        assert_eq!(false, source.is_force_requested());
        assert_eq!(false, token.is_force_requested());
    }

    #[test]
    fn cancellation_request_is_idempotent_and_tracks_force_request() {
        let source = CancellationSource::new();
        let token = source.token();

        assert_eq!(
            CancellationRequest::Cancelled,
            source.request_cancellation()
        );
        assert_eq!(
            CancellationRequest::ForceRequested,
            source.request_cancellation()
        );

        assert_eq!(true, source.is_cancelled());
        assert_eq!(true, token.is_cancelled());
        assert_eq!(true, source.is_force_requested());
        assert_eq!(true, token.is_force_requested());
    }
}
