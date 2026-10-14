#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(kv) => kv.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        if let Value::Num(n) = self { Some(*n) } else { None }
    }
    pub fn as_i32(&self) -> Option<i32> {
        self.as_f64().map(|n| n.round() as i32)
    }
    pub fn as_str(&self) -> Option<&str> {
        if let Value::Str(s) = self { Some(s) } else { None }
    }
    pub fn as_arr(&self) -> &[Value] {
        if let Value::Arr(a) = self { a } else { &[] }
    }
    pub fn entries(&self) -> &[(String, Value)] {
        if let Value::Obj(kv) = self { kv } else { &[] }
    }
    pub fn ints(&self) -> Vec<i32> {
        self.as_arr().iter().filter_map(Value::as_i32).collect()
    }
}

impl Value {
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Num(n) if n.fract() == 0.0 && n.abs() < 9.0e15 => out.push_str(&format!("{}", *n as i64)),
            Value::Num(n) => out.push_str(&format!("{n}")),
            Value::Str(s) => write_str(s, out),
            Value::Arr(a) => {
                out.push('[');
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write(out);
                }
                out.push(']');
            }
            Value::Obj(kv) => {
                out.push('{');
                for (i, (k, v)) in kv.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_str(k, out);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

fn write_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn parse(text: &str) -> Result<Value, String> {
    let mut p = Parser { s: text.as_bytes(), i: 0 };
    let v = p.value()?;
    p.ws();
    if p.i != p.s.len() {
        return Err(p.err("trailing characters"));
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn err(&self, what: &str) -> String {
        let line = self.s[..self.i.min(self.s.len())].iter().filter(|&&c| c == b'\n').count() + 1;
        format!("{what} at line {line}")
    }
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }
    fn expect(&mut self, c: u8) -> Result<(), String> {
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(self.err(&format!("expected '{}'", c as char)))
        }
    }
    fn lit(&mut self, word: &str, v: Value) -> Result<Value, String> {
        if self.s[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(v)
        } else {
            Err(self.err("unexpected token"))
        }
    }
    fn value(&mut self) -> Result<Value, String> {
        match self.peek().ok_or_else(|| self.err("unexpected end"))? {
            b'{' => {
                self.i += 1;
                let mut kv = Vec::new();
                if self.peek() == Some(b'}') {
                    self.i += 1;
                    return Ok(Value::Obj(kv));
                }
                loop {
                    let Value::Str(k) = self.value()? else { return Err(self.err("expected key")) };
                    self.expect(b':')?;
                    kv.push((k, self.value()?));
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Value::Obj(kv));
                        }
                        _ => return Err(self.err("expected ',' or '}'")),
                    }
                }
            }
            b'[' => {
                self.i += 1;
                let mut a = Vec::new();
                if self.peek() == Some(b']') {
                    self.i += 1;
                    return Ok(Value::Arr(a));
                }
                loop {
                    a.push(self.value()?);
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Value::Arr(a));
                        }
                        _ => return Err(self.err("expected ',' or ']'")),
                    }
                }
            }
            b'"' => {
                self.i += 1;
                let mut out = String::new();
                loop {
                    let c = *self.s.get(self.i).ok_or_else(|| self.err("unterminated string"))?;
                    self.i += 1;
                    match c {
                        b'"' => return Ok(Value::Str(out)),
                        b'\\' => {
                            let e = *self.s.get(self.i).ok_or_else(|| self.err("bad escape"))?;
                            self.i += 1;
                            match e {
                                b'n' => out.push('\n'),
                                b't' => out.push('\t'),
                                b'r' => out.push('\r'),
                                b'b' => out.push('\u{8}'),
                                b'f' => out.push('\u{c}'),
                                b'u' => {
                                    let hex = self.s.get(self.i..self.i + 4).ok_or_else(|| self.err("bad \\u"))?;
                                    let code = u32::from_str_radix(std::str::from_utf8(hex).unwrap_or("x"), 16)
                                        .map_err(|_| self.err("bad \\u"))?;
                                    out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                                    self.i += 4;
                                }
                                other => out.push(other as char),
                            }
                        }
                        _ => {
                            let start = self.i - 1;
                            let len = match c {
                                0xF0.. => 4,
                                0xE0.. => 3,
                                0xC0.. => 2,
                                _ => 1,
                            };
                            let end = (start + len).min(self.s.len());
                            out.push_str(&String::from_utf8_lossy(&self.s[start..end]));
                            self.i = end;
                        }
                    }
                }
            }
            b't' => self.lit("true", Value::Bool(true)),
            b'f' => self.lit("false", Value::Bool(false)),
            b'n' => self.lit("null", Value::Null),
            _ => {
                let start = self.i;
                while self.i < self.s.len() && matches!(self.s[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                    self.i += 1;
                }
                std::str::from_utf8(&self.s[start..self.i])
                    .ok()
                    .and_then(|t| t.parse().ok())
                    .map(Value::Num)
                    .ok_or_else(|| self.err("bad number"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sidecars() {
        let v = parse(r#"{"x": 734, "y": -5, "a": [1, 2.5, [3]], "s": "hé\"llo", "t": true, "n": null, "o": {}}"#).unwrap();
        assert_eq!(v.get("x").and_then(Value::as_i32), Some(734));
        assert_eq!(v.get("y").and_then(Value::as_i32), Some(-5));
        assert_eq!(v.get("a").unwrap().as_arr().len(), 3);
        assert_eq!(v.get("s").and_then(Value::as_str), Some("hé\"llo"));
        assert_eq!(v.get("t"), Some(&Value::Bool(true)));
        assert_eq!(v.get("o").unwrap().entries().len(), 0);
        assert!(parse("{\"a\": }").is_err());
        assert!(parse("[1, 2").is_err());
    }

    #[test]
    fn round_trips() {
        let v = Value::Obj(vec![
            ("text".into(), Value::Str("a \"quoted\" \\ line\nnext é".into())),
            ("done".into(), Value::Bool(true)),
            ("n".into(), Value::Num(42.0)),
            ("f".into(), Value::Num(0.5)),
        ]);
        assert_eq!(parse(&v.to_json()).unwrap(), v);
    }
}
