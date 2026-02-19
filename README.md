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

server
    .session_loop(async |session, addr| {
        // This will run on every new client

        Ok(())
    }).await;
```

## Protocol

#### Request

The request `id` is separated from the peer, and will increment only on it's requests.

```json
{ "type": "request", "id": 1, "method": "data", "data": "Hello from client" }
```

#### Response

The response `id` **must** remain the same as the request.

```json
{ "type": "response", "id": 1, "result": "Hello from server" }
```

#### Notifications

A notification is a method that doesn't need validation or output, it simply notifies a peer for a specific information

```json
{ "type": "notification", "result": "Hello from server" }
```
