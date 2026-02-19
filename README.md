<p align="center">
  <img src="logo.svg" alt="Session" width="150"/>
</p>
<h1 align="center">session-rs</h1>
<p align="center">A lightweight, async WebSocket protocol for Rust.</p>

## Introduction

This library provides **type-safe WebSocket communication** with a request-response and notification system built on top of a flexible protocol.  
It ensures compile-time guarantees for message structure, reduces runtime errors, and simplifies building Rust client/server applications.

- **Dynamic Methods**: Each message includes a method enum for type safety.
- **Typed Requests & Responses**: Automatic serialization and deserialization.
- **Optional Notifications**: Send asynchronous notifications across sessions.

## Features

- Fully typed WebSocket sessions
- Type-safe request/response mechanism
- Optional typed notifications (Todo)
- Lightweight, minimal runtime overhead
- Async-first with Tokio support

## Installation

```bash
cargo add session-rs
```

---

### **Basic Example (client)**

```rust
#[derive(Debug, Serialize, Deserialize)]
struct Data;

impl Method for Data {
    const NAME: &'static str = "data";
    type Request = String;
    type Response = String;
    type Error = String;
}

let session = Session::connect("127.0.0.1:8080", "/").await?;

session.start_receiver();

session
    .request::<Data>("Hello from client".to_string())
    .await?;
```

### **Basic Example (server)**

```rust
#[derive(Debug, Serialize, Deserialize)]
struct Data;

impl Method for Data {
    const NAME: &'static str = "data";
    type Request = String;
    type Response = String;
    type Error = String;
}

let server = SessionServer::bind("127.0.0.1:8080").await?;

loop {
    let session = server.accept().await;

    session
        .on::<Data, _>(async |_, req| {
            // Response is required
            Ok("Hello from server".to_string())
        })
        .await;
}
```
