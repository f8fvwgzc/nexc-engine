//! Field-level validation errors (rendered as the `errors` member of a
//! problem+json response) and small reusable rules.

use std::collections::BTreeMap;

use super::AppError;

/// Validation messages keyed by field name.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FieldErrors(BTreeMap<String, Vec<String>>);

impl FieldErrors {
    /// Records `message` for `field`.
    pub fn add(&mut self, field: &str, message: impl Into<String>) {
        self.0
            .entry(field.to_owned())
            .or_default()
            .push(message.into());
    }

    /// True when no error was recorded.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The recorded messages.
    pub fn as_map(&self) -> &BTreeMap<String, Vec<String>> {
        &self.0
    }

    /// `Ok(())` when empty, otherwise a 422 [`AppError::Validation`].
    pub fn into_result(self) -> Result<(), AppError> {
        if self.is_empty() {
            Ok(())
        } else {
            Err(AppError::Validation(self))
        }
    }
}

/// Types whose instances can check their own invariants.
pub trait Validate {
    /// Adds every violated rule to `errors`.
    fn validate(&self, errors: &mut FieldErrors);
}

/// Requires `value` to be non-blank and at most `max` characters.
pub fn check_text(errors: &mut FieldErrors, field: &str, value: &str, max: usize) {
    if value.trim().is_empty() {
        errors.add(field, "must not be blank");
    }
    check_max_len(errors, field, value, max);
}

/// Requires `value` to be at most `max` characters (may be empty).
pub fn check_max_len(errors: &mut FieldErrors, field: &str, value: &str, max: usize) {
    if value.chars().count() > max {
        errors.add(field, format!("must be at most {max} characters"));
    }
}

/// Requires a finite coordinate within a generous canvas range.
pub fn check_coordinate(errors: &mut FieldErrors, field: &str, value: f64) {
    if !value.is_finite() || value.abs() > 1_000_000.0 {
        errors.add(field, "must be a finite number within ±1e6");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_errors() {
        let mut e = FieldErrors::default();
        check_text(&mut e, "title", "  ", 10);
        check_max_len(&mut e, "content", "abcdef", 3);
        check_coordinate(&mut e, "x", f64::NAN);
        assert_eq!(e.as_map().len(), 3);
        assert!(matches!(e.into_result(), Err(AppError::Validation(_))));
        assert!(FieldErrors::default().into_result().is_ok());
    }
}
