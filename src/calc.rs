//! Address-bar calculator (Vivaldi): `12*3,5` shows 42 before you press Enter.

/// Evaluates + - * / % ^ and parentheses. `None` unless the input is a real expression (an operator and a number).
pub fn calculate(input: &str) -> Option<f64> {
    let s: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).map(|b| if b == b',' { b'.' } else { b }).collect();
    let ops = b"+-*/%^";
    if s.is_empty() || !s.iter().all(|b| b.is_ascii_digit() || ops.contains(b) || b"().".contains(b)) {
        return None;
    }
    if !s[1..].iter().any(|b| ops.contains(b)) {
        return None; // a bare number is not a calculation
    }
    let mut p = Parser { s: &s, i: 0 };
    let v = p.expr()?;
    (p.i == s.len() && v.is_finite()).then_some(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn eat(&mut self, c: u8) -> bool {
        let hit = self.s.get(self.i) == Some(&c);
        self.i += hit as usize;
        hit
    }

    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        loop {
            if self.eat(b'+') {
                v += self.term()?;
            } else if self.eat(b'-') {
                v -= self.term()?;
            } else {
                return Some(v);
            }
        }
    }

    fn term(&mut self) -> Option<f64> {
        let mut v = self.power()?;
        loop {
            if self.eat(b'*') {
                v *= self.power()?;
            } else if self.eat(b'/') {
                v /= self.power()?;
            } else if self.eat(b'%') {
                v %= self.power()?;
            } else {
                return Some(v);
            }
        }
    }

    fn power(&mut self) -> Option<f64> {
        let base = self.unary()?;
        if self.eat(b'^') { Some(base.powf(self.power()?)) } else { Some(base) }
    }

    fn unary(&mut self) -> Option<f64> {
        if self.eat(b'-') {
            return Some(-self.unary()?);
        }
        if self.eat(b'+') {
            return self.unary();
        }
        if self.eat(b'(') {
            let v = self.expr()?;
            return self.eat(b')').then_some(v);
        }
        let start = self.i;
        while self.s.get(self.i).is_some_and(|b| b.is_ascii_digit() || *b == b'.') {
            self.i += 1;
        }
        std::str::from_utf8(&self.s[start..self.i]).ok()?.parse().ok()
    }
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
