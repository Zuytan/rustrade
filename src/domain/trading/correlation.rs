use uuid::Uuid;

/// Generates a new correlation ID for tracing order flows.
pub fn generate_correlation_id() -> String {
    Uuid::new_v4().to_string()
}
