//! The sample crate's first doc line.
//! The sample crate's second doc line.
//!
//! (These files are only TEXT for park-bundle's tests to glue. They are never compiled.)

mod a;
/// Documentation on a public module.
pub mod b;

#[cfg(test)]
mod tests {
    use crate::a::hello;

    #[test]
    fn hello_says_hello() {
        assert_eq!(hello(), "hello");
    }
}

macro_rules! twice {
    ($x:expr) => {
        $crate::b::double($x)
    };
}

pub fn top() -> &'static str {
    crate::a::hello()
}

pub fn from_another_crate() {
    my_crate::something();
}

// A comment may talk about forbidden things: env!("X"), #![allow(x)], include!("y").
