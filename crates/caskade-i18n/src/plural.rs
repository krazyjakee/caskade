//! Plural rules: the `Plural-Forms` header's C expression of `n`, which
//! picks the `msgstr[i]` for a count.

use std::fmt;

/// A parsed `Plural-Forms` header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluralForms {
    /// How many forms the language has.
    pub count: usize,
    rule: Expr,
}

/// Why a `Plural-Forms` header couldn't be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluralError(pub String);

impl fmt::Display for PluralError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Plural-Forms: {}", self.0)
    }
}

impl std::error::Error for PluralError {}

impl Default for PluralForms {
    /// English and the gettext default: one, and everything else.
    fn default() -> Self {
        Self::parse("nplurals=2; plural=(n != 1);").expect("valid rule")
    }
}

impl PluralForms {
    /// Reads `nplurals=N; plural=EXPR;`.
    pub fn parse(header: &str) -> Result<Self, PluralError> {
        let mut count = None;
        let mut rule = None;
        for part in header.split(';') {
            let Some((key, value)) = part.split_once('=') else {
                continue;
            };
            match key.trim() {
                "nplurals" => {
                    count = Some(
                        value
                            .trim()
                            .parse::<usize>()
                            .ok()
                            .filter(|count| (1..=6).contains(count))
                            .ok_or_else(|| PluralError(format!("bad nplurals {value:?}")))?,
                    );
                }
                "plural" => rule = Some(Expr::parse(value)?),
                _ => {}
            }
        }
        match (count, rule) {
            (Some(count), Some(rule)) => Ok(Self { count, rule }),
            _ => Err(PluralError("needs nplurals and plural".to_owned())),
        }
    }

    /// The form for `n`, below [`Self::count`].
    pub fn index(&self, n: u64) -> usize {
        usize::try_from(self.rule.eval(n))
            .unwrap_or(usize::MAX)
            .min(self.count - 1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Expr {
    N,
    Number(u64),
    Not(Box<Expr>),
    Binary(Op, Box<Expr>, Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl Op {
    /// Binding strength, as in C.
    fn precedence(self) -> u8 {
        match self {
            Op::Or => 1,
            Op::And => 2,
            Op::Eq | Op::Ne => 3,
            Op::Lt | Op::Le | Op::Gt | Op::Ge => 4,
            Op::Add | Op::Sub => 5,
            Op::Mul | Op::Div | Op::Rem => 6,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
    N,
    Number(u64),
    Op(Op),
    Not,
    Question,
    Colon,
    Open,
    Close,
}

impl Expr {
    fn parse(source: &str) -> Result<Self, PluralError> {
        let tokens = tokenize(source)?;
        let mut parser = ExprParser {
            tokens: &tokens,
            at: 0,
        };
        let expr = parser.ternary()?;
        if parser.at != tokens.len() {
            return Err(PluralError(format!("unexpected text in {source:?}")));
        }
        Ok(expr)
    }

    fn eval(&self, n: u64) -> u64 {
        match self {
            Expr::N => n,
            Expr::Number(value) => *value,
            Expr::Not(inner) => u64::from(inner.eval(n) == 0),
            Expr::Ternary(condition, then, otherwise) => {
                if condition.eval(n) != 0 {
                    then.eval(n)
                } else {
                    otherwise.eval(n)
                }
            }
            Expr::Binary(op, left, right) => {
                let (a, b) = (left.eval(n), right.eval(n));
                match op {
                    Op::Or => u64::from(a != 0 || b != 0),
                    Op::And => u64::from(a != 0 && b != 0),
                    Op::Eq => u64::from(a == b),
                    Op::Ne => u64::from(a != b),
                    Op::Lt => u64::from(a < b),
                    Op::Le => u64::from(a <= b),
                    Op::Gt => u64::from(a > b),
                    Op::Ge => u64::from(a >= b),
                    Op::Add => a.wrapping_add(b),
                    Op::Sub => a.wrapping_sub(b),
                    Op::Mul => a.wrapping_mul(b),
                    Op::Div => a.checked_div(b).unwrap_or(0),
                    Op::Rem => a.checked_rem(b).unwrap_or(0),
                }
            }
        }
    }
}

fn tokenize(source: &str) -> Result<Vec<Token>, PluralError> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let next = bytes.get(at + 1).copied();
        let (token, width) = match bytes[at] {
            b' ' | b'\t' | b'\n' | b'\r' => {
                at += 1;
                continue;
            }
            b'n' => (Token::N, 1),
            b'0'..=b'9' => {
                let end = bytes[at..]
                    .iter()
                    .position(|b| !b.is_ascii_digit())
                    .map_or(bytes.len(), |offset| at + offset);
                let value = source[at..end]
                    .parse()
                    .map_err(|_| PluralError(format!("number too large in {source:?}")))?;
                (Token::Number(value), end - at)
            }
            b'|' if next == Some(b'|') => (Token::Op(Op::Or), 2),
            b'&' if next == Some(b'&') => (Token::Op(Op::And), 2),
            b'=' if next == Some(b'=') => (Token::Op(Op::Eq), 2),
            b'!' if next == Some(b'=') => (Token::Op(Op::Ne), 2),
            b'<' if next == Some(b'=') => (Token::Op(Op::Le), 2),
            b'>' if next == Some(b'=') => (Token::Op(Op::Ge), 2),
            b'<' => (Token::Op(Op::Lt), 1),
            b'>' => (Token::Op(Op::Gt), 1),
            b'+' => (Token::Op(Op::Add), 1),
            b'-' => (Token::Op(Op::Sub), 1),
            b'*' => (Token::Op(Op::Mul), 1),
            b'/' => (Token::Op(Op::Div), 1),
            b'%' => (Token::Op(Op::Rem), 1),
            b'!' => (Token::Not, 1),
            b'?' => (Token::Question, 1),
            b':' => (Token::Colon, 1),
            b'(' => (Token::Open, 1),
            b')' => (Token::Close, 1),
            _ => {
                return Err(PluralError(format!(
                    "unexpected character at {at} in {source:?}"
                )));
            }
        };
        tokens.push(token);
        at += width;
    }
    Ok(tokens)
}

struct ExprParser<'a> {
    tokens: &'a [Token],
    at: usize,
}

impl ExprParser<'_> {
    fn peek(&self) -> Option<Token> {
        self.tokens.get(self.at).copied()
    }

    fn expect(&mut self, token: Token) -> Result<(), PluralError> {
        if self.peek() == Some(token) {
            self.at += 1;
            Ok(())
        } else {
            Err(PluralError(format!("expected {token:?}")))
        }
    }

    /// `a ? b : c`, right-associative, below every binary operator.
    fn ternary(&mut self) -> Result<Expr, PluralError> {
        let condition = self.binary(1)?;
        if self.peek() != Some(Token::Question) {
            return Ok(condition);
        }
        self.at += 1;
        let then = self.ternary()?;
        self.expect(Token::Colon)?;
        let otherwise = self.ternary()?;
        Ok(Expr::Ternary(
            Box::new(condition),
            Box::new(then),
            Box::new(otherwise),
        ))
    }

    /// Left-associative binary operators binding at least `min`.
    fn binary(&mut self, min: u8) -> Result<Expr, PluralError> {
        let mut left = self.unary()?;
        while let Some(Token::Op(op)) = self.peek() {
            if op.precedence() < min {
                break;
            }
            self.at += 1;
            let right = self.binary(op.precedence() + 1)?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, PluralError> {
        match self.peek() {
            Some(Token::Not) => {
                self.at += 1;
                Ok(Expr::Not(Box::new(self.unary()?)))
            }
            Some(Token::N) => {
                self.at += 1;
                Ok(Expr::N)
            }
            Some(Token::Number(value)) => {
                self.at += 1;
                Ok(Expr::Number(value))
            }
            Some(Token::Open) => {
                self.at += 1;
                let inner = self.ternary()?;
                self.expect(Token::Close)?;
                Ok(inner)
            }
            other => Err(PluralError(format!("unexpected {other:?}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forms(header: &str, counts: &[u64]) -> Vec<usize> {
        let rule = PluralForms::parse(header).unwrap();
        counts.iter().map(|&n| rule.index(n)).collect()
    }

    #[test]
    fn english_and_swedish_have_one_and_other() {
        let header = "nplurals=2; plural=(n != 1);";
        assert_eq!(forms(header, &[0, 1, 2, 21]), [1, 0, 1, 1]);
        assert_eq!(PluralForms::default().index(1), 0);
    }

    #[test]
    fn french_counts_zero_as_one() {
        assert_eq!(forms("nplurals=2; plural=(n > 1);", &[0, 1, 2]), [0, 0, 1]);
    }

    #[test]
    fn polish_has_three_forms() {
        let header = "nplurals=3; plural=(n==1 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);";
        assert_eq!(
            forms(header, &[1, 2, 4, 5, 12, 21, 22, 25, 112, 122]),
            [0, 1, 1, 2, 2, 2, 1, 2, 2, 1]
        );
    }

    #[test]
    fn arabic_has_six_forms() {
        let header = "nplurals=6; plural=(n==0 ? 0 : n==1 ? 1 : n==2 ? 2 : n%100>=3 && n%100<=10 ? 3 : n%100>=11 ? 4 : 5);";
        assert_eq!(
            forms(header, &[0, 1, 2, 3, 11, 100, 102]),
            [0, 1, 2, 3, 4, 5, 5]
        );
    }

    #[test]
    fn single_form_languages_and_out_of_range_rules() {
        assert_eq!(forms("nplurals=1; plural=0;", &[0, 1, 5]), [0, 0, 0]);
        // A rule that picks a form the language doesn't have gets the last.
        assert_eq!(forms("nplurals=2; plural=n;", &[7]), [1]);
        assert_eq!(forms("nplurals=2; plural=!(n / 0);", &[3]), [1]);
    }

    #[test]
    fn rejects_broken_rules() {
        for header in [
            "plural=(n != 1);",
            "nplurals=2;",
            "nplurals=0; plural=0;",
            "nplurals=2; plural=(n != 1;",
            "nplurals=2; plural=n $ 1;",
            "nplurals=2; plural=n 1;",
            "nplurals=2; plural=n ? 1;",
        ] {
            assert!(PluralForms::parse(header).is_err(), "{header}");
        }
    }
}
