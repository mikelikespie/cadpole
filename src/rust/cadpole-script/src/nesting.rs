//! A bound on how deeply a script nests, measured on the text before the
//! parser sees it (docs/security.md, "Starlark sandbox").
//!
//! starlark-rust parses by recursive descent, and its resolver, compiler and
//! evaluator all walk the tree recursively too, so every level of nesting in
//! the source costs a few kilobytes of native stack in each pass: measured on
//! a 2 MB thread, about 1.4 KB per parenthesis, 2.3 KB per operator, 4 KB per
//! indented block, 8 KB per nested `lambda`. A native stack overflow is not a
//! panic but an abort of the whole process, so a 10 KB script of nested
//! parentheses, or 800 `elif`s at one indent, took `cadpole serve` down with
//! every session in it. None of the interpreter's own limits help: the
//! call-stack cap bounds *runtime* recursion, and the parse has already
//! overflowed before the tick or heap counters exist. So the depth is bounded
//! here, on the bytes, where nothing recursive has run yet.
//!
//! The measure is an upper bound on the depth of the tree the parser would
//! build. Every open bracket counts one; every operator counts one (each is
//! a node with its operand below it, so `a + b + c` is three deep); `lambda`,
//! `if`, `else`, `for`, `in`, `and`, `or`, `not` and `.` count one each;
//! every indented block counts one, and so does every `elif`/`else` in a
//! chain, because the parser nests each one inside the last. A comma ends an
//! operand, so a long flat list of points costs two however long it is, and
//! a new logical line starts again from the block depth. Strings and
//! comments count nothing, except that the expressions inside an f-string's
//! braces are code and are measured like any other.
//!
//! The scan is a small hand-written tokenizer rather than starlark's own
//! lexer because the lexer is not re-exported by the `starlark` crate. It
//! only has to agree with the real one about where strings and comments
//! begin and end; everything else it may over-count, and a script it rejects
//! for over-counting would have to be a hundred operators deep in one
//! operand, which is not a script anyone writes.

/// Where a script crossed the nesting limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TooDeep {
    pub line: u32,
    pub col: u32,
    /// The depth reached at that point: one more than the limit.
    pub depth: usize,
    pub limit: usize,
}

impl std::fmt::Display for TooDeep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "script nests {} levels deep here (brackets, operators, lambdas, elif chains and \
             indented blocks combined); the limit is {}. Split the expression or the block up",
            self.depth, self.limit
        )
    }
}

impl std::error::Error for TooDeep {}

/// What the scanner is inside of, innermost last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frame {
    /// An open `(`, `[` or `{`.
    Bracket(u8),
    /// The expression part of an f-string, between `{` and `}`.
    FExpr,
    /// A string literal. `quote` is `"` or `'`; `triple` is `"""`/`'''`;
    /// `fstring` means `{expr}` inside it is code.
    Str { quote: u8, triple: bool, fstring: bool },
}

/// Refuse a script whose nesting would exceed `limit`.
pub fn check(source: &str, limit: usize) -> Result<(), TooDeep> {
    Scanner::new(source, limit).run()
}

struct Scanner<'a> {
    s: &'a [u8],
    i: usize,
    /// Where the token being counted starts, for the error's column.
    tok: usize,
    line: u32,
    line_start: usize,
    limit: usize,
    /// Open brackets, f-string expressions and strings, innermost last.
    stack: Vec<Frame>,
    /// One counter per code context (the statement, then each open bracket or
    /// f-string expression): the operators seen since the last comma there.
    ops: Vec<usize>,
    /// Depth of the expression at hand: open brackets plus every `ops`.
    units: usize,
    /// One entry per enclosing indented block: its width, and how many
    /// `elif`/`else` clauses the statement at that level has chained so far.
    /// The outermost entry is the module level, width 0.
    blocks: Vec<(usize, usize)>,
    /// Depth from `blocks`: the indented ones plus every chained clause.
    block_units: usize,
    /// True until the first token of a logical line, when indentation counts.
    at_line_start: bool,
}

impl<'a> Scanner<'a> {
    fn new(source: &'a str, limit: usize) -> Scanner<'a> {
        Scanner {
            s: source.as_bytes(),
            i: 0,
            tok: 0,
            line: 1,
            line_start: 0,
            limit,
            stack: Vec::new(),
            ops: vec![0],
            units: 0,
            blocks: vec![(0, 0)],
            block_units: 0,
            at_line_start: true,
        }
    }

    fn depth(&self) -> usize {
        self.block_units + self.units
    }

    fn checked(&self) -> Result<(), TooDeep> {
        if self.depth() > self.limit {
            return Err(TooDeep {
                line: self.line,
                col: (self.tok - self.line_start) as u32 + 1,
                depth: self.depth(),
                limit: self.limit,
            });
        }
        Ok(())
    }

    fn peek(&self, k: usize) -> u8 {
        self.s.get(self.i + k).copied().unwrap_or(0)
    }

    fn newline(&mut self) {
        self.line += 1;
        self.line_start = self.i;
    }

    fn in_code(&self) -> bool {
        !matches!(self.stack.last(), Some(Frame::Str { .. }))
    }

    /// A logical line has ended: the next token's indentation is a block.
    fn end_logical_line(&mut self) {
        self.units = 0;
        self.ops.clear();
        self.ops.push(0);
        self.at_line_start = true;
    }

    fn open(&mut self, f: Frame) -> Result<(), TooDeep> {
        self.stack.push(f);
        self.ops.push(0);
        self.units += 1;
        self.checked()
    }

    fn close_code_frame(&mut self) {
        self.stack.pop();
        let inner = self.ops.pop().unwrap_or(0);
        self.units = self.units.saturating_sub(1 + inner);
        if self.ops.is_empty() {
            self.ops.push(0);
        }
    }

    fn operator(&mut self) -> Result<(), TooDeep> {
        if let Some(c) = self.ops.last_mut() {
            *c += 1;
        }
        self.units += 1;
        self.checked()
    }

    fn comma(&mut self) {
        if let Some(c) = self.ops.last_mut() {
            self.units = self.units.saturating_sub(*c);
            *c = 0;
        }
    }

    /// A logical line starts at `self.i` with `width` bytes of indentation
    /// and its first word is `word`.
    fn statement(&mut self, width: usize, word: &[u8]) -> Result<(), TooDeep> {
        while self.blocks.len() > 1 && self.blocks.last().is_some_and(|&(w, _)| w > width) {
            let (_, chain) = self.blocks.pop().unwrap();
            self.block_units = self.block_units.saturating_sub(1 + chain);
        }
        if self.blocks.last().is_some_and(|&(w, _)| w < width) {
            self.blocks.push((width, 0));
            self.block_units += 1;
        }
        // `elif` / `else` continue the statement at this level, one level
        // deeper each; anything else starts a new one.
        if let Some((_, chain)) = self.blocks.last_mut() {
            if matches!(word, b"elif" | b"else") {
                *chain += 1;
                self.block_units += 1;
            } else {
                self.block_units = self.block_units.saturating_sub(*chain);
                *chain = 0;
            }
        }
        self.checked()
    }

    fn run(mut self) -> Result<(), TooDeep> {
        while self.i < self.s.len() {
            if self.in_code() {
                self.code_byte()?;
            } else {
                self.string_byte()?;
            }
        }
        Ok(())
    }

    fn string_byte(&mut self) -> Result<(), TooDeep> {
        let Some(Frame::Str { quote, triple, fstring }) = self.stack.last().copied() else { return Ok(()) };
        let c = self.peek(0);
        match c {
            b'\\' => {
                // An escape, in raw strings too: `r"\""` is two characters,
                // not the end of the string. The next byte is never a quote
                // that closes, so skip it (and count a newline it may be).
                let escaped = self.peek(1);
                self.i += 2;
                if escaped == b'\n' {
                    self.newline();
                }
            }
            b'\n' => {
                self.i += 1;
                self.newline();
                if !triple {
                    // An unterminated single-quoted string: the parser will
                    // refuse it; measure the rest as code so nothing hides.
                    self.stack.pop();
                }
            }
            b'{' if fstring => {
                if self.peek(1) == b'{' {
                    self.i += 2;
                } else {
                    self.tok = self.i;
                    self.i += 1;
                    self.open(Frame::FExpr)?;
                }
            }
            _ if c == quote => {
                if triple {
                    if self.peek(1) == quote && self.peek(2) == quote {
                        self.i += 3;
                        self.stack.pop();
                    } else {
                        self.i += 1;
                    }
                } else {
                    self.i += 1;
                    self.stack.pop();
                }
            }
            _ => self.i += 1,
        }
        Ok(())
    }

    fn code_byte(&mut self) -> Result<(), TooDeep> {
        // Indentation is read at the start of a logical line, from the first
        // non-blank byte; blank and comment-only lines set no block.
        if self.at_line_start && self.stack.is_empty() {
            let mut w = 0;
            while matches!(self.peek(w), b' ' | b'\t' | 0x0c) {
                w += 1;
            }
            match self.peek(w) {
                b'\n' | b'\r' | b'#' | 0 => {}
                b'\\' if self.peek(w + 1) == b'\n' => {}
                _ => {
                    self.at_line_start = false;
                    self.i += w;
                    self.tok = self.i;
                    let mut e = self.i;
                    while matches!(self.s.get(e), Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')) {
                        e += 1;
                    }
                    let word = &self.s[self.i..e];
                    return self.statement(w, word);
                }
            }
            self.i += w;
            if self.i >= self.s.len() {
                return Ok(());
            }
        }
        self.tok = self.i;
        let c = self.peek(0);
        match c {
            b'#' => {
                while self.i < self.s.len() && self.peek(0) != b'\n' {
                    self.i += 1;
                }
            }
            b'\n' => {
                self.i += 1;
                self.newline();
                if self.stack.is_empty() {
                    self.end_logical_line();
                }
            }
            b'\\' if self.peek(1) == b'\n' => {
                self.i += 2;
                self.newline();
            }
            b'\\' if self.peek(1) == b'\r' && self.peek(2) == b'\n' => {
                self.i += 3;
                self.newline();
            }
            b' ' | b'\t' | b'\r' | 0x0c => self.i += 1,
            b'(' | b'[' | b'{' => {
                self.i += 1;
                self.open(Frame::Bracket(c))?;
            }
            b')' | b']' => {
                self.i += 1;
                let want = if c == b')' { b'(' } else { b'[' };
                if self.stack.last() == Some(&Frame::Bracket(want)) {
                    self.close_code_frame();
                }
            }
            b'}' => {
                self.i += 1;
                match self.stack.last() {
                    Some(Frame::Bracket(b'{')) | Some(Frame::FExpr) => self.close_code_frame(),
                    _ => {}
                }
            }
            b',' => {
                self.i += 1;
                self.comma();
            }
            b';' => {
                self.i += 1;
                if self.stack.is_empty() {
                    self.end_logical_line();
                    self.at_line_start = false;
                }
            }
            b'"' | b'\'' => self.string_start(false),
            b'0'..=b'9' => {
                // A number, including `1.5e-3` and `0x1f`; the sign of an
                // exponent is counted as an operator, which errs upwards.
                while matches!(self.peek(0), b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'.') {
                    self.i += 1;
                }
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                let start = self.i;
                while matches!(self.peek(0), b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_') {
                    self.i += 1;
                }
                let word = &self.s[start..self.i];
                if matches!(self.peek(0), b'"' | b'\'') {
                    if let Some(fstring) = string_prefix(word) {
                        self.string_start(fstring);
                        return Ok(());
                    }
                }
                if matches!(word, b"lambda" | b"if" | b"else" | b"for" | b"in" | b"and" | b"or" | b"not") {
                    self.operator()?;
                }
            }
            // Every operator character. Multi-byte operators (`**`, `//`,
            // `<<`, `==`, ...) count once.
            b'+' | b'-' | b'*' | b'/' | b'%' | b'<' | b'>' | b'=' | b'!' | b'&' | b'|' | b'^' | b'~' | b'.' => {
                let two = self.peek(1);
                self.i += 1;
                if (c == two && matches!(c, b'*' | b'/' | b'<' | b'>' | b'=')) || two == b'=' {
                    self.i += 1;
                }
                // `=` alone is assignment or a keyword argument; `:` and the
                // rest add no depth. `==`, `<=` and the like are comparisons.
                if c == b'=' && two != b'=' {
                    return Ok(());
                }
                self.operator()?;
            }
            _ => self.i += 1,
        }
        Ok(())
    }

    fn string_start(&mut self, fstring: bool) {
        let quote = self.peek(0);
        let triple = self.peek(1) == quote && self.peek(2) == quote;
        self.i += if triple { 3 } else { 1 };
        self.stack.push(Frame::Str { quote, triple, fstring });
    }
}

/// Whether a string prefix such as `r`, `b`, `f`, `rb`, `fr` makes an
/// f-string; `None` when the word is not a prefix at all.
fn string_prefix(word: &[u8]) -> Option<bool> {
    if word.is_empty() || word.len() > 2 {
        return None;
    }
    let mut fstring = false;
    for &c in word {
        match c.to_ascii_lowercase() {
            b'r' | b'b' => {}
            b'f' => fstring = true,
            _ => return None,
        }
    }
    Some(fstring)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn depth_ok(src: &str, limit: usize) -> bool {
        check(src, limit).is_ok()
    }

    #[test]
    fn brackets_operators_lambdas_and_blocks_all_count() {
        assert!(check(&format!("x = {}1{}\n", "(".repeat(101), ")".repeat(101)), 100).is_err());
        assert!(check(&format!("x = {}1{}\n", "[".repeat(101), "]".repeat(101)), 100).is_err());
        assert!(check(&format!("x = {}1{}\n", "str(".repeat(101), ")".repeat(101)), 100).is_err());
        assert!(check(&format!("x = {}1\n", "-".repeat(101)), 100).is_err());
        assert!(check(&format!("x = {}1\n", "not ".repeat(101)), 100).is_err());
        assert!(check(&format!("x = 1{}\n", "+1".repeat(101)), 100).is_err());
        assert!(check(&format!("x = 'a'{}\n", ".strip()".repeat(101)), 100).is_err());
        assert!(check(&format!("x = {}1\n", "lambda: ".repeat(101)), 100).is_err());
        assert!(check(&format!("x = {}1\n", "1 if 1 else ".repeat(101)), 100).is_err());
        let ifs: String = (0..101).map(|i| format!("{}if True:\n", " ".repeat(i))).collect();
        assert!(check(&format!("{ifs}{}pass\n", " ".repeat(101)), 100).is_err());
        let defs: String = (0..101).map(|i| format!("{}def f{i}():\n", " ".repeat(i))).collect();
        assert!(check(&format!("{defs}{}pass\n", " ".repeat(101)), 100).is_err());
        // The same shapes one under the limit pass.
        assert!(depth_ok(&format!("x = {}1{}\n", "(".repeat(99), ")".repeat(99)), 100));
        assert!(depth_ok(&format!("x = {}1\n", "lambda: ".repeat(99)), 100));
        assert!(depth_ok(&format!("{ifs}{}pass\n", " ".repeat(101)), 102));
    }

    /// The parser nests every `elif` inside the previous one, so a chain at
    /// a single indent is as deep as the chain is long.
    #[test]
    fn an_elif_chain_counts_one_per_clause() {
        let chain = |n: usize| format!("if a:\n    pass\n{}x = 1\n", "elif a:\n    pass\n".repeat(n));
        assert!(check(&chain(101), 100).is_err());
        assert!(depth_ok(&chain(90), 100));
        // ...and a following statement at that level starts afresh.
        let two = format!("{}\n{}", chain(60), chain(60));
        assert!(depth_ok(&two, 100));
        // A chain inside a block adds to the block's depth.
        let nested = format!("def f():\n    if a:\n        pass\n{}", "    elif a:\n        pass\n".repeat(100));
        assert!(check(&nested, 100).is_err());
        // An `else` after the chain is one more; the body under it is one more.
        let src = "if a:\n    pass\nelif b:\n    pass\nelse:\n    pass\n";
        assert!(depth_ok(src, 3));
        assert!(check(src, 2).is_err());
    }

    #[test]
    fn the_error_says_where() {
        let e = check("a = 1\nb = ((((1))))\n", 3).unwrap_err();
        assert_eq!((e.line, e.col), (2, 8), "{e:?}");
        assert_eq!(e.depth, 4);
        assert!(e.to_string().contains("the limit is 3"), "{e}");
        let e = check("if a:\n    if b:\n        if c:\n            pass\n", 2).unwrap_err();
        assert_eq!((e.line, e.col), (3, 9), "{e:?}");
    }

    #[test]
    fn nesting_inside_an_fstring_expression_is_measured() {
        let deep = format!("x = f\"{{{}1{}}}\"\n", "(".repeat(101), ")".repeat(101));
        assert!(check(&deep, 100).is_err());
        let deep = format!("x = rf'{{{}1{}}}'\n", "[".repeat(101), "]".repeat(101));
        assert!(check(&deep, 100).is_err());
        // Braces in the text part are not code.
        assert!(depth_ok("x = f\"{{{{{{{{{{{{{{{{{{{{\"\n", 5));
        // A string inside an f-string expression is a string.
        assert!(depth_ok("x = f\"{'((((((((((((('}\"\n", 5));
    }

    #[test]
    fn strings_and_comments_hide_nothing_and_count_nothing() {
        let parens_in_string = format!("x = \"{}\"\n", "(".repeat(500));
        assert!(depth_ok(&parens_in_string, 10));
        let parens_in_comment = format!("x = 1  # {}\n", "(".repeat(500));
        assert!(depth_ok(&parens_in_comment, 10));
        let triple = format!("x = \"\"\"a\n{}\n\"\"\"\n", "[".repeat(500));
        assert!(depth_ok(&triple, 10));
        let raw = format!("x = r\"\\\"{}\"\n", "(".repeat(500));
        assert!(depth_ok(&raw, 10));
        let bytes = format!("x = b'{}'\n", "{".repeat(500));
        assert!(depth_ok(&bytes, 10));
        let escaped = "x = \"a\\\"((((((((((((((((((((\"\n";
        assert!(depth_ok(escaped, 5));
        // A string with an escaped backslash right before the closing quote
        // still ends where the parser says it does.
        let after = format!("x = \"a\\\\\"; y = {}1{}\n", "(".repeat(12), ")".repeat(12));
        assert!(check(&after, 10).is_err());
    }

    #[test]
    fn a_long_flat_literal_is_shallow() {
        let table: String = (0..5000).map(|i| format!("({i}, -{i}, {i}.5e-3), ")).collect();
        assert!(depth_ok(&format!("pts = [{table}]\n"), 6));
        let kwargs: String = (0..500).map(|i| format!("a{i}=-{i}, ")).collect();
        assert!(depth_ok(&format!("f({kwargs})\n"), 6));
        let dict: String = (0..500).map(|i| format!("\"k{i}\": -{i} * 2, ")).collect();
        assert!(depth_ok(&format!("d = {{{dict}}}\n"), 6));
        let multiline: String = (0..500).map(|i| format!("    ({i}, {i}),\n")).collect();
        assert!(depth_ok(&format!("pts = [\n{multiline}]\n"), 6));
    }

    #[test]
    fn a_new_statement_starts_from_the_block_depth() {
        let many: String = (0..1000).map(|i| format!("x{i} = ((1 + 2) * 3)\n")).collect();
        assert!(depth_ok(&many, 6));
        // Dedenting pops the block.
        let src = "def f():\n    if x:\n        y = 1\n    z = 2\nw = 3\n";
        assert!(depth_ok(src, 3));
        // Continuation lines belong to the same logical line.
        let cont = format!("x = 1 + \\\n    {}1{}\n", "(".repeat(6), ")".repeat(6));
        assert!(check(&cont, 5).is_err());
        // Two statements on one line are two statements.
        assert!(depth_ok("x = (((1))); y = (((2)))\n", 3));
    }

    #[test]
    fn the_examples_are_nowhere_near_the_limit() {
        let dir = std::env::var("CADPOLE_EXAMPLES").unwrap_or_else(|_| "examples".into());
        let mut seen = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let p = entry.unwrap().path();
            if p.extension().is_some_and(|e| e == "star") {
                let src = std::fs::read_to_string(&p).unwrap();
                // Real scripts sit under a fifth of the limit.
                assert!(depth_ok(&src, crate::Limits::MAX_NESTING / 5), "{}", p.display());
                seen += 1;
            }
        }
        assert!(seen > 5, "found {seen} example scripts in {dir}");
    }
}
