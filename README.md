# Gopal - Music Listening Tracker

A daemon-like Rust application that monitors media playback activity across Linux desktop using the MPRIS standard. It tracks play, pause, stop, and track change events, logging detailed listening sessions to a SQLite database.

## Architecture

The project consists of two main components:

1. **`gopald`** - The monitoring daemon that tracks MPRIS events
2. **`gopal-cli`** - A CLI tool for querying listening statistics

## Installation

### Quick Installation (Recommended)

```bash
# Clone the repository
git clone git@github.com:arpandaze/gopal.git
cd gopal

# Install everything with one command (requires just)
just install
```

This will build in release mode, install binaries, set up the systemd service, and start the daemon.

### Manual Installation

```bash
# Build the project
cargo build --release

# Install binaries
cargo install --path .

# Set up systemd service manually
cp gopald.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable gopald.service
systemctl --user start gopald.service
```

### Development Setup

```bash
# Install just (if not already installed)
cargo install just

# Install bacon for auto-restart during development
cargo install bacon

# Start development environment
just dev
```

## Usage

### Starting the Daemon

#### Manual Start (Foreground)
```bash
gopald --foreground --verbose
```

#### As a Systemd User Service (Recommended)

1. Copy the service file:
```bash
cp gopald.service ~/.config/systemd/user/
```

2. Enable and start the service:
```bash
systemctl --user daemon-reload
systemctl --user enable gopald.service
systemctl --user start gopald.service
```

3. Check service status:
```bash
systemctl --user status gopald.service
```

4. View logs:
```bash
journalctl --user -u gopald.service -f
```

## Supported Media Players

Any application that implements the MPRIS D-Bus interface, including:

- Spotify
- VLC Media Player
- Rhythmbox
- Amarok
- Clementine
- MPV (with MPRIS plugin)
- Firefox (for web audio)
- Chrome/Chromium (for web audio)
- And many more...

## License

This project is licensed under the MIT License - see the LICENSE file for details.
