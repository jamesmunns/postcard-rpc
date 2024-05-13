#![no_std]

// You'll need the unsued imports soon!
#![allow(unused_imports)]

use postcard::experimental::schema::Schema;
use postcard_rpc::{endpoint, topic};
use serde::{Deserialize, Serialize};

endpoint!(PingEndpoint, u32, u32, "ping");
