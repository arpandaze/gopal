//! Main entry point for the music tracker library
//! 
//! This file exists to satisfy Cargo's requirement for a main.rs file
//! when building a library crate. The actual binaries are in:
//! - src/daemon/main.rs (musicd daemon)
//! - src/cli/main.rs (music-cli query tool)

fn main() {
    eprintln!("This is a library crate. Use one of the following binaries:");
    eprintln!("  cargo run --bin gopald     # Start the music tracking daemon");
    eprintln!("  cargo run --bin gopal-cli  # Query listening statistics");
    eprintln!();
    eprintln!("Or install the binaries:");
    eprintln!("  cargo install --path .");
    eprintln!("  gopald --help");
    eprintln!("  gopal-cli --help");
    eprintln!();
    eprintln!("Or use the justfile commands:");
    eprintln!("  just install    # Install and start systemd service");
    eprintln!("  just dev        # Start development mode with auto-restart");
    eprintln!("  just stats      # View listening statistics");
    
    std::process::exit(1);
}
