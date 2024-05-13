# Back to the firmware

We can now take a look at the `comms-01` project, in the `firmware` folder.

We've taken away most of the driver code, and replaced it with the code we need to set up our
RP2040's `postcard-rpc` setup.

## Setup and Run

In our `main`, we've added this code:

```rust
let driver = usb::Driver::new(p.USB, Irqs);
let mut config = example_config();
config.manufacturer = Some("OneVariable");
config.product = Some("ov-twin");
let buffers = ALL_BUFFERS.take();
let (device, ep_in, ep_out) = configure_usb(driver, &mut buffers.usb_device, config);
let dispatch = Dispatcher::new(&mut buffers.tx_buf, ep_in, Context {});

spawner.must_spawn(dispatch_task(ep_out, dispatch, &mut buffers.rx_buf));
spawner.must_spawn(usb_task(device));
```

<hr>

Let's break this down piece by piece:

```rust
let driver = usb::Driver::new(p.USB, Irqs);
```

This line is straight out of `embassy-rp`, it just sets up the hardware and interrupt handlers
needed to manage the USB hardware at a low level. You would do this for any `embassy-rp` project
using USB.

<hr>

Next up, we handle some configuration:

```rust
let mut config = example_config();
config.manufacturer = Some("OneVariable");
config.product = Some("ov-twin");
```

`example_config()` is a function from the `postcard_rpc::target_server` module. This takes the
configuration structure provided by `embassy-usb`, and customizes it in a standard way. This
looks like this:

```rust
pub fn example_config() -> embassy_usb::Config<'static> {
    // Create embassy-usb Config
    let mut config = embassy_usb::Config::new(0x16c0, 0x27DD);
    config.manufacturer = Some("Embassy");
    config.product = Some("USB example");
    config.serial_number = Some("12345678");

    // Required for windows compatibility.
    // https://developer.nordicsemi.com/nRF_Connect_SDK/doc/1.9.1/kconfig/CONFIG_CDC_ACM_IAD.html#help
    config.device_class = 0xEF;
    config.device_sub_class = 0x02;
    config.device_protocol = 0x01;
    config.composite_with_iads = true;

    config
}
```

We then overwrite the `manufacturer` and `product` fields to something specific for our exercise.

<hr>

Then, we continue configuring the RP2040's USB hardware:

```rust
let buffers = ALL_BUFFERS.take();
let (device, ep_in, ep_out) = configure_usb(driver, &mut buffers.usb_device, config);
let dispatch = Dispatcher::new(&mut buffers.tx_buf, ep_in, Context {});
```

These lines do three things:

* We take some static data buffers that `postcard_rpc` needs for USB communication, as well as
  for serializing and deserializing messages.
* `configure_usb`, a function from `postcard_rpc::target_server` configures the USB:
    * It applies the `config` that we just prepared
    * It configures the low level drivers using the `embassy-usb` interfaces
    * It gives us back three things:
        * the `device`, which is a task that needs to be run to maintain the low level USB
          driver pieces
        * `ep_in`, our USB "Bulk Endpoint", in the In (to the PC) direction
        * `ep_out`, our USB "Bulk Endpoint", in the Out (to the MCU) direction
* We set up a `Dispatcher` (more on this below), giving it the buffers, the `ep_in`, and a struct
  called `Context`

<hr>

Then, we spawn two tasks:

```rust
spawner.must_spawn(dispatch_task(ep_out, dispatch, &mut buffers.rx_buf));
spawner.must_spawn(usb_task(device));
```

Which look like this, basically "just go run forever":

```rust
/// This actually runs the dispatcher
#[embassy_executor::task]
async fn dispatch_task(
    ep_out: Endpoint<'static, USB, Out>,
    dispatch: Dispatcher,
    rx_buf: &'static mut [u8],
) {
    rpc_dispatch(ep_out, dispatch, rx_buf).await;
}

/// This handles the low level USB management
#[embassy_executor::task]
pub async fn usb_task(mut usb: UsbDevice<'static, Driver<'static, USB>>) {
    usb.run().await;
}
```

Hopefully, this all makes sense covering the "setup" and "run" parts of getting the postcard-rpc
stack going:

* We setup the low level hardware, from the `embassy-rp` drivers
* We have a helper function that configures the `embassy-usb` components
* We hand those pieces to *something* from `postcard-rpc`, that uses the `embassy-usb` components

Let's scroll back up to the top of the firmware and see what we skipped:

## Defining our protocol

At the top of `comms-01`, there's some interesting looking code:

```rust
static ALL_BUFFERS: ConstInitCell<AllBuffers<256, 256, 256>> =
    ConstInitCell::new(AllBuffers::new());

pub struct Context {}

define_dispatch! {
    dispatcher: Dispatcher<
        Mutex = ThreadModeRawMutex,
        Driver = usb::Driver<'static, USB>,
        Context = Context
    >;
    PingEndpoint => blocking ping_handler,
}
```

The first part with `ALL_BUFFERS` we've explained a bit: we're using the `ConstInitCell` from
the `static_cell` crate to create a "single use" set of buffers that have static lifetime.

The three `256` values are the size in bytes we give for various parts of the USB and postcard-rpc
stack.

We then define a struct called `Context` with no fields. We'll look into this more soon!

Finally, we call a slightly weird macro called `define_dispatch!`. This comes from the
`postcard-rpc` crate, and we'll break that down a bit.

```rust
dispatcher: Dispatcher<
    Mutex = ThreadModeRawMutex,
    Driver = usb::Driver<'static, USB>,
    Context = Context
>;
```

First, since `postcard-rpc` can work with ANY device that works with `embassy-usb`, we need to
define which types we are using, so the macro can create a **Dispatcher** type for us. The
dispatcher has a couple of responsibilities:

* When we RECEIVE a **Request**, it figures out what *kind* of message it is, and passes that
  message on to a handler, if it knows about that kind of Request.
* If we pass on the message to the handler, we need to **deserialize** the message, so that the
  handler doesn't need to manage that
* When that handler completes, it will return a **Response**. The Dispatcher will then serialize
  that response, and send it back over USB.
* If an error ever occurs, for example if we ever got a message kind we don't understand, or if
  deserialization failed due to message corruption, the dispatcher will automatically send back
  an error response.

> NOTE: this macro *looks* like it's using "associated type" syntax, but it's not really, it's just
macro syntax, so don't read too much into it!

How does this `Dispatcher` know all the kinds of messages it needs to handle? That's what the next
part is for:

```rust
PingEndpoint => blocking ping_handler,
```

This is saying:

* Whenever we get a message on the `PingEndpoint`
* Decode it, and pass it to the `blocking` function called `ping_handler`

If we look lower in our code, we'll find a function that looks like this:

```rust
fn ping_handler(_context: &mut Context, header: WireHeader, rqst: u32) -> u32 {
    info!("ping: seq - {=u32}", header.seq_no);
    rqst
}
```

This handler will be called whenever we receive a `PingEndpoint` request. All `postcard-rpc`
take these three arguments:

* A `&mut` reference to the `Context` type we defined in `define_dispatch`, you can put anything
  you like in this `Context` type!
* The `header` of the request, this includes the Key and sequence number of the request
* The `rqst`, which will be whatever the `Request` type of this `Endpoint` is

This function also returns exactly one thing: whatever the `Response` type of this endpoint.

We can see that our `ping_handler` will return whatever value it received without modification, and
log the sequence number that we saw.

And that's all that's necessary on the firmware side!
