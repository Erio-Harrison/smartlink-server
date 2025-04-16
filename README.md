# SmartLink WebSocket Server

## Overview

SmartLink WebSocket Server is a real-time communication server built in Rust using the Tokio and Tungstenite libraries. It supports features like user status updates, private and group messaging, WebRTC signaling, typing indicators, and social media-style moment interactions (e.g., likes, comments, updates, and deletions). The server is designed for scalability and thread safety, utilizing asynchronous programming and a connection management system to handle user connections efficiently.

## Features

- **User Status Management**: Tracks online/offline status and broadcasts updates to relevant users.
- **Real-Time Messaging**: Supports private and group conversations with message status updates (e.g., read receipts).
- **WebRTC Signaling**: Facilitates peer-to-peer video/audio call setup.
- **Typing Indicators**: Notifies users in real-time when someone is typing.
- **Moments System**: Allows users to publish, like, comment, update, or delete moments, with notifications sent to relevant friends or viewers.
- **Connection Cleanup**: Automatically removes inactive connections after a timeout period.
- **Thread-Safe Design**: Uses `Arc` and `RwLock` for safe concurrent access to shared state.

## Prerequisites

- Rust (stable, latest version recommended)
- Cargo (Rust's package manager)
- A compatible MongoDB instance (if integrating with a database, not included in this code)
- Environment variable `PORT` (optional, defaults to 8080)

## Installation

1. Clone the repository:

   ```bash
   git clone https://github.com/Erio-Harrison/smartlink-websocket-server.git
   cd smartlink-websocket-server
   ```

2. Ensure Rust is installed:

   ```bash
   rustup update
   ```

3. Build the project:

   ```bash
   cargo build --release
   ```

4. Run the server:

   ```bash
   cargo run --release
   ```

   Alternatively, set a custom port:

   ```bash
   PORT=8080 cargo run --release
   ```

## Usage

- The server listens for WebSocket connections at `ws://0.0.0.0:8080` (or the specified `PORT`).
- Clients must send an initial `UserStatus` message with a `sender_id` to authenticate and establish a connection.
- Supported message types include `NewMessage`, `MessageStatusUpdate`, `WebRTCSignal`, `TypingIndicator`, `NewMoment`, `MomentLike`, `MomentComment`, `MomentUpdate`, and `MomentDelete`.
- Messages are serialized in JSON with camelCase naming conventions (enforced by `serde`).

## Configuration

- **Port**: Set via the `PORT` environment variable (default: 8080).
- **Logging**: Uses the `tracing` crate for structured logging. Configure via `tracing_subscriber` in the code.
- **Timeout**: Inactive connections are cleaned up every 30 seconds if no ping is received within 60 seconds.

## Project Structure

- `src/main.rs`: Core server logic, including WebSocket handling, connection management, and message processing.
- **Dependencies** (in `Cargo.toml`):
  - `tokio = { version = "1.25.0", features = ["full"] }`: Asynchronous runtime
  - `tokio-tungstenite = "0.19.0"`: WebSocket support
  - `futures-util = "0.3.28"`: Utilities for asynchronous streams
  - `serde = { version = "1.0.163", features = ["derive"] }`: JSON serialization/deserialization
  - `serde_json = "1.0.96"`: JSON parsing and serialization
  - `chrono = { version = "0.4.24", features = ["serde"] }`: Timestamp handling
  - `tracing = "0.1.37"`: Logging framework
  - `tracing-subscriber = "0.3.17"`: Logging subscriber for formatting

## Contributing

Contributions are welcome! Please follow these steps:

1. Fork the repository.
2. Create a feature branch (`git checkout -b feature/your-feature`).
3. Commit your changes (`git commit -m "Add your feature"`).
4. Push to the branch (`git push origin feature/your-feature`).
5. Open a pull request.

## License

This project is licensed under the MIT License. See the LICENSE file for details.

## Author

- **Name**: Harrison
- **Email**: u7541840@gmail.com
- **GitHub**: [Erio-Harrison](https://github.com/Erio-Harrison)

## Acknowledgments

- Built with Rust and the Tokio ecosystem*.
- Inspired by real-time communication needs for the SmartLink project.