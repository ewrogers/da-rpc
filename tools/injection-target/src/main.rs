//! Inert process used for safe loader integration testing.

use std::{
    io::{self, Write},
    process,
};

fn main() -> io::Result<()> {
    println!("Injection target ready: pid={}", process::id());

    print!("Press enter to exit...");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    Ok(())
}
