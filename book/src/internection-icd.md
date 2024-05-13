# Interface Control Document

An [Interface Control Document], or ICD, is a systems engineering term for the definition of an
interface for some kind of system.

[Interface Control Document]: https://en.wikipedia.org/wiki/Interface_control_document

In our system, it defines the "project specific" bits of how our two systems will talk to
each other. To start off, there's not much in our `workshop-icd` project. We define a single
`postcard_rpc::Endpoint`, as we read about in the previous section:

```rust
endpoint!(PingEndpoint, u32, u32, "ping");
```

This declares an endpoint, `PingEndpoint`, that takes a `u32` as a Request, a `u32` as a Response,
and a path of "ping".

Next, let's look at our next firmware project, `comms-01`.
