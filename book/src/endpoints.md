# Endpoints

We're now entering the more "self directed" part of the workshop! Feel free to ask as many questions
as you'd like, or build the things YOU want to!

A great place to start is by building various endpoints for the different sensors on the board.

For now we'll only focus on Endpoints and address Topics later, but feel free to go ahead if you'd
like!

Some questions to think about include:

* What kind of endpoints, or logical requests make sense for the different parts of the board?
* What kind of data makes sense for a request? Not all requests need to include data!
* What kind of data makes sense for a response? Not all responses need to include data!
* When should we use built in types, like `bool` or `i32`, and when would it make sense to define
  our own types?
* Should our endpoints use blocking handlers? async handlers?

Don't forget, we have lots of parts on our board, and example code for interacting with:

* Buttons
* Potentiometer Dial
* RGB LEDs
* Accelerometer (X, Y, Z)

## Host side

You can definitely start with a basic "demo" app that just makes one kind of request, or sends
request every N milliseconds, X times.

One easy way to make an interactive program is by making a "REPL", or a "Read, Evaluate, Print,
Loop" program. An easy way to do this is using a structure like this:

```rust
#[tokio::main]
async fn main() {
    // Begin repl...
    loop {
        print!("> ");
        stdout().flush().unwrap();
        let line = read_line().await;
        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts.as_slice() {
            ["ping"] => {
                let ping = client.ping(42).await.unwrap();
                println!("Got: {ping}.");
            }
            ["ping", n] => {
                let Ok(idx) = n.parse::<u32>() else {
                    println!("Bad u32: '{n}'");
                    continue;
                };
                let ping = client.ping(idx).await.unwrap();
                println!("Got: {ping}.");
            }
            other => {
                println!("Error, didn't understand '{other:?};");
            }
        }
    }
}

async fn read_line() -> String {
    tokio::task::spawn_blocking(|| {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap();
        line
    })
    .await
    .unwrap()
}
```

You can very quickly build something that feels like a scripting interface, and is usually very
natural feeling for tech-oriented users. These tend to be VERY valuable tools to have when doing
board bringup, or even early factory testing!

Of course, you could also make a command line interface using a crate like `clap`, or even a GUI
application if you know how to already!
