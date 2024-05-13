# Establishing Comms

So far we've experimented with the board, but we want to get the host PC involved!

We'll want to talk to our device over USB. So far, we've interacted with our device like this:

## So Far

```text
            ┌─────────────┐
            │             │
            │     PC      │
            │             │
            └─────────────┘
                   ▲
                   │ USB
                   ▼
            ┌─────────────┐
            │             │
            │   USB Hub   │
            │             │
            └─────────────┘
            ▲
      ┌─USB─┘
      ▼
┌───────────┐             ┌───────────┐
│    MCU    │             │    MCU    │
│  (debug)  │─────SWD────▶│ (target)  │
└───────────┘             └───────────┘
```

## What we Want

We'll want to enable USB on the target device, so then our diagram looks like this:

```text
            ┌─────────────┐
            │             │
            │     PC      │
            │             │
            └─────────────┘
                   ▲
                   │ USB
                   ▼
            ┌─────────────┐
            │             │
            │   USB Hub   │
            │             │
            └─────────────┘
            ▲             ▲
      ┌─USB─┘             └─USB─┐
      ▼                         ▼
┌───────────┐             ┌───────────┐
│    MCU    │             │    MCU    │
│  (debug)  │─────SWD────▶│ (target)  │
└───────────┘             └───────────┘
```

## Zooming in

Ignoring the USB hub and the debug MCU, we'll have something a little like this:

```text
┌────────────┐                 ┌───────────┐
│     PC     │                 │    MCU    │
│            │◀──────USB──────▶│ (target)  │
└────────────┘                 └───────────┘
```

This slides over a lot of detail though! Let's look at it with a little bit more detail:

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

### Host Side

On the host side, there's a couple of main pieces. We'll have our application, which needs to
interact with devices in some way. We'll use the [`nusb` crate], an async friendly library that
manages high level USB interactions. `nusb` manages the interactions with your operating system,
which in turn has drivers for your specific USB hardware.

[`nusb` crate]: https://docs.rs/nusb/

### Target Side

Conversely on the target side, things can be a little more diverse depending on the software and
hardware we are using, but in the case of Embassy on the RP2040, your application will interact
with interfaces from the `embassy-usb` crate which describe USB capabilities in a portable and async
way. These capabilities are provided by the USB drivers provided by the `embassy-rp` HAL, which
manages the low level hardware interactions.

### Working Together

At each of these layers, we can conceptually think of each "part" of the PC and Target talking to
each other:

* The RP2040 USB hardware talks to the PC USB hardware at a physical and electrical level
* Your operating system and drivers talk to the embassy-rp drivers, to exchange messages with each
  other
* The `nusb` crate talks to `embassy-usb` to exchange messages, such as USB "Bulk" frames
* Your PC application will want to talk to the Firmware application, using some sort of protocol

If you come from a networking background, this will look very familiar to the OSI or TCP/IP model,
where we have different layers with different responsibilities.

USB is a complex topic, and we won't get too deep into it! For the purposes of today's exercise,
we'll focus on USB Bulk Endpoints. These work somewhat similar to "UDP Frames" from networking:

* Each device can send and receive "Bulk transfers"
* "Bulk transfers" are variable sized, and framed messages. Sort of like sending `[u8]` slices
  to each other
* Each device can send messages whenever they feel like, though the PC is "in charge", it decides
  when it gives messages to the device, and when it accepts messages from the device

There are many other ways that USB can work, and a lot of details we are skipping. We aren't using
"standard" USB definitions, like you might use for a USB keyboard, serial ports, or a MIDI device.

Instead, we are using "raw" USB Bulk frames, like you might do if you were writing a proprietary
device driver for your device.

### Something to be desired

Although the stack we've looked at so far handles all of the "transport", we're lacking any sort
of high level protocol! We can shove chunks of bytes back and forth, but what SHOULD we send, and
when? This is a little like having a raw TCP connection, but no HTTP to "talk" at a higher level!

We'll need to define two main things:

* How to interpret our messages, e.g some kind of "wire format"
* Some sort of protocol, e.g. "how we behave" when talking to each other
