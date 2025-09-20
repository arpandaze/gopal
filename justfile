# Justfile for Gopal Music Tracker
# Run `just --list` to see all available commands

# Default recipe
default:
    @just --list

# Development Commands
# ===================

# Start daemon in development mode with auto-restart
dev:
    @echo "Starting gopald in development mode with bacon auto-restart..."
    @echo "Using development database: ~/.local/share/musicd/music_dev.db"
    RUST_LOG=info bacon -- --bin gopald -- --database ~/.local/share/musicd/music_dev.db --foreground --verbose

# Start daemon once in development mode (no auto-restart)
dev-once:
    @echo "Starting gopald in development mode (single run)..."
    @echo "Using development database: ~/.local/share/musicd/music_dev.db"
    RUST_LOG=info cargo run --bin gopald -- --database ~/.local/share/musicd/music_dev.db --foreground --verbose

# Reset development database
reset-db:
    @echo "Resetting development database..."
    rm -f ~/.local/share/musicd/music_dev.db
    @echo "Development database deleted."

# Query development database
dev-stats:
    @echo "Development database stats:"
    cargo run --bin gopal-cli -- --database ~/.local/share/musicd/music_dev.db stats --period today

dev-history:
    @echo "Development database history:"
    cargo run --bin gopal-cli -- --database ~/.local/share/musicd/music_dev.db history --period today

dev-status:
    @echo "Development database status:"
    cargo run --bin gopal-cli -- --database ~/.local/share/musicd/music_dev.db status

# Build Commands
# ==============

# Build in debug mode
build:
    cargo build

# Build in release mode
build-release:
    cargo build --release

# Run tests
test:
    cargo test

# Check code without building
check:
    cargo check

# Format code
fmt:
    cargo fmt

# Run clippy linter
lint:
    cargo clippy

# Production Commands
# ==================

# Install binaries and set up systemd service
install: build-release
    @echo "Installing gopald to production..."
    
    # Install binaries to system location
    sudo cp target/release/gopald /usr/local/bin/
    sudo cp target/release/gopal-cli /usr/local/bin/
    sudo chmod +x /usr/local/bin/gopald
    sudo chmod +x /usr/local/bin/gopal-cli
    @echo "✓ Binaries installed to /usr/local/bin/"
    
    # Create necessary directories
    mkdir -p ~/.local/share/musicd
    mkdir -p ~/.config/musicd
    @echo "✓ Created application directories"
    
    # Copy systemd service file
    mkdir -p ~/.config/systemd/user
    cp gopald.service ~/.config/systemd/user/
    @echo "✓ Systemd service file copied"
    
    # Reload systemd and enable service
    systemctl --user daemon-reload
    systemctl --user enable gopald.service
    @echo "✓ Systemd service enabled"
    
    # Start the service
    systemctl --user start gopald.service
    @echo "✓ Gopald service started"
    
    @echo ""
    @echo "🎵 Installation complete!"
    @echo "Check status: systemctl --user status gopald.service"
    @echo "View logs:    journalctl --user -u gopald.service -f"
    @echo "Query stats:  gopal-cli stats"

# Stop and disable the systemd service
uninstall:
    @echo "Uninstalling gopald..."
    
    # Stop and disable service
    -systemctl --user stop gopald.service
    -systemctl --user disable gopald.service
    @echo "✓ Systemd service stopped and disabled"
    
    # Remove service file
    rm -f ~/.config/systemd/user/gopald.service
    systemctl --user daemon-reload
    @echo "✓ Service file removed"
    
    # Remove binaries
    sudo rm -f /usr/local/bin/gopald
    sudo rm -f /usr/local/bin/gopal-cli
    @echo "✓ Binaries removed from /usr/local/bin/"
    
    @echo "Note: Database remains at ~/.local/share/musicd/ (remove manually if needed)"

# Service management
# ==================

# Start the systemd service
start:
    systemctl --user start gopald.service
    @echo "✓ Gopald service started"

# Stop the systemd service
stop:
    systemctl --user stop gopald.service
    @echo "✓ Gopald service stopped"

# Restart the systemd service
restart:
    systemctl --user restart gopald.service
    @echo "✓ Gopald service restarted"

# Check service status
status:
    systemctl --user status gopald.service

# View service logs
logs:
    journalctl --user -u gopald.service -f

# Query Commands (Production)
# ===========================

# Show today's listening statistics
stats:
    gopal-cli stats --period today

# Show listening history
history:
    gopal-cli history --period today

# Show top tracks this week
top-tracks:
    gopal-cli top-tracks --period week --limit 20

# Show top artists this week
top-artists:
    gopal-cli top-artists --period week --limit 10

# Show database status
db-status:
    gopal-cli status

# Utility Commands
# ================

# Clean build artifacts
clean:
    cargo clean

# Update dependencies
update:
    cargo update

# Generate documentation
docs:
    cargo doc --open

# Run all checks (test, lint, format)
ci: test lint fmt
    @echo "✓ All checks passed"

# Debug MPRIS players
debug-mpris:
    python3 debug_mpris.py

# Development workflow
# ===================

# Full development setup
dev-setup: reset-db
    @echo "Setting up development environment..."
    @echo "1. Database reset: ✓"
    @echo "2. Starting daemon with auto-restart..."
    @just dev

# Quick development cycle
dev-cycle: build dev-once

# Check development progress
dev-check: dev-stats dev-history

# Help
# ====

# Show detailed help
help:
    @echo "Gopal Music Tracker - Development & Deployment Commands"
    @echo ""
    @echo "🔧 Development:"
    @echo "  just dev          - Start daemon with bacon auto-restart (recommended)"
    @echo "  just dev-once     - Start daemon once (no auto-restart)"
    @echo "  just reset-db     - Reset development database"
    @echo "  just dev-stats    - Show development database stats"
    @echo "  just dev-history  - Show development database history"
    @echo ""
    @echo "🚀 Production:"
    @echo "  just install      - Build, install, and start systemd service"
    @echo "  just uninstall    - Stop and remove systemd service"
    @echo "  just start/stop   - Control systemd service"
    @echo "  just logs         - View service logs"
    @echo ""
    @echo "📊 Queries:"
    @echo "  just stats        - Today's listening statistics"
    @echo "  just history      - Today's listening history"
    @echo "  just top-tracks   - Top tracks this week"
    @echo ""
    @echo "🛠️  Development Tools:"
    @echo "  just test         - Run tests"
    @echo "  just lint         - Run clippy"
    @echo "  just fmt          - Format code"
    @echo "  just debug-mpris  - Debug MPRIS players"