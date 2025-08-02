# `postcard-rpc` overview

The goal of `postcard-rpc` is to make it easier for a host PC to talk to a constrained device, like a microcontroller.

In many cases, it is useful to have a microcontroller handling real time operations, like reading sensors or controlling motors; while the "big picture" tasks are handled by a PC.

## Remote Procedure Calls

One way of achieving this is to use a "Remote Procedure Call" (RPC) approach, where:

* The PC sends a **Request** message, asking the MCU to do something, and waits for a **Response** message.
* The MCU receives this Request, and performs the action
* The MCU sends the response, and the PC recieves it.

In essence, we want to make this:

```
PC ---Request--> MCU
                 ...
PC <--Response-- MCU
```

look like this:

```rust
async fn request() -> Response { ... }
```

## How does this relate to `postcard`?

[`postcard`](https://postcard.jamesmunns.com) is a Rust crate for serializing and deserializing data. It has a couple of very relevant features:

* We can use it to define compact messages
* We can send those messages as bytes across a number of different interfaces
* We can use it on very constrained devices, like microcontrollers, as the messages are small and relatively "cheap" to serialize and deserialize

## What does this add on top of postcard?

`postcard-rpc` adds a major feature to `postcard` formatted messages: a standard header containing two things:

* an eight byte, unique `Key`
* a `varint(u32)` "sequence number"

### The `Key`

The `Key` uniquely identifies what "kind" of message this is. In order to generate it, `postcard-rpc` takes two pieces of data:

* a `&str` "path" URI, similar to how you would use URIs as part of an HTTP path
* The schema of the message type itself, using the experimental [schema] feature of `postcard`.

[schema]: https://docs.rs/postcard/latest/postcard/experimental/index.html#message-schema-generation

Let's say we had a message type like:

```rust
struct SetLight {
    r: u8,
    g: u8,
    b: u8,
    idx: u16,
}
```

and we wanted to map it to the path `lights/set_rgb`.

Both the schema and the path will take many more than eight bytes to describe, so instead we *hash* the two pieces of data in a deterministic way, to produce a value like `0x482c55743ba118e1`.

Specifically, we use [`FNV1a`](https://en.wikipedia.org/wiki/Fowler%E2%80%93Noll%E2%80%93Vo_hash_function), and produce a 64-bit digest, by first hashing the path, then hashing the schema. FNV1a is a non-cryptographic hash function, designed to be reasonably efficient to compute even on small platforms like microcontrollers.

Changing **anything** about *either* of the path or the schema will produce a drastically different `Key` value.

### The "sequence number"

Sometimes, we might want to have multiple requests "in flight" at once. Instead of this:

```
PC ---Request A-->. MCU
                  |
   <--Response A--'

   ---Request B-->.
                  |
   <--Response B--'

   ---Request C-->.
                  |
   <--Response C--'
```

We'd like to do this:

```
PC ---Request A-->--.       MCU
   ---Request B-->--|--.
   ---Request C-->--|--|--.
                    |  |  |
   <--Response A----'  |  |
   <--Response B-------'  |
   <--Response C----------'
```

Or if the requests take different amounts of time to process, even this:

```
PC ---Request A-->-----.       MCU
   ---Request B-->-----|--.
   ---Request C-->--.  |  |
                    |  |  |
   <--Response C----'  |  |
   <--Response A-------'  |
   <--Response B----------'
```

By adding a sequence number, we can uniquely match each response to the specific request, allowing for out of order processing of long requests.

## How you use this:

> NOTE: Check out the [example](https://github.com/jamesmunns/postcard-rpc/tree/main/example) folder for a project that follows these recommendations.

I'd suggest breaking up your project into three main crates:

* The `protocol` crate
    * This crate should depend on `postcard` and `postcard-rpc`
    * This crate is a `no_std` library project
    * This crate defines all the message types and paths used in the following crates
* The `firmware` crate
    * This crate should depend on `postcard`, `postcard-rpc`, `serde`, and your `protocol` crate.
    * This crate is a `no_std` binary project
* The `host` crate
    * This crate should depend on `postcard`, `postcard-rpc`, `serde`, and your `protocol` crate.
    * This crate is a `std` binary/library project

### The `protocol` crate

This part is pretty boring! You define some types, and make sure they derive (at least) the `Serialize` and `Deserialize` traits from `serde`, and the `Schema` trait from `postcard`.

```rust
// This is our path
pub const SLEEP_PATH: &str = "sleep";

// This is our Request type
#[derive(Serialize, Deserialize, Schema)]
pub struct Sleep {
    pub seconds: u32,
    pub micros: u32,
}

// This is our Response type
#[derive(Serialize, Deserialize, Schema)]
pub struct SleepDone {
    pub slept_for: Sleep,
}
```

### The `firmware` crate

In this part, you'll need to do a couple things:

1. Create a `Dispatch` struct. You'll need to define:
    * What your `Context` type is, this will be passed as a `&mut` ref to all handlers
    * What your `Error` type is - this is a type you can return from handlers if the message can not be processed
    * How many handlers max you can support
    * If you use `CobsDispatch`, you'll also need to define how many bytes to use for buffering COBS encoded messages.
2. Register each of your handlers. For each handler, you'll need to define:
    * The `Key` that should be used for the handler
    * a handler function
3. Feed messages into the `Dispatch`, which will call the handlers when a message matching that handler is found.

The handler functions have the following signature:

```rust
fn handler(
    hdr: &WireHeader,
    context: &mut Context,
    bytes: &[u8],
) -> Result<(), Error>;
```

The `hdr` is the decoded `Key` and `seq_no` of our message. We know that the `Key` matches our function already, but you could use the same handler for multiple `Key`s, so passing it allows you to check if you need to.

The `context` is a mutable reference to the Context type chosen when you create the `Dispatch` instance. It is recommended that you include whatever you need to send responses back to the PC in the `context` structure.

The `bytes` are the body of the request. You are expected to use `postcard::from_bytes` to decode the body to your specific message type.

Note that handlers are synchronous/blocking functions! However, you can still spawn async tasks from this context.

A typical handler might look something like this:

```rust
fn sleep_handler(
    hdr: &WireHeader,
    c: &mut Context,
    bytes: &[u8]
) -> Result<(), CommsError> {
    // Decode the body of the request
    let Ok(msg) = from_bytes::<Sleep>(bytes) else {
        // return an error if we can't decode the
        // message. Include the sequence number so
        // we can use that for our boilerplate "error"
        // response.
        return Err(CommsError::Postcard(hdr.seq_no))
    }

    // We have a message, attempt to spawn an embassy
    // task to handle this request. If we fail, return
    // an error with the sequence number so we can tell
    // the PC we couldn't serve the request
    //
    // Our context contains a Mutex'd sender that allows
    // the spawned task to send a reply.
    let new_c = c.clone();
    c.spawner
        .spawn(sleep_task(hdr.seq_no, new_c, msg))
        .map_err(|_| CommsError::Busy(hdr.seq_no))
}
```

The handler might call an embassy task that looks like this:

```rust
// A pool size of three means that we can handle three requests
// concurrently.
#[embassy_executor::task(pool_size = 3)]
async fn sleep_task(seq_no: u32, c: Context, s: Sleep) {
    info!("Sleep spawned");
    Timer::after(Duration::from_secs(s.seconds.into())).await;
    Timer::after(Duration::from_micros(s.micros.into())).await;
    info!("Sleep complete");

    // Try to send a response. If it fails, we are disconnected
    // so no sense in retrying. We reply with the pre-computed
    // reply key, and the sequence number of the request.
    let resp = SleepDone { slept_for: s };
    let _ = c.sender
        .lock()
        .await
        .send(seq_no, c.sleep_done_key, resp).await;
}
```

### The `host` crate

On the host side, the API is pretty simple:

1. We create a `HostClient` that establishes the serial link with our device
2. We make requests using the `HostClient`

```rust
// We create a client with:
//
// * A serial port path of "/dev/ttyUSB0"
// * An error path of "error"
// * An error type of `WireError`
let client = HostClient::<WireError>::new("/dev/ttyUSB0", "error")?;

// We make a request with:
//
// * A URI of "sleep"
// * A Request of type `Sleep`
// * A Response of type `SleepDone`
let resp: Result<SleepDone, WireError> = client
    .req_resp::<Sleep, SleepDone>("sleep").await;
```

#### Permissions on host

Note that depending on your operating system you might need to grant access to the device for non-privileged users.
For the provided example you can use the following [udev rules] on Linux:

```
# These rules are based on the udev rules from the OpenOCD + probe.rs projects
#
# This file is available under the GNU General Public License v2.0
#
# SETUP INSTRUCTIONS:
#
# 1. Copy/write/update this file to `/etc/udev/rules.d/60-postcard-rpc.rules`
# 2. Run `sudo udevadm control --reload` to ensure the new rules are used
# 3. Run `sudo udevadm trigger` to ensure the new rules are applied to already added devices.

# Default demos from postcard-rpc - 16c0:27dd
ACTION=="add|change", SUBSYSTEM=="usb|tty|hidraw", ATTRS{idVendor}=="16c0", ATTRS{idProduct}=="27dd", MODE="660", GROUP="plugdev", TAG+="uaccess"
```

[udev rules]: https://www.kernel.org/pub/linux/utils/kernel/hotplug/udev/udev.html
