//! OpenViKey Lab CLI harness.

fn main() {
    if let Err(error) = openvikey_lab::cli::run() {
        eprintln!("Error: {error}");
        std::process::exit(1);
    }
}
