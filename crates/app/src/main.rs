// The binary is the one place that talks to stdout.
#[allow(clippy::print_stdout)]
fn main() -> anyhow::Result<()> {
    println!("{}", greeting("world"));
    Ok(())
}

fn greeting(name: &str) -> String {
    format!("Hello, {name}!")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greeting_names_the_target() {
        assert_eq!(greeting("Rust"), "Hello, Rust!");
    }
}
