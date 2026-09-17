//! The platform's hosted effects, by the member that names them.
//!
//! `Host.roc` declares `stdin_bytes! : () => Try(List(U8), …)` with no body, and the
//! platform's `hosted { "hosted_stdin_bytes": Host.stdin_bytes!, … }` says which C
//! symbol implements it. Loading the platform registers each pair here, with the
//! member's signature laid out; the VM's builtin dispatcher asks here before giving
//! up on a qualified name it has no code for.
//!
//! WHO makes the call is installed separately, and only by `host/` — the crate
//! linked into the platform's host, which is the only place the symbols exist. In a
//! plain `rocflight` process nothing is installed, and reaching an effect is an error
//! that says so rather than a silent no-op.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use super::abi::Signature;
use crate::eval::value::Value;

/// A hosted function: the symbol the host defines, and how to call it.
#[derive(Debug, Clone)]
pub struct Hosted {
    pub symbol: String,
    pub signature: Signature,
}

/// Makes the call: `(symbol, signature, args)`. `host/` installs this.
pub type Caller = fn(&str, &Signature, &[Value]) -> Result<Value, String>;

static REGISTRY: Mutex<Option<HashMap<String, Hosted>>> = Mutex::new(None);
static CALLER: OnceLock<Caller> = OnceLock::new();

/// Register `Module.member!` as `symbol`, called with `signature`.
pub fn register(member: &str, symbol: &str, signature: Signature) {
    let mut registry = REGISTRY.lock().expect("hosted registry");
    registry
        .get_or_insert_with(HashMap::new)
        .insert(member.to_string(), Hosted { symbol: symbol.to_string(), signature });
}

/// Install the one function that reaches the host. First caller wins.
pub fn install(caller: Caller) {
    let _ = CALLER.set(caller);
}

/// Is `Module.member` a registered effect?
pub fn is_registered(module: &str, member: &str) -> bool {
    let registry = REGISTRY.lock().expect("hosted registry");
    registry.as_ref().is_some_and(|r| r.contains_key(&format!("{}.{}", module, member)))
}

/// Call `Module.member` if it is a hosted effect; `None` if it is not one at all.
pub fn call(module: &str, member: &str, args: &[Value]) -> Option<Result<Value, String>> {
    let hosted = {
        let registry = REGISTRY.lock().expect("hosted registry");
        registry.as_ref()?.get(&format!("{}.{}", module, member))?.clone()
    };
    Some(match CALLER.get() {
        Some(caller) => caller(&hosted.symbol, &hosted.signature, args),
        None => Err(format!(
            "`{}.{}` is an effect of the platform's host (`{}`); it runs when rocflight is \
             linked into that host — `rocflight file.roc` — not in-process",
            module, member, hosted.symbol
        )),
    })
}
