# postcard-rpc

`postcard-rpc` is a fairly new crate that captures a lot of the "manual" or "bespoke" protocols I've
built on top of `postcard` over the past years.

First, let me define some concepts, as they are used by `postcard-rpc`:

## RPC, or Remote Procedure Call

RPC is a pattern for communication, often over a network, where one device wants another device
to do something. This "something" can be storing data we provide, retrieving some data, or doing
some more complicated operation.

This has a typical interaction pattern:

* The first device makes a **Request** to the second device
* The second device processes that **Request**, and sends a **Response**

With our microcontroller, this might look a little like this:

```text
PC ---Request--> MCU
                 ...
PC <--Response-- MCU
```

The reason this is called a "Remote Procedure Call" is because conceptually, we want this to
"feel like" a normal function call, and ignore the network entirely, and instead look like:

```rust

async fn procedure(Request) -> Response {
    // ...
}

```

Conceptually, this is similar to things like a REST request over the network, a GET or PUT request
might transfer data, or trigger some more complex operation.

## Endpoints

For any given kind of RPC, there will be a pair of Request and Response types that go with each
other. If the MCU could respond with one of a few kinds of responses, we can use an `enum` to
capture all of those.

But remember, we probably want to have multiple kinds of requests and responses that we support!

For that, we can define multiple `Endpoint`s, where each `Endpoint` refers to a single pair of
request and response types. We also want to add in a little unique information per endpoint, in
case we want to use the same types for multiple endpoints.

We might define an `Endpoint` in `postcard-rpc` like this:

```rust

#[derive(Debug, PartialEq, Serialize, Deserialize, Schema)]
pub struct Sleep {
    pub seconds: u32,
    pub micros: u32,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Schema)]
pub struct SleepDone {
    pub slept_for: Sleep,
}

endpoint!(
    SleepEndpoint,  // This is the NAME of the Endpoint
    Sleep,          // This is the Request type
    SleepDone,      // This is the Response type
    "sleep",        // This is the "path" of the endpoint
);


```

These endpoints will be defined in some shared library crate between our MCU and our PC.

## Unsolicited messages

Although many problems can be solved using a Request/Response pattern, it is also common to send
"unsolicited" messages. Two common cases are "streaming" and "notifications".

"Streaming" is relevant when you are sending a LOT of messages, for example sending continuous
sendor readings, and where making one request for every response would add a lot of overhead.

"Notifications" are relevant when you are RARELY sending messages, but don't want to constantly
"poll" for a result.

## Topics

`postcard-rpc` also allows for this in either direction, referred to as `Topic`s. The name `Topic`
is inspired by MQTT, which is used for publish and subscribe (or "pubsub") style data transfers.

We might define a `Topic` in `postcard-rpc` like this:

```rust
#[derive(Debug, PartialEq, Serialize, Deserialize, Schema)]
pub struct AccelReading {
    pub x: i16,
    pub y: i16,
    pub z: i16,
}

topic!(
    AccelTopic,     // This is the NAME of the Topic
    AccelReading,   // This is the Topic type
    "acceleration", // This is the "path" of the topic
);
```

## `postcard-rpc` Messages

Now that we have our three kinds of messages:

* Endpoint Requests
* Endpoint Responses
* Topic Messages

How does `postcard-rpc` help us determine which is which?

At the front of every message, we add a header with two fields:

1. a Key, explained below
2. a Sequence Number (a `u32`)

### Keys

Since `postcard` doesn't describe the types of the messages that it is sending, and we don't want
to send a lot of extra data for every message, AND we don't want to manually define all the
different unique IDs for every message kind, instead `postcard-rpc` automatically and
deterministically generates IDs using two pieces of information:

1. The Schema of the message type
2. The "path" string of the endpoint

So from our examples before:

```
SleepEndpoint::Request::Key  = hash("sleep") + hash(schema(Sleep));
SleepEndpoint::Response::Key = hash("sleep") + hash(schema(SleepDone));
AccelTopic::Message::Key     = hash("acceleration") + hash(schema(AccelReading));
```

As of now, keys boil down to an 8-byte value, calculated at compile time as a constant.

This is important for two reasons:

1. It gives us a "unique" ID for every kind of request and response
2. If the contents of the request or response changes, so does the key! This means that we never
   have to worry about the issue of one of the devices changing a message's type, and
   misinterpreting the data (though it means we can't 'partially understand' messages that have
   changed in a small way).

### Sequence Numbers

Since we might have multiple requests "In Flight" at one time, we use an incrementing sequence
number to each request. This lets us tell which response goes with each request, even if they
arrive out of order.

For example:

```text
PC ---Request 1-->-----.       MCU
   ---Request 2-->-----|--.
   ---Request 3-->--.  |  |
                    |  |  |
   <--Response 3----'  |  |
   <--Response 1-------'  |
   <--Response 2----------'
```

Even though our responses come back in a different order, we can still tell which responses went
with each request.

## Putting it all together

We've now added one "logical" layer to our stack, the postcard-rpc protocol!

Remember our old diagram:

```text
 ┌──────┐                                     ┌──────┐
┌┤  PC  ├────────────────────┐               ┌┤Target├────────────────────┐
│└──────┘                    │               │└──────┘                    │
│      Rust Application      │               │      Rust Application      │
│            Host            │◁ ─ ─ ─ ─ ─ ─ ▷│           Target           │
│                            │               │                            │
├────────────────────────────┤               ├────────────────────────────┤
│         NUSB crate         │◁ ─ ─ ─ ─ ─ ─ ▷│     embassy-usb crate      │
├────────────────────────────┤               ├────────────────────────────┤
│ Operating System + Drivers │◁ ─ ─ ─ ─ ─ ─ ▷│     embassy-rp drivers     │
├────────────────────────────┤               ├────────────────────────────┤
│        USB Hardware        │◀─────USB─────▶│        USB Hardware        │
└────────────────────────────┘               └────────────────────────────┘
```

Now it looks something like this:

```text
 ┌──────┐                                     ┌──────┐
┌┤  PC  ├────────────────────┐               ┌┤Target├────────────────────┐
│└──────┘                    │               │└──────┘                    │
│      Rust Application      │               │      Rust Application      │
│            Host            │◁ ─ ─ ─ ─ ─ ─ ▷│           Target           │
│                            │               │                            │
├────────────────────────────┤               ├────────────────────────────┤
│  postcard-rpc host client  │◁ ─ ─ ─ ─ ─ ─ ▷│ postcard-rpc target server │
├────────────────────────────┤               ├────────────────────────────┤
│         NUSB crate         │◁ ─ ─ ─ ─ ─ ─ ▷│     embassy-usb crate      │
├────────────────────────────┤               ├────────────────────────────┤
│ Operating System + Drivers │◁ ─ ─ ─ ─ ─ ─ ▷│     embassy-rp drivers     │
├────────────────────────────┤               ├────────────────────────────┤
│        USB Hardware        │◀─────USB─────▶│        USB Hardware        │
└────────────────────────────┘               └────────────────────────────┘
```

That's enough theory for now, let's start applying it to our firmware to get messages back and
forth!
