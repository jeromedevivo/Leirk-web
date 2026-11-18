//! Address-bar calculator (Vivaldi): `12*3,5` shows 42 before you press Enter.

/// Evaluates + - * / % ^ and parentheses. `None` unless the input is a real expression (an operator and a number).
pub fn calculate(input: &str) -> Option<f64> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_with_precedence() {
        assert_eq!(calculate("2+3*4"), Some(14.0));
        assert_eq!(calculate("(2+3)*4"), Some(20.0));
        assert_eq!(calculate("2^10"), Some(1024.0));
        assert_eq!(calculate("-3 + 5"), Some(2.0));
        assert_eq!(calculate("10 % 3"), Some(1.0));
        assert_eq!(calculate("12*3,5"), Some(42.0), "French decimal comma");
        assert_eq!(calculate("1/4"), Some(0.25));
    }

    #[test]
    fn rejects_non_expressions() {
        assert_eq!(calculate("42"), None);
        assert_eq!(calculate("rust lang"), None);
        assert_eq!(calculate("2+"), None);
        assert_eq!(calculate("1/0"), None);
        assert_eq!(calculate("example.com"), None);
        assert_eq!(calculate("(1+2"), None);
    }
}
