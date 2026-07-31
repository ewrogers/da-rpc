//! Inert process used for safe loader integration testing.

use std::{
    env,
    io::{self, Write},
    process, thread,
    time::Duration,
};

fn main() -> io::Result<()> {
    println!("Injection target ready: pid={}", process::id());
    io::stdout().flush()?;

    match parse_wait()? {
        Wait::Input => {
            print!("Press enter to exit...");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
        }
        Wait::Duration(duration) => thread::sleep(duration),
    }

    Ok(())
}

enum Wait {
    Input,
    Duration(Duration),
}

fn parse_wait() -> io::Result<Wait> {
    let mut arguments = env::args_os().skip(1);

    let Some(option) = arguments.next() else {
        return Ok(Wait::Input);
    };

    if option != "--wait-ms" {
        return Err(invalid_arguments());
    }

    let milliseconds = arguments
        .next()
        .and_then(|value| value.into_string().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(invalid_arguments)?;

    if arguments.next().is_some() {
        return Err(invalid_arguments());
    }

    Ok(Wait::Duration(Duration::from_millis(milliseconds)))
}

fn invalid_arguments() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "usage: injection-target [--wait-ms <milliseconds>]",
    )
}
