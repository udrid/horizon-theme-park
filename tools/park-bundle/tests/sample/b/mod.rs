//! Module b: found at b/mod.rs, and it has a child module of its own.

#[allow(dead_code)]
mod c;

pub fn double(x: i32) -> i32 {
    crate::b::c::twice(x)
}
