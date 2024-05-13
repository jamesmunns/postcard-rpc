# Streaming with Topics

`Topics` are useful for cases when you either want to send a LOT of data, e.g. streaming raw sensor
values, or cases where you want to rarely send notifications that some event has happened.

## Always Sending

One way you can use Topics is to just always send data, even unprompted. For example, you could
periodically send information like "uptime", or how many milliseconds since the software has
started. For more complex projects, you could include other performance counters, CPU Load, or
memory usage over time.

You'll need to store the time that the program started (check out `Instant` from `embassy-time`!),
and make a task OUTSIDE the dispatcher to do this. Don't forget that the `Dispatcher` struct has
the sender as a field, and it has a a method called `publish()` you can use with
`sender.publish::<YourTopic>(&your_msg).await`.

## Start/Stop sending

You can also pair starting and stopping a stream on a Topic by using an endpoint. You could use
a `spawn` handler to begin streaming, and use a `blocking` or `async` task to signal the task to
stop.

You may need to share some kind of signal, `embassy-sync` has useful data structures you can use
in the `Context` struct, or as a `static`.

Consider setting up some kind of streaming endpoint for the accelerometer using Topics.

Some things to keep in mind:

* How should the host provide the "configuration" values for the stream, like the frequency of
  sampling?
* What to do if an error occurs, and we need to stop the stream without the host asking?
* What to do if the host asks the target to stop, but it had never started?
