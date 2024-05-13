# Postcard

We could use any sort of wire format, like JSON or HTTP. However our microcontroller is small, and
we want a protocol that will work well for both devices.

For this workshop, we'll use a format called [`postcard`]. It's a compact binary format, built on
top of the [`serde`] crate. It supports all Rust types, including primitives, structs, and enums.

[`postcard`]: https://docs.rs/postcard
[`serde`]: https://serde.rs

We can define a type like this:

```rust
#[derive(Serialize, Deserialize)]
pub struct AccelReading {
    pub x: i16,
    pub y: i16,
    pub z: i16,
}
```

If we were to serialize a value like this:

```rust
AccelReading { x: 63, y: -1, z: -32768 }
```

We'd end up with a value like this:

```rust
[
    0x7E,             // 63
    0x01,             // -1
    0xFF, 0xFF, 0x03, // -32768
]
```

We don't have to worry exactly WHY it looks like this, though you could look at the
[postcard specification] if you wanted, but the main take aways are:

[postcard specification]: https://postcard.jamesmunns.com/wire-format#signed-integer-encoding

* This is a "non self describing format": The messages don't describe the *type* at all, only the
  values. This means we send less data on the wire, but both sides have to understand what data they
  are looking at.
* The message is fairly compact, it only takes us 5 bytes to send data that takes 6 bytes in memory

Since `postcard` works on both desktop and `no_std` targets, we don't need to do anything extra
to define how to turn Rust data types into bytes, and how to turn bytes into data types.

## Still something missing

We could go off running, just sending postcard encoded data back and forth over USB, but there's
two problems we'll run into quickly:

1. We probably will want to send different KINDS of messages. How does each device tell each other
   what type of message this is, and how to interpret them?
2. How do we define how each side behaves? How can one device request something from the other, and
   know how to interpret that response?

At the end of the day, postcard is just an *encoding*, not a *protocol*. You could build something
on top of postcard to describe a protocol, and that's what `postcard-rpc` is!
