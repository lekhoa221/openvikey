//! OpenViKey Lab CLI harness.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    openvikey_lab::cli::run()
}
