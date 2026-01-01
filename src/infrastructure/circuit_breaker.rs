use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

/// Circuit breaker state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,   // Normal operation - requests pass through
    Open,     // Failure threshold breached - reject all requests
    HalfOpen, // Testing if service recovered - allow limited requests
}

/// Circuit breaker for protecting against cascading failures
pub struct CircuitBreaker {
    state: Arc<RwLock<CircuitBreakerState>>,
    failure_threshold: usize,
    success_threshold: usize,
    timeout: Duration,
    name: String,
}

struct CircuitBreakerState {
    state: CircuitState,
    failure_count: usize,
    success_count: usize,
    last_failure_time: Option<Instant>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker
    ///
    /// # Arguments
    /// * `name` - Identifier for logging
    /// * `failure_threshold` - Number of consecutive failures before opening circuit
    /// * `success_threshold` - Number of consecutive successes in HalfOpen to close circuit
    /// * `timeout` - Duration to wait before transitioning from Open to HalfOpen
    pub fn new(
        name: impl Into<String>,
        failure_threshold: usize,
        success_threshold: usize,
        timeout: Duration,
    ) -> Self {
        Self {
            state: Arc::new(RwLock::new(CircuitBreakerState {
                state: CircuitState::Closed,
                failure_count: 0,
                success_count: 0,
                last_failure_time: None,
            })),
            failure_threshold,
            success_threshold,
            timeout,
            name: name.into(),
        }
    }

    /// Execute a function with circuit breaker protection
    pub async fn call<F, T, E>(&self, f: F) -> Result<T, CircuitBreakerError<E>>
    where
        F: std::future::Future<Output = Result<T, E>>,
    {
        // Check if circuit is open
        {
            let mut state = self.state.write().await;

            if state.state == CircuitState::Open {
                // Check if timeout elapsed to transition to HalfOpen
                if let Some(last_failure) = state.last_failure_time {
                    if last_failure.elapsed() > self.timeout {
                        info!(
                            "CircuitBreaker [{}]: Transitioning Open -> HalfOpen (timeout elapsed)",
                            self.name
                        );
                        state.state = CircuitState::HalfOpen;
                        state.success_count = 0;
                    } else {
                        return Err(CircuitBreakerError::Open(format!(
                            "Circuit breaker [{}] is open. Retry in {:?}",
                            self.name,
                            self.timeout - last_failure.elapsed()
                        )));
                    }
                }
            }
        }

        // Execute function
        match f.await {
            Ok(result) => {
                self.on_success().await;
                Ok(result)
            }
            Err(e) => {
                self.on_failure().await;
                Err(CircuitBreakerError::Inner(e))
            }
        }
    }

    /// Record a successful call
    async fn on_success(&self) {
        let mut state = self.state.write().await;

        match state.state {
            CircuitState::HalfOpen => {
                state.success_count += 1;
                if state.success_count >= self.success_threshold {
                    info!(
                        "CircuitBreaker [{}]: Transitioning HalfOpen -> Closed ({} successes)",
                        self.name, state.success_count
                    );
                    state.state = CircuitState::Closed;
                    state.failure_count = 0;
                    state.success_count = 0;
                }
            }
            CircuitState::Closed => {
                // Reset failure count on success
                state.failure_count = 0;
            }
            CircuitState::Open => {
                // Should not happen, but reset if it does
                warn!(
                    "CircuitBreaker [{}]: Success recorded in Open state (unexpected)",
                    self.name
                );
            }
        }
    }

    /// Record a failed call
    async fn on_failure(&self) {
        let mut state = self.state.write().await;

        state.failure_count += 1;
        state.last_failure_time = Some(Instant::now());

        match state.state {
            CircuitState::Closed => {
                if state.failure_count >= self.failure_threshold {
                    error!(
                        "CircuitBreaker [{}]: Transitioning Closed -> Open ({} failures)",
                        self.name, state.failure_count
                    );
                    state.state = CircuitState::Open;
                }
            }
            CircuitState::HalfOpen => {
                // Any failure in HalfOpen immediately reopens circuit
                warn!(
                    "CircuitBreaker [{}]: Transitioning HalfOpen -> Open (failure during recovery)",
                    self.name
                );
                state.state = CircuitState::Open;
                state.success_count = 0;
            }
            CircuitState::Open => {
                // Already open, just increment counter
            }
        }
    }

    /// Get current circuit state
    pub async fn state(&self) -> CircuitState {
        self.state.read().await.state
    }
}

/// Error type for circuit breaker
#[derive(Debug, thiserror::Error)]
pub enum CircuitBreakerError<E> {
    #[error("Circuit breaker is open: {0}")]
    Open(String),

    #[error(transparent)]
    Inner(E),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_circuit_opens_after_failures() {
        let cb = CircuitBreaker::new("test", 3, 2, Duration::from_secs(1));

        // Simulate 3 failures
        for _ in 0..3 {
            let result = cb.call(async { Err::<(), &str>("error") }).await;
            assert!(result.is_err());
        }

        // Circuit should be open now
        assert_eq!(cb.state().await, CircuitState::Open);

        // Next call should fail fast
        let result = cb.call(async { Ok::<(), &str>(()) }).await;
        assert!(matches!(result, Err(CircuitBreakerError::Open(_))));
    }

    #[tokio::test]
    async fn test_circuit_recovers_after_timeout() {
        let cb = CircuitBreaker::new("test", 2, 2, Duration::from_millis(100));

        // Open the circuit
        for _ in 0..2 {
            let _ = cb.call(async { Err::<(), &str>("error") }).await;
        }

        assert_eq!(cb.state().await, CircuitState::Open);

        // Wait for timeout
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Should transition to HalfOpen and allow request
        let result = cb.call(async { Ok::<(), &str>(()) }).await;
        assert!(result.is_ok());

        // One more success to fully close
        let result = cb.call(async { Ok::<(), &str>(()) }).await;
        assert!(result.is_ok());

        assert_eq!(cb.state().await, CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_halfopen_reopens_on_failure() {
        let cb = CircuitBreaker::new("test", 2, 2, Duration::from_millis(100));

        // Open the circuit
        for _ in 0..2 {
            let _ = cb.call(async { Err::<(), &str>("error") }).await;
        }

        // Wait for timeout to transition to HalfOpen
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Failure in HalfOpen should reopen
        let _ = cb.call(async { Err::<(), &str>("error") }).await;

        assert_eq!(cb.state().await, CircuitState::Open);
    }
}
