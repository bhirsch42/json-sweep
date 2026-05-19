use nom::{
    IResult, Parser,
    bytes::complete::take_while,
    character::complete::{char as nom_char, satisfy},
    combinator::{all_consuming, recognize, verify},
    multi::separated_list1,
    sequence::pair,
};

fn ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn ident_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

fn ident(input: &str) -> IResult<&str, &str> {
    recognize(pair(
        satisfy(ident_start),
        take_while(ident_continue),
    ))
    .parse(input)
}

/// Parse a dot-separated path of identifiers.
///
/// Grammar: `ident ("." ident)*` where `ident = [A-Za-z_][A-Za-z0-9_-]*`.
/// Consumes the entire input — anything left over is an error.
pub fn parse_path(input: &str) -> IResult<&str, Vec<String>> {
    all_consuming(verify(
        separated_list1(nom_char('.'), ident),
        |segs: &Vec<&str>| !segs.is_empty(),
    ))
    .parse(input)
    .map(|(rest, segs)| (rest, segs.into_iter().map(String::from).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(input: &str, expected: &[&str]) {
        let (rest, segs) = parse_path(input).expect("parse should succeed");
        assert_eq!(rest, "");
        assert_eq!(
            segs,
            expected.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            "input: {input:?}"
        );
    }

    fn err(input: &str) {
        assert!(parse_path(input).is_err(), "expected error for {input:?}");
    }

    #[test]
    fn single_ident() {
        ok("a", &["a"]);
        ok("foo", &["foo"]);
        ok("foo_bar", &["foo_bar"]);
        ok("_private", &["_private"]);
        ok("kebab-case", &["kebab-case"]);
    }

    #[test]
    fn dotted_path() {
        ok("econ.seed", &["econ", "seed"]);
        ok("a.b.c", &["a", "b", "c"]);
        ok("knobs.spawn.softmax_temp", &["knobs", "spawn", "softmax_temp"]);
    }

    #[test]
    fn rejects_empty() {
        err("");
    }

    #[test]
    fn rejects_leading_dot() {
        err(".foo");
    }

    #[test]
    fn rejects_trailing_dot() {
        err("foo.");
    }

    #[test]
    fn rejects_double_dot() {
        err("foo..bar");
    }

    #[test]
    fn rejects_digit_start() {
        err("1foo");
        err("a.2b");
    }

    #[test]
    fn rejects_internal_space() {
        err("foo bar");
        err("foo. bar");
    }
}
