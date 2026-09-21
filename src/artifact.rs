//! `Builtin.roc`, parsed at BUILD time and read back at run time.
//!
//! A four-line program that names a `Dict` parses 1,358 lines of `Builtin.roc` — source
//! the user did not write and that cannot change without rebuilding the binary. That
//! was 1.8ms of a 2.6ms run. This is the same trees, written out once by
//! `cargo run --bin gen-artifact` and `include_bytes!`d back in.
//!
//! **Every name is borrowed from the blob, not interned.** The strings live in the
//! binary's read-only data, so reading one is a slice and not an allocation — which is
//! sound because every comparison of a name in the interpreter is by content, as
//! `memory::string_pool` already documents. That is the single biggest reason this is
//! faster than parsing rather than merely different.
//!
//! **Staleness is a BUILD error, not a run-time check.** The artifact records a hash of
//! the source it was made from and `build.rs` compares it against `src/roc/Builtin.roc`,
//! so an artifact that no longer matches fails `cargo build` with the command to
//! regenerate it. Checking at run time would mean hashing 700kB on every startup, which
//! is most of what this phase saves.

use crate::ast::{Expr, MatchArm, NodeId, Pattern, StrPart};
use crate::types::Type;

/// Bumped whenever the FORMAT changes, so an artifact from an older tree is rejected by
/// `build.rs` rather than decoded as nonsense.
pub const MAGIC: &[u8; 8] = b"ROCFLT02";

/// FNV-1a of the source an artifact was built from. `build.rs` computes the same thing
/// over `src/roc/Builtin.roc` and refuses to build if they differ.
pub fn source_hash(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

// ---------------------------------------------------------------- writing

/// Builds the blob. Used by `gen-artifact` only; the interpreter never writes one.
pub struct Writer {
    out: Vec<u8>,
    strings: Vec<String>,
    index: std::collections::HashMap<String, u32>,
}

impl Default for Writer {
    fn default() -> Self {
        Writer { out: Vec::new(), strings: Vec::new(), index: std::collections::HashMap::new() }
    }
}

impl Writer {
    fn u(&mut self, mut n: u64) {
        // LEB128: a length, a node id or a small tag is one byte almost always.
        while n >= 0x80 {
            self.out.push((n as u8) | 0x80);
            n >>= 7;
        }
        self.out.push(n as u8);
    }

    fn tag(&mut self, t: u8) {
        self.out.push(t);
    }

    fn i128(&mut self, n: i128) {
        self.out.extend_from_slice(&n.to_le_bytes());
    }

    fn f64(&mut self, n: f64) {
        self.out.extend_from_slice(&n.to_bits().to_le_bytes());
    }

    fn bool(&mut self, b: bool) {
        self.out.push(u8::from(b));
    }

    /// A name, as an index into the shared table. `Builtin.roc` repeats its names
    /// heavily, so this is both smaller and one less thing to read back.
    fn s(&mut self, text: &str) {
        let next = self.strings.len() as u32;
        let at = *self.index.entry(text.to_string()).or_insert_with(|| {
            self.strings.push(text.to_string());
            next
        });
        self.u(u64::from(at));
    }

    fn node(&mut self, id: NodeId, base: u32) {
        // Relative to the member's own first node, so loading rebases by addition.
        self.u(u64::from(id.index() as u32 - base));
    }

    fn seq<T>(&mut self, items: &[T], mut each: impl FnMut(&mut Self, &T)) {
        self.u(items.len() as u64);
        for item in items {
            each(self, item);
        }
    }

    /// The finished blob: magic, source hash, string table, then the members.
    pub fn finish(self, hash: u64, body_count: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.out.len() + 64 * self.strings.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&hash.to_le_bytes());
        out.extend_from_slice(&(body_count as u32).to_le_bytes());
        // The table: every distinct name, length-prefixed, in one run.
        let mut header = Writer::default();
        header.u(self.strings.len() as u64);
        for text in &self.strings {
            header.u(text.len() as u64);
        }
        out.extend_from_slice(&header.out);
        for text in &self.strings {
            out.extend_from_slice(text.as_bytes());
        }
        out.extend_from_slice(&self.out);
        out
    }
}

// ---------------------------------------------------------------- reading

pub struct Reader<'a> {
    blob: &'static [u8],
    at: usize,
    strings: &'a [&'static str],
}

impl<'a> Reader<'a> {
    fn u(&mut self) -> u64 {
        let mut n = 0u64;
        let mut shift = 0;
        loop {
            let byte = self.blob[self.at];
            self.at += 1;
            n |= u64::from(byte & 0x7f) << shift;
            if byte < 0x80 {
                return n;
            }
            shift += 7;
        }
    }

    fn tag(&mut self) -> u8 {
        let t = self.blob[self.at];
        self.at += 1;
        t
    }

    fn i128(&mut self) -> i128 {
        let mut buf = [0u8; 16];
        buf.copy_from_slice(&self.blob[self.at..self.at + 16]);
        self.at += 16;
        i128::from_le_bytes(buf)
    }

    fn f64(&mut self) -> f64 {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&self.blob[self.at..self.at + 8]);
        self.at += 8;
        f64::from_bits(u64::from_le_bytes(buf))
    }

    fn bool(&mut self) -> bool {
        self.tag() != 0
    }

    /// A name, borrowed from the blob. No allocation and no interning: the bytes are in
    /// the binary and outlive everything.
    fn s(&mut self) -> &'static str {
        let at = self.u() as usize;
        self.strings[at]
    }

    fn node(&mut self, base: u32) -> NodeId {
        NodeId(base + self.u() as u32)
    }

    fn seq<T>(&mut self, base: u32, mut each: impl FnMut(&mut Self, u32) -> T) -> Vec<T> {
        let n = self.u() as usize;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(each(self, base));
        }
        out
    }
}

// ---------------------------------------------------------------- types

fn put_type(w: &mut Writer, ty: &Type) {
    match ty {
        Type::Str => w.tag(0),
        Type::U8 => w.tag(1),
        Type::U16 => w.tag(2),
        Type::U32 => w.tag(3),
        Type::U64 => w.tag(4),
        Type::U128 => w.tag(5),
        Type::I8 => w.tag(6),
        Type::I16 => w.tag(7),
        Type::I32 => w.tag(8),
        Type::I64 => w.tag(9),
        Type::I128 => w.tag(10),
        Type::F32 => w.tag(11),
        Type::F64 => w.tag(12),
        Type::Dec => w.tag(13),
        Type::Bool => w.tag(14),
        Type::Unit => w.tag(15),
        Type::TypeVar(v) => {
            w.tag(16);
            w.u(u64::from(*v));
        }
        Type::List(inner) => {
            w.tag(17);
            put_type(w, inner);
        }
        Type::Function(a, b) => {
            w.tag(18);
            put_type(w, a);
            put_type(w, b);
        }
        Type::Optional(inner) => {
            w.tag(19);
            put_type(w, inner);
        }
        Type::Range(inner) => {
            w.tag(20);
            put_type(w, inner);
        }
        Type::Tuple(items) => {
            w.tag(21);
            w.seq(items, |w, t| put_type(w, t));
        }
        Type::Record { fields, open } => {
            w.tag(22);
            w.seq(fields, |w, (n, t)| {
                w.s(n);
                put_type(w, t);
            });
            w.bool(*open);
        }
        Type::Nominal { name, backing } => {
            w.tag(23);
            w.s(name);
            put_type(w, backing);
        }
        Type::TagUnion { tags, open } => {
            w.tag(24);
            w.seq(tags, |w, (n, payload)| {
                w.s(n);
                w.seq(payload, |w, t| put_type(w, t));
            });
            w.bool(*open);
        }
    }
}

fn get_type(r: &mut Reader) -> Type {
    match r.tag() {
        0 => Type::Str,
        1 => Type::U8,
        2 => Type::U16,
        3 => Type::U32,
        4 => Type::U64,
        5 => Type::U128,
        6 => Type::I8,
        7 => Type::I16,
        8 => Type::I32,
        9 => Type::I64,
        10 => Type::I128,
        11 => Type::F32,
        12 => Type::F64,
        13 => Type::Dec,
        14 => Type::Bool,
        15 => Type::Unit,
        16 => Type::TypeVar(r.u() as u32),
        17 => Type::List(Box::new(get_type(r))),
        18 => {
            let a = get_type(r);
            let b = get_type(r);
            Type::Function(Box::new(a), Box::new(b))
        }
        19 => Type::Optional(Box::new(get_type(r))),
        20 => Type::Range(Box::new(get_type(r))),
        21 => Type::Tuple(r.seq(0, |r, _| get_type(r))),
        22 => {
            let fields = r.seq(0, |r, _| (r.s(), get_type(r)));
            Type::Record { fields, open: r.bool() }
        }
        23 => {
            let name = r.s();
            Type::Nominal { name, backing: Box::new(get_type(r)) }
        }
        24 => {
            let tags = r.seq(0, |r, _| (r.s(), r.seq(0, |r, _| get_type(r))));
            Type::TagUnion { tags, open: r.bool() }
        }
        other => unreachable_tag("Type", other),
    }
}

// ---------------------------------------------------------------- patterns

fn put_pattern(w: &mut Writer, p: &Pattern) {
    match p {
        Pattern::Wildcard => w.tag(0),
        Pattern::Binding(n) => {
            w.tag(1);
            w.s(n);
        }
        Pattern::Int(n) => {
            w.tag(2);
            w.i128(*n);
        }
        Pattern::Float(f, raw) => {
            w.tag(3);
            w.f64(*f);
            w.i128(*raw);
        }
        Pattern::Str(s) => {
            w.tag(4);
            w.s(s);
        }
        Pattern::StrInterp { prefix, segments } => {
            w.tag(5);
            w.s(prefix);
            w.seq(segments, |w, (a, b)| {
                w.s(a);
                w.s(b);
            });
        }
        Pattern::As { name, inner } => {
            w.tag(6);
            w.s(name);
            put_pattern(w, inner);
        }
        Pattern::Tag { name, args } => {
            w.tag(7);
            w.s(name);
            w.seq(args, |w, p| put_pattern(w, p));
        }
        Pattern::Tuple(items) => {
            w.tag(8);
            w.seq(items, |w, p| put_pattern(w, p));
        }
        Pattern::Record { fields, rest } => {
            w.tag(9);
            w.seq(fields, |w, (n, p)| {
                w.s(n);
                put_pattern(w, p);
            });
            put_opt_str(w, rest.as_ref());
        }
        Pattern::List { before, rest, after } => {
            w.tag(10);
            w.seq(before, |w, p| put_pattern(w, p));
            match rest {
                None => w.tag(0),
                Some(None) => w.tag(1),
                Some(Some(name)) => {
                    w.tag(2);
                    w.s(name);
                }
            }
            w.seq(after, |w, p| put_pattern(w, p));
        }
    }
}

fn get_pattern(r: &mut Reader) -> Pattern {
    match r.tag() {
        0 => Pattern::Wildcard,
        1 => Pattern::Binding(r.s()),
        2 => Pattern::Int(r.i128()),
        3 => {
            let f = r.f64();
            Pattern::Float(f, r.i128())
        }
        4 => Pattern::Str(r.s()),
        5 => {
            let prefix = r.s();
            Pattern::StrInterp { prefix, segments: r.seq(0, |r, _| (r.s(), r.s())) }
        }
        6 => {
            let name = r.s();
            Pattern::As { name, inner: Box::new(get_pattern(r)) }
        }
        7 => {
            let name = r.s();
            Pattern::Tag { name, args: r.seq(0, |r, _| get_pattern(r)) }
        }
        8 => Pattern::Tuple(r.seq(0, |r, _| get_pattern(r))),
        9 => {
            let fields = r.seq(0, |r, _| (r.s(), get_pattern(r)));
            Pattern::Record { fields, rest: get_opt_str(r) }
        }
        10 => {
            let before = r.seq(0, |r, _| get_pattern(r));
            let rest = match r.tag() {
                0 => None,
                1 => Some(None),
                2 => Some(Some(r.s())),
                other => unreachable_tag("Pattern::List rest", other),
            };
            Pattern::List { before, rest, after: r.seq(0, |r, _| get_pattern(r)) }
        }
        other => unreachable_tag("Pattern", other),
    }
}

fn put_opt_str(w: &mut Writer, s: Option<&&'static str>) {
    match s {
        None => w.tag(0),
        Some(text) => {
            w.tag(1);
            w.s(text);
        }
    }
}

fn get_opt_str(r: &mut Reader) -> Option<&'static str> {
    if r.tag() == 0 {
        None
    } else {
        Some(r.s())
    }
}

// ---------------------------------------------------------------- expressions

fn put_expr(w: &mut Writer, e: &Expr, base: u32) {
    use crate::ast::BinOp;
    match e {
        Expr::Str(s, id) => {
            w.tag(0);
            w.s(s);
            w.node(*id, base);
        }
        Expr::StrInterp(parts, id) => {
            w.tag(1);
            w.seq(parts, |w, part| match part {
                StrPart::Literal(s) => {
                    w.tag(0);
                    w.s(s);
                }
                StrPart::Expr(inner) => {
                    w.tag(1);
                    put_expr(w, inner, base);
                }
            });
            w.node(*id, base);
        }
        Expr::Int(n, id) => {
            w.tag(2);
            w.i128(*n);
            w.node(*id, base);
        }
        Expr::Float(f, raw, id) => {
            w.tag(3);
            w.f64(*f);
            w.i128(*raw);
            w.node(*id, base);
        }
        Expr::Ident(s, id) => {
            w.tag(4);
            w.s(s);
            w.node(*id, base);
        }
        Expr::Qualified { module, name, id } => {
            w.tag(5);
            w.s(module);
            w.s(name);
            w.node(*id, base);
        }
        Expr::BinOp { left, op, right, id } => {
            w.tag(6);
            put_expr(w, left, base);
            w.tag(binop_tag(*op));
            put_expr(w, right, base);
            w.node(*id, base);
        }
        Expr::Lambda { params, body, id } => {
            w.tag(7);
            w.seq(params, |w, p| w.s(p));
            put_expr(w, body, base);
            w.node(*id, base);
        }
        Expr::Call { func, args, id } => {
            w.tag(8);
            put_expr(w, func, base);
            w.seq(args, |w, a| put_expr(w, a, base));
            w.node(*id, base);
        }
        Expr::Let { name, annotation, value, body, id } => {
            w.tag(9);
            w.s(name);
            match annotation {
                None => w.tag(0),
                Some(ty) => {
                    w.tag(1);
                    put_type(w, ty);
                }
            }
            put_expr(w, value, base);
            put_expr(w, body, base);
            w.node(*id, base);
        }
        Expr::Unit(id) => {
            w.tag(10);
            w.node(*id, base);
        }
        Expr::Record(fields, id) => {
            w.tag(11);
            w.seq(fields, |w, (n, v)| {
                w.s(n);
                put_expr(w, v, base);
            });
            w.node(*id, base);
        }
        Expr::Bool(b, id) => {
            w.tag(12);
            w.bool(*b);
            w.node(*id, base);
        }
        Expr::List(items, id) => {
            w.tag(13);
            w.seq(items, |w, i| put_expr(w, i, base));
            w.node(*id, base);
        }
        Expr::RecordUpdate { base: record, fields, id } => {
            w.tag(14);
            put_expr(w, record, base);
            w.seq(fields, |w, (n, v)| {
                w.s(n);
                put_expr(w, v, base);
            });
            w.node(*id, base);
        }
        Expr::Range { start, end, inclusive, id } => {
            w.tag(15);
            put_expr(w, start, base);
            put_expr(w, end, base);
            w.bool(*inclusive);
            w.node(*id, base);
        }
        Expr::Tuple(items, id) => {
            w.tag(16);
            w.seq(items, |w, i| put_expr(w, i, base));
            w.node(*id, base);
        }
        Expr::TupleIndex { tuple, index, id } => {
            w.tag(17);
            put_expr(w, tuple, base);
            w.u(*index as u64);
            w.node(*id, base);
        }
        Expr::Match { scrutinee, arms, id } => {
            w.tag(18);
            put_expr(w, scrutinee, base);
            w.seq(arms, |w, arm| {
                w.seq(&arm.patterns, |w, p| put_pattern(w, p));
                match &arm.guard {
                    None => w.tag(0),
                    Some(g) => {
                        w.tag(1);
                        put_expr(w, g, base);
                    }
                }
                put_expr(w, &arm.body, base);
            });
            w.node(*id, base);
        }
        Expr::If { condition, then_branch, otherwise, id } => {
            w.tag(19);
            put_expr(w, condition, base);
            put_expr(w, then_branch, base);
            put_expr(w, otherwise, base);
            w.node(*id, base);
        }
        Expr::VarDecl { name, value, body, id } => {
            w.tag(20);
            w.s(name);
            put_expr(w, value, base);
            put_expr(w, body, base);
            w.node(*id, base);
        }
        Expr::Assign { name, value, body, id } => {
            w.tag(21);
            w.s(name);
            put_expr(w, value, base);
            put_expr(w, body, base);
            w.node(*id, base);
        }
        Expr::For { name, iterable, body, id } => {
            w.tag(22);
            w.s(name);
            put_expr(w, iterable, base);
            put_expr(w, body, base);
            w.node(*id, base);
        }
        Expr::While { condition, body, id } => {
            w.tag(23);
            put_expr(w, condition, base);
            put_expr(w, body, base);
            w.node(*id, base);
        }
        Expr::Return(inner, id) => {
            w.tag(24);
            put_expr(w, inner, base);
            w.node(*id, base);
        }
        Expr::Crash(inner, id) => {
            w.tag(25);
            put_expr(w, inner, base);
            w.node(*id, base);
        }
        Expr::Expect(inner, id) => {
            w.tag(26);
            put_expr(w, inner, base);
            w.node(*id, base);
        }
        Expr::Dbg(inner, id) => {
            w.tag(27);
            put_expr(w, inner, base);
            w.node(*id, base);
        }
        Expr::Break(id) => {
            w.tag(28);
            w.node(*id, base);
        }
        Expr::Dispatch { receiver, method, args, id } => {
            w.tag(29);
            put_expr(w, receiver, base);
            w.s(method);
            w.seq(args, |w, a| put_expr(w, a, base));
            w.node(*id, base);
        }
        Expr::OptionalField { record, field, id } => {
            w.tag(30);
            put_expr(w, record, base);
            w.s(field);
            w.node(*id, base);
        }
        Expr::FieldAccess { record, field, id } => {
            w.tag(31);
            put_expr(w, record, base);
            w.s(field);
            w.node(*id, base);
        }
        Expr::Tag { name, args, id } => {
            w.tag(32);
            w.s(name);
            w.seq(args, |w, a| put_expr(w, a, base));
            w.node(*id, base);
        }
    }
    // `BinOp` is a plain C-like enum; keeping its mapping beside the writer rather than
    // deriving a number from the variant order means reordering it cannot silently
    // change what an old artifact decodes to — `build.rs` rejects it instead.
    fn binop_tag(op: BinOp) -> u8 {
        match op {
            BinOp::Add => 0,
            BinOp::Sub => 1,
            BinOp::Mul => 2,
            BinOp::Div => 3,
            BinOp::IntDiv => 4,
            BinOp::Rem => 5,
            BinOp::Eq => 6,
            BinOp::Ne => 7,
            BinOp::Lt => 8,
            BinOp::Le => 9,
            BinOp::Gt => 10,
            BinOp::Ge => 11,
            BinOp::And => 12,
            BinOp::Or => 13,
        }
    }
}

fn get_expr(r: &mut Reader, base: u32) -> Expr {
    use crate::ast::BinOp;
    fn binop(t: u8) -> BinOp {
        match t {
            0 => BinOp::Add,
            1 => BinOp::Sub,
            2 => BinOp::Mul,
            3 => BinOp::Div,
            4 => BinOp::IntDiv,
            5 => BinOp::Rem,
            6 => BinOp::Eq,
            7 => BinOp::Ne,
            8 => BinOp::Lt,
            9 => BinOp::Le,
            10 => BinOp::Gt,
            11 => BinOp::Ge,
            12 => BinOp::And,
            13 => BinOp::Or,
            other => unreachable_tag("BinOp", other),
        }
    }
    match r.tag() {
        0 => {
            let s = r.s();
            Expr::Str(s, r.node(base))
        }
        1 => {
            let parts = r.seq(base, |r, base| match r.tag() {
                0 => StrPart::Literal(r.s()),
                // A leaked `&'static Expr`, as the parser makes one.
                1 => StrPart::Expr(Box::leak(Box::new(get_expr(r, base)))),
                other => unreachable_tag("StrPart", other),
            });
            Expr::StrInterp(parts, r.node(base))
        }
        2 => {
            let n = r.i128();
            Expr::Int(n, r.node(base))
        }
        3 => {
            let f = r.f64();
            let raw = r.i128();
            Expr::Float(f, raw, r.node(base))
        }
        4 => {
            let s = r.s();
            Expr::Ident(s, r.node(base))
        }
        5 => {
            let module = r.s();
            let name = r.s();
            Expr::Qualified { module, name, id: r.node(base) }
        }
        6 => {
            let left = Box::new(get_expr(r, base));
            let op = binop(r.tag());
            let right = Box::new(get_expr(r, base));
            Expr::BinOp { left, op, right, id: r.node(base) }
        }
        7 => {
            let params: std::rc::Rc<[&'static str]> = r.seq(base, |r, _| r.s()).into();
            let body = std::rc::Rc::new(get_expr(r, base));
            Expr::Lambda { params, body, id: r.node(base) }
        }
        8 => {
            let func = Box::new(get_expr(r, base));
            let args = r.seq(base, get_expr);
            Expr::Call { func, args, id: r.node(base) }
        }
        9 => {
            let name = r.s();
            let annotation = if r.tag() == 0 { None } else { Some(get_type(r)) };
            let value = Box::new(get_expr(r, base));
            let body = Box::new(get_expr(r, base));
            Expr::Let { name, annotation, value, body, id: r.node(base) }
        }
        10 => Expr::Unit(r.node(base)),
        11 => {
            let fields = r.seq(base, |r, base| (r.s(), get_expr(r, base)));
            Expr::Record(fields, r.node(base))
        }
        12 => {
            let b = r.bool();
            Expr::Bool(b, r.node(base))
        }
        13 => {
            let items = r.seq(base, get_expr);
            Expr::List(items, r.node(base))
        }
        14 => {
            let record = Box::new(get_expr(r, base));
            let fields = r.seq(base, |r, base| (r.s(), get_expr(r, base)));
            Expr::RecordUpdate { base: record, fields, id: r.node(base) }
        }
        15 => {
            let start = Box::new(get_expr(r, base));
            let end = Box::new(get_expr(r, base));
            let inclusive = r.bool();
            Expr::Range { start, end, inclusive, id: r.node(base) }
        }
        16 => {
            let items = r.seq(base, get_expr);
            Expr::Tuple(items, r.node(base))
        }
        17 => {
            let tuple = Box::new(get_expr(r, base));
            let index = r.u() as usize;
            Expr::TupleIndex { tuple, index, id: r.node(base) }
        }
        18 => {
            let scrutinee = Box::new(get_expr(r, base));
            let arms = r.seq(base, |r, base| {
                let patterns = r.seq(base, |r, _| get_pattern(r));
                let guard = if r.tag() == 0 { None } else { Some(get_expr(r, base)) };
                MatchArm { patterns, guard, body: get_expr(r, base) }
            });
            Expr::Match { scrutinee, arms, id: r.node(base) }
        }
        19 => {
            let condition = Box::new(get_expr(r, base));
            let then_branch = Box::new(get_expr(r, base));
            let otherwise = Box::new(get_expr(r, base));
            Expr::If { condition, then_branch, otherwise, id: r.node(base) }
        }
        20 => {
            let name = r.s();
            let value = Box::new(get_expr(r, base));
            let body = Box::new(get_expr(r, base));
            Expr::VarDecl { name, value, body, id: r.node(base) }
        }
        21 => {
            let name = r.s();
            let value = Box::new(get_expr(r, base));
            let body = Box::new(get_expr(r, base));
            Expr::Assign { name, value, body, id: r.node(base) }
        }
        22 => {
            let name = r.s();
            let iterable = Box::new(get_expr(r, base));
            let body = Box::new(get_expr(r, base));
            Expr::For { name, iterable, body, id: r.node(base) }
        }
        23 => {
            let condition = Box::new(get_expr(r, base));
            let body = Box::new(get_expr(r, base));
            Expr::While { condition, body, id: r.node(base) }
        }
        24 => {
            let inner = Box::new(get_expr(r, base));
            Expr::Return(inner, r.node(base))
        }
        25 => {
            let inner = Box::new(get_expr(r, base));
            Expr::Crash(inner, r.node(base))
        }
        26 => {
            let inner = Box::new(get_expr(r, base));
            Expr::Expect(inner, r.node(base))
        }
        27 => {
            let inner = Box::new(get_expr(r, base));
            Expr::Dbg(inner, r.node(base))
        }
        28 => Expr::Break(r.node(base)),
        29 => {
            let receiver = Box::new(get_expr(r, base));
            let method = r.s();
            let args = r.seq(base, get_expr);
            Expr::Dispatch { receiver, method, args, id: r.node(base) }
        }
        30 => {
            let record = Box::new(get_expr(r, base));
            let field = r.s();
            Expr::OptionalField { record, field, id: r.node(base) }
        }
        31 => {
            let record = Box::new(get_expr(r, base));
            let field = r.s();
            Expr::FieldAccess { record, field, id: r.node(base) }
        }
        32 => {
            let name = r.s();
            let args = r.seq(base, get_expr);
            Expr::Tag { name, args, id: r.node(base) }
        }
        other => unreachable_tag("Expr", other),
    }

}

/// A tag the writer cannot produce. Only a corrupt or mismatched artifact reaches this,
/// and `build.rs` rejects a mismatched one before the binary exists.
fn unreachable_tag(what: &str, tag: u8) -> ! {
    panic!("builtin artifact: {} has no variant {} — regenerate it", what, tag)
}

// ---------------------------------------------------------------- members

/// One member, as `builtin::load` wants it back.
pub struct Member {
    pub name: &'static str,
    pub ast: Expr,
    pub intrinsics: Vec<&'static str>,
    pub signatures: Vec<(&'static str, Type)>,
    pub nominals: Vec<(&'static str, Type)>,
}

/// Write one member. `base` is the id of its first node; `offsets` is every node's
/// source offset, in id order, so loading can rebuild the table.
pub fn put_member(
    w: &mut Writer,
    name: &str,
    base: u32,
    offsets: &[u32],
    ast: &Expr,
    intrinsics: &[&'static str],
    signatures: &[(&'static str, Type)],
    nominals: &[(&'static str, Type)],
) {
    w.s(name);
    // A fixed-width length, patched once the body is written, so opening the artifact
    // can step over a member it was not asked for instead of decoding it. Without this
    // the header decoded all eight trees and threw them away — 0.7ms, most of what the
    // artifact saves.
    let length_at = w.out.len();
    w.out.extend_from_slice(&0u32.to_le_bytes());
    let body_at = w.out.len();
    w.u(offsets.len() as u64);
    for offset in offsets {
        w.u(u64::from(*offset));
    }
    put_expr(w, ast, base);
    w.seq(intrinsics, |w, s| w.s(s));
    w.seq(signatures, |w, (n, t)| {
        w.s(n);
        put_type(w, t);
    });
    w.seq(nominals, |w, (n, t)| {
        w.s(n);
        put_type(w, t);
    });
    let length = (w.out.len() - body_at) as u32;
    w.out[length_at..length_at + 4].copy_from_slice(&length.to_le_bytes());
}

/// The artifact, decoded far enough to find each member. Strings are resolved once,
/// members are decoded only when asked for.
pub struct Artifact {
    strings: Vec<&'static str>,
    /// Where each member's body starts, by name.
    members: Vec<(&'static str, usize)>,
    blob: &'static [u8],
}

impl Artifact {
    /// Read the header: the string table, then each member's name and offset.
    ///
    /// The strings are slices of the blob, which lives in the binary — so this
    /// allocates one `Vec` of pointers and copies no text at all.
    pub fn open(blob: &'static [u8]) -> Option<Artifact> {
        if blob.len() < 20 || &blob[..8] != MAGIC {
            return None;
        }
        let count = u32::from_le_bytes(blob[16..20].try_into().ok()?) as usize;
        let mut head = Reader { blob, at: 20, strings: &[] };
        let n_strings = head.u() as usize;
        let mut lengths = Vec::with_capacity(n_strings);
        for _ in 0..n_strings {
            lengths.push(head.u() as usize);
        }
        // One validation of the whole run, then a slice per name.
        let total: usize = lengths.iter().sum();
        let text = std::str::from_utf8(blob.get(head.at..head.at + total)?).ok()?;
        let mut strings = Vec::with_capacity(n_strings);
        let mut at = 0;
        for len in lengths {
            strings.push(text.get(at..at + len)?);
            at += len;
        }
        let mut r = Reader { blob, at: head.at + total, strings: &strings };
        let mut members = Vec::with_capacity(count);
        for _ in 0..count {
            let name = r.s();
            let length = u32::from_le_bytes(blob.get(r.at..r.at + 4)?.try_into().ok()?) as usize;
            r.at += 4;
            // Where the member's own body begins. Opening the artifact reads NO tree:
            // it steps over each body by its length and decodes only what is asked for.
            members.push((name, r.at));
            r.at += length;
        }
        Some(Artifact { strings, members, blob })
    }

    pub fn has(&self, name: &str) -> bool {
        self.members.iter().any(|(n, _)| *n == name)
    }

    /// Decode one member, installing its nodes in the table first so the ids in the
    /// tree can be rebased onto them.
    pub fn member(&self, name: &str) -> Option<Member> {
        let (_, at) = self.members.iter().find(|(n, _)| *n == name)?;
        let mut r = Reader { blob: self.blob, at: *at, strings: &self.strings };
        let n_offsets = r.u() as usize;
        let mut offsets = Vec::with_capacity(n_offsets);
        for _ in 0..n_offsets {
            offsets.push(r.u() as u32);
        }
        let base = crate::ast::push_nodes(&offsets);
        Some(Member {
            name: self.members.iter().find(|(n, _)| *n == name)?.0,
            ast: get_expr(&mut r, base),
            intrinsics: r.seq(base, |r, _| r.s()),
            signatures: r.seq(base, |r, _| (r.s(), get_type(r))),
            nominals: r.seq(base, |r, _| (r.s(), get_type(r))),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one property that matters: what comes out is what went in. `Debug` is the
    /// comparison because `Expr` has no `PartialEq`, and it prints every field.
    #[test]
    fn a_member_round_trips() {
        let members = crate::builtin::parse_members(&["Box"]).expect("parse Box");
        let (loaded, base, offsets) = &members[0];
        let mut w = Writer::default();
        put_member(
            &mut w,
            loaded.name,
            *base,
            offsets,
            &loaded.ast,
            &loaded.intrinsics,
            &loaded.signatures,
            &loaded.nominals,
        );
        let blob: &'static [u8] = Box::leak(w.finish(0, 1).into_boxed_slice());
        let artifact = Artifact::open(blob).expect("open");
        let back = artifact.member("Box").expect("member");

        assert_eq!(back.intrinsics, loaded.intrinsics);
        assert_eq!(format!("{:?}", back.signatures), format!("{:?}", loaded.signatures));
        assert_eq!(format!("{:?}", back.nominals), format!("{:?}", loaded.nominals));

        // Every id moved by the same amount — that IS the rebase — so the trees are
        // compared with the numbers taken out, and the ids are checked separately by
        // what they point at.
        let before = blank_ids(&format!("{:?}", loaded.ast));
        let after = blank_ids(&format!("{:?}", back.ast));
        assert_eq!(before, after, "round trip changed the tree");
    }

    /// `NodeId(17)` -> `NodeId(_)`, so a rebased tree prints the same as its original.
    fn blank_ids(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(at) = rest.find("NodeId(") {
            out.push_str(&rest[..at + "NodeId(".len()]);
            rest = &rest[at + "NodeId(".len()..];
            let end = rest.find(')').unwrap_or(rest.len());
            out.push('_');
            rest = &rest[end..];
        }
        out.push_str(rest);
        out
    }

    /// The ids are only worth anything if they still point at the right offsets.
    #[test]
    fn a_members_node_offsets_survive() {
        let members = crate::builtin::parse_members(&["Box"]).expect("parse Box");
        let (loaded, base, offsets) = &members[0];
        let mut w = Writer::default();
        put_member(
            &mut w,
            loaded.name,
            *base,
            offsets,
            &loaded.ast,
            &loaded.intrinsics,
            &loaded.signatures,
            &loaded.nominals,
        );
        let blob: &'static [u8] = Box::leak(w.finish(0, 1).into_boxed_slice());
        let artifact = Artifact::open(blob).expect("open");
        let before = crate::ast::node_count() as u32;
        let _ = artifact.member("Box").expect("member");
        let after = crate::ast::node_count() as u32;
        assert_eq!((after - before) as usize, offsets.len(), "wrong number of nodes installed");
        assert_eq!(&crate::ast::offsets_between(before, after), offsets, "offsets differ");
    }
}
